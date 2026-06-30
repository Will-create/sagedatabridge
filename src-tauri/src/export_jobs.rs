use futures_util::TryStreamExt;
use serde::Deserialize;
use serde_json::Value;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager, State, Window};

use crate::analytics::TableXlsxWriter;
use crate::db;
use crate::state::{AppState, ExportJob, Filter};

const EXPORT_PAGE_SIZE: u32 = 2_000;
const XLSX_PART_BYTES: u64 = 450_000_000;
const XLSX_ZIP32_LIMIT_BYTES: u64 = 3_800_000_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableExportSpec {
    pub connection_id: String,
    pub schema: String,
    pub table: String,
    pub filters: Vec<Filter>,
    pub selected_columns: Vec<String>,
    pub format: String,
    pub file_path: String,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn dedupe_key(spec: &TableExportSpec) -> String {
    format!(
        "table:{}:{}:{}:{}:{}:{}",
        spec.connection_id,
        spec.schema,
        spec.table,
        spec.format,
        serde_json::to_string(&spec.filters).unwrap_or_default(),
        serde_json::to_string(&spec.selected_columns).unwrap_or_default()
    )
}

fn emit_job(app: &AppHandle, job: &ExportJob) {
    let _ = app.emit_all("export_job_updated", job.clone());
}

fn update_job<F>(app: &AppHandle, id: &str, update: F) -> Result<ExportJob, String>
where
    F: FnOnce(&mut ExportJob),
{
    let state = app.state::<AppState>();
    let job = {
        let mut jobs = state.export_jobs.lock().map_err(|e| e.to_string())?;
        let job = jobs
            .get_mut(id)
            .ok_or_else(|| format!("Export job '{}' not found", id))?;
        update(job);
        job.clone()
    };
    emit_job(app, &job);
    Ok(job)
}

fn is_cancelled(app: &AppHandle, id: &str) -> bool {
    app.state::<AppState>()
        .export_jobs
        .lock()
        .ok()
        .and_then(|jobs| jobs.get(id).map(|job| job.cancel_requested))
        .unwrap_or(false)
}

fn tmp_path(path: &Path, job_id: &str) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Invalid export path".to_string())?;
    Ok(path.with_file_name(format!(".{}.{}.tmp", name, job_id)))
}

fn cleanup_stale_temp_files(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return;
    };
    let prefix = format!(".{}.", name);
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let candidate = entry.path();
        let matches = candidate
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value.starts_with(&prefix) && value.ends_with(".tmp"))
            .unwrap_or(false);
        if matches {
            let _ = fs::remove_file(candidate);
        }
    }
}

fn replace_file(tmp: &Path, final_path: &Path) -> Result<(), String> {
    if final_path.exists() {
        fs::remove_file(final_path).map_err(|e| format!("Failed to replace export file: {}", e))?;
    }
    fs::rename(tmp, final_path).map_err(|e| format!("Failed to finalize export file: {}", e))
}

fn part_path(path: &Path, part: u32) -> Result<PathBuf, String> {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Invalid export file name".to_string())?;
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("xlsx");
    Ok(parent.join(format!("{}.part-{:03}.{}", stem, part, extension)))
}

fn csv_value(value: &Value) -> String {
    let text = match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        other => other.to_string(),
    };
    if text.contains(',') || text.contains('"') || text.contains('\n') || text.contains('\r') {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

fn sql_value(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(value) => if *value { "1" } else { "0" }.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => format!("N'{}'", value.replace('\'', "''")),
        other => format!("N'{}'", other.to_string().replace('\'', "''")),
    }
}

async fn run_table_export(
    app: AppHandle,
    job_id: String,
    spec: TableExportSpec,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _permit = state
        .export_slots
        .clone()
        .acquire_owned()
        .await
        .map_err(|e| e.to_string())?;
    if is_cancelled(&app, &job_id) {
        update_job(&app, &job_id, |job| {
            job.status = "cancelled".to_string();
            job.phase = "cancelled".to_string();
            job.finished_at = Some(now());
            job.cancellable = false;
        })?;
        return Ok(());
    }
    update_job(&app, &job_id, |job| {
        job.status = "running".to_string();
        job.phase = "counting".to_string();
        job.started_at = Some(now());
    })?;

    let config = state.resolve_connection_config(&spec.connection_id)?;
    let mut client = db::connect(&config).await?;
    let columns = db::get_columns(&mut client, &spec.schema, &spec.table).await?;
    let mut tds = db::open_tds_connection(&config).await?;
    let selected = spec
        .selected_columns
        .iter()
        .collect::<std::collections::HashSet<_>>();
    let indexes = columns
        .iter()
        .enumerate()
        .filter(|(_, column)| selected.is_empty() || selected.contains(&column.name))
        .map(|(index, column)| (index, column.name.clone()))
        .collect::<Vec<_>>();
    if indexes.is_empty() {
        return Err("No columns selected for export".to_string());
    }

    let export_timeout = Duration::from_secs(
        state
            .config
            .lock()
            .ok()
            .map(|config| config.query_timeout_secs)
            .unwrap_or(db::DEFAULT_QUERY_TIMEOUT_SECS)
            .max(300),
    );
    let first = db::get_table_data_tds(
        &mut tds,
        &spec.schema,
        &spec.table,
        0,
        1,
        &spec.filters,
        &columns,
        export_timeout,
    )
    .await?;
    let total_rows = first.total_count;
    update_job(&app, &job_id, |job| {
        job.total_rows = total_rows;
        job.phase = "writing".to_string();
    })?;

    let final_path = PathBuf::from(&spec.file_path);
    cleanup_stale_temp_files(&final_path);
    let mut temp_path = tmp_path(&final_path, &job_id)?;
    let _ = fs::remove_file(&temp_path);
    let file =
        File::create(&temp_path).map_err(|e| format!("Failed to create export file: {}", e))?;
    let mut writer = Some(BufWriter::with_capacity(8 * 1024 * 1024, file));
    let headers = indexes
        .iter()
        .map(|(_, name)| name.clone())
        .collect::<Vec<_>>();
    let full_table = format!(
        "[{}].[{}]",
        spec.schema.replace(']', "]]"),
        spec.table.replace(']', "]]")
    );

    let mut xlsx_writer = if spec.format == "xlsx" {
        Some(TableXlsxWriter::new(
            writer.take().unwrap(),
            headers.clone(),
        )?)
    } else {
        None
    };
    let mut xlsx_part = 1u32;
    let mut output_paths = Vec::new();
    let mut completed_bytes = 0u64;
    match spec.format.as_str() {
        "csv" => writeln!(
            writer.as_mut().unwrap(),
            "{}",
            headers
                .iter()
                .map(|value| csv_value(&Value::String(value.clone())))
                .collect::<Vec<_>>()
                .join(",")
        )
        .map_err(|e| e.to_string())?,
        "json" => writer
            .as_mut()
            .unwrap()
            .write_all(b"[\n")
            .map_err(|e| e.to_string())?,
        "sql" => writeln!(
            writer.as_mut().unwrap(),
            "-- Export of {}\n-- Generated by Sage Data Bridge\n",
            full_table
        )
        .map_err(|e| e.to_string())?,
        "xlsx" => {}
        _ => return Err("Unsupported backend export format".to_string()),
    }

    let mut processed = 0i64;
    let mut first_json = true;
    let select_columns = headers
        .iter()
        .map(|name| format!("[{}]", name.replace(']', "]]")))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {} FROM {} {}",
        select_columns,
        full_table,
        db::build_where_clause(&spec.filters)
    );
    let stream = tokio::time::timeout(export_timeout, tds.simple_query(sql))
        .await
        .map_err(|_| {
            format!(
                "Export query timed out after {} seconds",
                export_timeout.as_secs()
            )
        })?
        .map_err(|e| format!("Export query failed: {}", e))?;
    let mut rows = stream.into_row_stream();

    while let Some(row) = tokio::time::timeout(export_timeout, rows.try_next())
        .await
        .map_err(|_| {
            format!(
                "Export result read timed out after {} seconds",
                export_timeout.as_secs()
            )
        })?
        .map_err(|e| format!("Export result read failed: {}", e))?
    {
        if is_cancelled(&app, &job_id) {
            let _ = fs::remove_file(&temp_path);
            update_job(&app, &job_id, |job| {
                job.status = "cancelled".to_string();
                job.phase = "cancelled".to_string();
                job.finished_at = Some(now());
                job.cancellable = false;
            })?;
            return Ok(());
        }

        let values = (0..headers.len())
            .map(|index| db::cell_to_json_tds(&row, index))
            .collect::<Vec<_>>();
        match spec.format.as_str() {
            "csv" => writeln!(
                writer.as_mut().unwrap(),
                "{}",
                values.iter().map(csv_value).collect::<Vec<_>>().join(",")
            )
            .map_err(|e| e.to_string())?,
            "json" => {
                if !first_json {
                    writer
                        .as_mut()
                        .unwrap()
                        .write_all(b",\n")
                        .map_err(|e| e.to_string())?;
                }
                let object = headers
                    .iter()
                    .cloned()
                    .zip(values.into_iter())
                    .collect::<serde_json::Map<_, _>>();
                serde_json::to_writer(writer.as_mut().unwrap(), &object)
                    .map_err(|e| e.to_string())?;
                first_json = false;
            }
            "sql" => writeln!(
                writer.as_mut().unwrap(),
                "INSERT INTO {} ({}) VALUES ({});",
                full_table,
                headers
                    .iter()
                    .map(|name| format!("[{}]", name.replace(']', "]]")))
                    .collect::<Vec<_>>()
                    .join(", "),
                values.iter().map(sql_value).collect::<Vec<_>>().join(", ")
            )
            .map_err(|e| e.to_string())?,
            "xlsx" => {
                let cells = values
                    .iter()
                    .map(|value| match value {
                        Value::Null => String::new(),
                        Value::String(value) => value.clone(),
                        other => other.to_string(),
                    })
                    .collect::<Vec<_>>();
                let numeric = values
                    .iter()
                    .map(|value| value.is_number())
                    .collect::<Vec<_>>();
                xlsx_writer.as_mut().unwrap().write_row(&cells, &numeric)?;
            }
            _ => unreachable!(),
        }

        processed += 1;
        if processed % EXPORT_PAGE_SIZE as i64 != 0 {
            continue;
        }
        if let Some(writer) = writer.as_mut() {
            writer
                .flush()
                .map_err(|e| format!("Failed to flush export file: {}", e))?;
        }
        if let Some(writer) = xlsx_writer.as_mut() {
            writer.flush()?;
        }
        let bytes = fs::metadata(&temp_path).map(|meta| meta.len()).unwrap_or(0);
        update_job(&app, &job_id, |job| {
            job.processed_rows = processed;
            job.bytes_written = completed_bytes.saturating_add(bytes);
            job.percent = if job.total_rows <= 0 {
                99
            } else {
                ((processed.min(job.total_rows) * 100) / job.total_rows).clamp(0, 99) as u8
            };
        })?;
        if spec.format == "xlsx" && bytes >= XLSX_PART_BYTES && processed < total_rows {
            xlsx_writer.take().unwrap().finish()?;
            let completed_path = part_path(&final_path, xlsx_part)?;
            replace_file(&temp_path, &completed_path)?;
            completed_bytes = completed_bytes.saturating_add(bytes);
            output_paths.push(completed_path.display().to_string());
            xlsx_part += 1;
            let next_path = part_path(&final_path, xlsx_part)?;
            temp_path = tmp_path(&next_path, &job_id)?;
            let file = File::create(&temp_path)
                .map_err(|e| format!("Failed to create Excel part file: {}", e))?;
            xlsx_writer = Some(TableXlsxWriter::new(
                BufWriter::with_capacity(8 * 1024 * 1024, file),
                headers.clone(),
            )?);
            update_job(&app, &job_id, |job| {
                job.phase = format!("writing Excel part {}", xlsx_part);
                job.output_paths = output_paths.clone();
            })?;
        }
    }

    if let Some(writer) = writer.as_mut() {
        writer
            .flush()
            .map_err(|e| format!("Failed to flush export file: {}", e))?;
    }
    if let Some(writer) = xlsx_writer.as_mut() {
        writer.flush()?;
    }
    let bytes = fs::metadata(&temp_path).map(|meta| meta.len()).unwrap_or(0);
    if spec.format == "xlsx" && bytes >= XLSX_ZIP32_LIMIT_BYTES {
        return Err("An Excel part exceeded the safe ZIP32 size limit".to_string());
    }
    update_job(&app, &job_id, |job| {
        job.processed_rows = processed;
        job.bytes_written = completed_bytes.saturating_add(bytes);
        job.percent = 99;
        job.phase = "finalizing".to_string();
    })?;

    if spec.format == "json" {
        writer
            .as_mut()
            .unwrap()
            .write_all(b"\n]\n")
            .map_err(|e| e.to_string())?;
    }
    if let Some(writer) = xlsx_writer {
        writer.finish()?;
    } else {
        writer
            .as_mut()
            .unwrap()
            .flush()
            .map_err(|e| format!("Failed to flush export file: {}", e))?;
        drop(writer);
    }
    let completed_path = if spec.format == "xlsx" && !output_paths.is_empty() {
        part_path(&final_path, xlsx_part)?
    } else {
        final_path.clone()
    };
    replace_file(&temp_path, &completed_path)?;
    let final_bytes = fs::metadata(&completed_path)
        .map(|meta| meta.len())
        .unwrap_or(0);
    output_paths.push(completed_path.display().to_string());
    update_job(&app, &job_id, |job| {
        job.status = "completed".to_string();
        job.phase = "completed".to_string();
        job.percent = 100;
        job.bytes_written = completed_bytes.saturating_add(final_bytes);
        job.finished_at = Some(now());
        job.cancellable = false;
        job.output_paths = output_paths;
    })?;
    Ok(())
}

#[tauri::command]
pub async fn start_table_export(
    window: Window,
    state: State<'_, AppState>,
    spec: TableExportSpec,
) -> Result<ExportJob, String> {
    let key = dedupe_key(&spec);
    {
        let jobs = state.export_jobs.lock().map_err(|e| e.to_string())?;
        if let Some(job) = jobs.values().find(|job| {
            job.dedupe_key == key && matches!(job.status.as_str(), "queued" | "running")
        }) {
            return Ok(job.clone());
        }
        if jobs.values().any(|job| {
            matches!(job.status.as_str(), "queued" | "running")
                && job.output_paths.iter().any(|path| path == &spec.file_path)
        }) {
            return Err("Another export is already writing to this output path".to_string());
        }
    }
    let job = ExportJob {
        id: uuid::Uuid::new_v4().to_string(),
        dedupe_key: key,
        kind: format!("table-{}", spec.format),
        label: format!(
            "{}.{} ({})",
            spec.schema,
            spec.table,
            spec.format.to_uppercase()
        ),
        status: "queued".to_string(),
        phase: "queued".to_string(),
        processed_rows: 0,
        total_rows: 0,
        bytes_written: 0,
        percent: 0,
        output_paths: vec![spec.file_path.clone()],
        created_at: now(),
        started_at: None,
        finished_at: None,
        error: None,
        cancellable: true,
        cancel_requested: false,
    };
    {
        let mut jobs = state.export_jobs.lock().map_err(|e| e.to_string())?;
        if jobs.len() >= 100 {
            let mut terminal = jobs
                .values()
                .filter(|item| !matches!(item.status.as_str(), "queued" | "running"))
                .map(|item| (item.created_at.clone(), item.id.clone()))
                .collect::<Vec<_>>();
            terminal.sort();
            for (_, id) in terminal.into_iter().take(jobs.len().saturating_sub(99)) {
                jobs.remove(&id);
            }
        }
        jobs.insert(job.id.clone(), job.clone());
    }
    emit_job(&window.app_handle(), &job);
    let app = window.app_handle();
    let job_id = job.id.clone();
    let temp_file_path = tmp_path(Path::new(&spec.file_path), &job.id).ok();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_table_export(app.clone(), job_id.clone(), spec).await {
            if let Some(path) = temp_file_path {
                let _ = fs::remove_file(path);
            }
            let _ = update_job(&app, &job_id, |job| {
                job.status = "failed".to_string();
                job.phase = "failed".to_string();
                job.error = Some(error);
                job.finished_at = Some(now());
                job.cancellable = false;
            });
        }
    });
    Ok(job)
}

#[tauri::command]
pub fn list_export_jobs(state: State<'_, AppState>) -> Result<Vec<ExportJob>, String> {
    let mut jobs = state
        .export_jobs
        .lock()
        .map_err(|e| e.to_string())?
        .values()
        .cloned()
        .collect::<Vec<_>>();
    jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(jobs)
}

#[tauri::command]
pub fn cancel_export_job(
    window: Window,
    state: State<'_, AppState>,
    job_id: String,
) -> Result<(), String> {
    let job = {
        let mut jobs = state.export_jobs.lock().map_err(|e| e.to_string())?;
        let job = jobs
            .get_mut(&job_id)
            .ok_or_else(|| format!("Export job '{}' not found", job_id))?;
        if matches!(job.status.as_str(), "queued" | "running") {
            job.cancel_requested = true;
            job.phase = "cancelling".to_string();
        }
        job.clone()
    };
    emit_job(&window.app_handle(), &job);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{csv_value, part_path, sql_value};
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn quotes_csv_and_sql_values() {
        assert_eq!(csv_value(&json!("a,\"b\"")), "\"a,\"\"b\"\"\"");
        assert_eq!(sql_value(&json!("l'entreprise")), "N'l''entreprise'");
        assert_eq!(sql_value(&serde_json::Value::Null), "NULL");
    }

    #[test]
    fn creates_excel_part_paths() {
        assert_eq!(
            part_path(Path::new("/tmp/export.xlsx"), 3)
                .unwrap()
                .to_string_lossy(),
            "/tmp/export.part-003.xlsx"
        );
    }
}
