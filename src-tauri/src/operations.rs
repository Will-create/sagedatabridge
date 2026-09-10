use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use tauri::{AppHandle, Manager, State, Window};

use crate::db;
use crate::state::{AppState, ColumnInfo, ConnectionConfig, RelationshipInfo};

const MAX_OPERATION_JOBS: usize = 100;
const MAX_OPERATION_LOGS: usize = 200;
const DEFAULT_BATCH_SIZE: u32 = 500;
const MAX_BATCH_SIZE: u32 = 500;
const OPERATION_TIMEOUT_SECS: u64 = 6 * 60 * 60;
const FALLBACK_BACKUP_DIRECTORY: &str = r"C:\Temp\SageDataBridgeBackups";

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn default_batch_size() -> u32 {
    DEFAULT_BATCH_SIZE
}

fn default_conflict_strategy() -> String {
    "fail".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferTableSpec {
    pub schema: String,
    pub table: String,
    #[serde(default)]
    pub target_schema: String,
    #[serde(default)]
    pub target_table: String,
}

impl TransferTableSpec {
    fn source_key(&self) -> String {
        format!("{}.{}", self.schema, self.table)
    }

    fn target_schema(&self) -> &str {
        if self.target_schema.trim().is_empty() {
            &self.schema
        } else {
            &self.target_schema
        }
    }

    fn target_table(&self) -> &str {
        if self.target_table.trim().is_empty() {
            &self.table
        } else {
            &self.target_table
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferSpec {
    pub source_connection_id: String,
    pub target_connection_id: String,
    #[serde(default)]
    pub source_database: String,
    #[serde(default)]
    pub target_database: String,
    pub tables: Vec<TransferTableSpec>,
    #[serde(default = "default_batch_size")]
    pub batch_size: u32,
    #[serde(default = "default_conflict_strategy")]
    pub conflict_strategy: String,
    #[serde(default)]
    pub confirmation_text: String,
    #[serde(default)]
    pub edition_mismatch_confirmation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSpec {
    pub connection_id: String,
    #[serde(default)]
    pub database: String,
    pub backup_path: String,
    #[serde(default)]
    pub copy_only: bool,
    #[serde(default)]
    pub confirmation_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreSpec {
    pub connection_id: String,
    pub backup_path: String,
    pub target_database: String,
    #[serde(default)]
    pub replace_existing: bool,
    #[serde(default)]
    pub confirmation_text: String,
    #[serde(default)]
    pub salvage: bool,
    #[serde(default)]
    pub repair_level: String,
    #[serde(default)]
    pub allow_data_loss_confirmation: String,
    #[serde(default)]
    pub extra_backup_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInspection {
    pub classification: String,
    pub salvageable: bool,
    pub unrecoverable: bool,
    pub header_ok: bool,
    pub file_list_ok: bool,
    pub verify_ok: bool,
    pub suggested_target: String,
    pub database_name: String,
    pub backup_name: String,
    pub backup_finish_date: String,
    pub backup_size: String,
    #[serde(default)]
    pub family_count: u32,
    #[serde(default)]
    pub family_sequence: u32,
    #[serde(default)]
    pub media_set_id: String,
    #[serde(default)]
    pub server_name: String,
    #[serde(default)]
    pub provided_families: Vec<u32>,
    #[serde(default)]
    pub missing_families: Vec<u32>,
    pub messages: Vec<String>,
    pub header: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupStagePlan {
    pub source_path: String,
    pub destination_path: String,
    pub bytes: u64,
    pub sql_account: String,
    pub local_host: bool,
    pub needs_stage: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationTemplate {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub spec: Value,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogEntry {
    pub at: String,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationTableProgress {
    pub source_schema: String,
    pub source_table: String,
    pub target_schema: String,
    pub target_table: String,
    pub expected_rows: i64,
    pub transferred_rows: i64,
    pub validated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationJob {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub spec: Value,
    pub status: String,
    pub phase: String,
    pub processed_rows: i64,
    pub total_rows: i64,
    pub bytes_written: u64,
    pub percent: u8,
    pub output_paths: Vec<String>,
    pub table_progress: Vec<OperationTableProgress>,
    pub logs: Vec<OperationLogEntry>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error: Option<String>,
    pub cancellable: bool,
    pub cancel_requested: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationTableCheck {
    pub source_schema: String,
    pub source_table: String,
    pub target_schema: String,
    pub target_table: String,
    pub source_rows: i64,
    pub target_rows: i64,
    pub primary_key: Vec<String>,
    pub compatible: bool,
    pub messages: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub target_exists: bool,
    #[serde(default)]
    pub has_identity: bool,
    #[serde(default)]
    pub trigger_count: i64,
    #[serde(default)]
    pub missing_parent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationPreflight {
    pub kind: String,
    pub allowed: bool,
    pub summary: String,
    pub required_confirmation: String,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub tables: Vec<OperationTableCheck>,
    pub ordered_tables: Vec<TransferTableSpec>,
    #[serde(default)]
    pub source_edition: String,
    #[serde(default)]
    pub target_edition: String,
    #[serde(default)]
    pub path_host: String,
    #[serde(default)]
    pub backup_header: Option<Value>,
    #[serde(default)]
    pub suggested_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlServerPathEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub is_backup: bool,
    pub size_bytes: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SqlServerPathListing {
    pub host: String,
    pub instance_name: String,
    pub local_host: bool,
    pub directory: String,
    pub default_backup_directory: String,
    pub entries: Vec<SqlServerPathEntry>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationTableInfo {
    pub schema: String,
    pub name: String,
    pub full_name: String,
    pub row_count: Option<i64>,
    pub has_primary_key: bool,
}

fn quote_identifier(value: &str) -> String {
    format!("[{}]", value.replace(']', "]]"))
}

fn quote_table(schema: &str, table: &str) -> String {
    format!("{}.{}", quote_identifier(schema), quote_identifier(table))
}

fn quote_sql_string(value: &str) -> String {
    format!("N'{}'", value.replace('\'', "''"))
}

fn host_leaf(host: &str) -> String {
    host.trim()
        .split(['\\', '/'])
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches('.')
        .trim_matches(['\\', '/'])
        .to_ascii_lowercase()
}

pub fn is_local_sql_host(host: &str) -> bool {
    let leaf = host_leaf(host);
    if leaf.is_empty() || matches!(leaf.as_str(), "localhost" | "127.0.0.1" | "::1" | "(local)") {
        return true;
    }
    let computer = std::env::var("COMPUTERNAME").unwrap_or_default().to_ascii_lowercase();
    !computer.is_empty() && (leaf == computer || host.trim().eq_ignore_ascii_case(&computer))
}

pub fn is_backup_file_path(path: &str) -> bool {
    let trimmed = path.trim();
    !trimmed.is_empty()
        && !trimmed.contains('\n')
        && !trimmed.contains('\r')
        && trimmed.to_ascii_lowercase().ends_with(".bak")
}

pub fn is_user_profile_path(path: &str) -> bool {
    let normalized = path.replace('/', "\\").to_ascii_lowercase();
    normalized.contains("\\users\\")
        || normalized.contains("\\documents and settings\\")
        || normalized.contains("\\onedrive")
}

pub fn salvage_target_allowed(name: &str) -> bool {
    name.to_ascii_lowercase().contains("_repaired")
}

pub fn repaired_database_name(source: &str) -> String {
    let base = source.trim();
    if base.is_empty() {
        "database_repaired".to_string()
    } else if salvage_target_allowed(base) {
        base.to_string()
    } else {
        format!("{base}_repaired")
    }
}

pub fn classify_backup(header_ok: bool, backup_name: &str, verify_ok: bool, header_error: &str, verify_error: &str, family_count: u32, provided_families: u32) -> &'static str {
    let combined = format!("{header_error} {verify_error}").to_ascii_lowercase();
    if combined.contains("operating system error 5") || combined.contains("access is denied") || combined.contains("native error: 3201") {
        return "access_denied";
    }
    if !header_ok {
        return "header_destroyed";
    }
    if backup_name.to_ascii_uppercase().contains("INCOMPLETE") {
        return "incomplete";
    }
    if (family_count > 1 && provided_families < family_count)
        || (combined.contains("media families but only") && provided_families < family_count.max(1))
    {
        return "missing_media_family";
    }
    if verify_ok {
        return "healthy";
    }
    "checksum_damage"
}

pub fn parse_media_family_counts(error: &str) -> Option<(u32, u32)> {
    let lower = error.to_ascii_lowercase();
    let after_has = lower.split("has ").nth(1)?;
    let family_count = after_has.split_whitespace().next()?.parse().ok()?;
    let after_only = after_has.split("only ").nth(1)?;
    let provided = after_only
        .split_whitespace()
        .next()
        .and_then(|token| token.chars().take_while(|character| character.is_ascii_digit()).collect::<String>().parse().ok())?;
    Some((family_count, provided))
}

fn backup_media_paths(primary: &str, extra: &[String]) -> Vec<String> {
    let mut paths = Vec::new();
    for path in std::iter::once(primary).chain(extra.iter().map(String::as_str)) {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !paths.iter().any(|existing: &String| existing.eq_ignore_ascii_case(trimmed)) {
            paths.push(trimmed.to_string());
        }
    }
    paths
}

fn from_disks(paths: &[String]) -> String {
    paths.iter().map(|path| format!("DISK = {}", quote_sql_string(path))).collect::<Vec<_>>().join(", ")
}

fn friendly_sql_path_error(error: &str, path: &str, suggested: &str) -> String {
    let lower = error.to_ascii_lowercase();
    let access_denied = lower.contains("operating system error 5")
        || lower.contains("access is denied")
        || lower.contains("native error: 3201")
        || lower.contains("cannot open backup device");
    if !access_denied {
        return error.to_string();
    }
    let suggestion = if suggested.trim().is_empty() {
        "the SQL Server instance backup folder (not a Desktop or Documents folder)".to_string()
    } else {
        format!("this server path instead: {suggested}")
    };
    format!(
        "SQL Server cannot write '{path}'. Operating system error 5 (Access is denied): the SQL Server service account, not your Windows login, must be able to write this folder. Desktop and user-profile folders are almost never writable. Use {suggestion}."
    )
}

fn join_sql_path(base: &str, name: &str) -> String {
    if base.ends_with('\\') || base.ends_with('/') {
        format!("{base}{name}")
    } else if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}\\{name}")
    }
}

fn parent_sql_path(path: &str) -> String {
    let trimmed = path.trim().trim_end_matches(['\\', '/']);
    if let Some(index) = trimmed.rfind('\\') {
        let parent = &trimmed[..index];
        if parent.is_empty() || parent == "\\" {
            return trimmed.to_string();
        }
        parent.to_string()
    } else {
        trimmed.to_string()
    }
}

fn transfer_same_database(spec: &TransferSpec, source_db: &str, target_db: &str) -> bool {
    spec.source_connection_id == spec.target_connection_id && source_db.eq_ignore_ascii_case(target_db)
}

fn connection_for_operation(state: &AppState, connection_id: &str, database: &str) -> Result<ConnectionConfig, String> {
    let mut config = state.resolve_connection_config(connection_id)?;
    if !database.trim().is_empty() {
        config.database = database.trim().to_string();
    }
    Ok(config)
}

fn resolved_database(config: &ConnectionConfig, override_database: &str) -> String {
    if override_database.trim().is_empty() {
        config.database.clone()
    } else {
        override_database.trim().to_string()
    }
}

fn would_truncate(source: &ColumnInfo, target: &ColumnInfo) -> bool {
    match (source.max_length, target.max_length) {
        (Some(source_len), Some(target_len)) if source_len > 0 && target_len > 0 && target_len < source_len => true,
        _ => false,
    }
}

fn would_lose_precision(source: &ColumnInfo, target: &ColumnInfo) -> bool {
    if !matches!(normalized_type(source).as_str(), "decimal" | "numeric" | "money" | "smallmoney") {
        return false;
    }
    let source_precision = source.precision.unwrap_or(0);
    let target_precision = target.precision.unwrap_or(0);
    let source_scale = source.scale.unwrap_or(0);
    let target_scale = target.scale.unwrap_or(0);
    (target_precision > 0 && source_precision > 0 && target_precision < source_precision)
        || (source_scale > 0 && target_scale < source_scale)
}

fn sql_value(value: &Value, column: &ColumnInfo) -> String {
    if value.is_null() {
        return "NULL".to_string();
    }
    let data_type = column.data_type.to_ascii_lowercase();
    if matches!(data_type.as_str(), "binary" | "varbinary" | "image" | "timestamp" | "rowversion") {
        if let Some(value) = value.as_str() {
            if value.starts_with("0x") {
                return value.to_string();
            }
        }
    }
    match value {
        Value::Bool(value) => if *value { "1".to_string() } else { "0".to_string() },
        Value::Number(value) => value.to_string(),
        Value::String(value) => quote_sql_string(value),
        other => quote_sql_string(&other.to_string()),
    }
}

fn count_from_query(data: &crate::state::TableData) -> i64 {
    data.rows
        .first()
        .and_then(|row| row.first())
        .and_then(|value| value.as_i64().or_else(|| value.as_bool().map(i64::from)).or_else(|| value.as_str().and_then(|value| value.parse().ok())))
        .unwrap_or(0)
}

async fn execute_operation_sql(
    client: &mut db::DbClient,
    sql: &str,
    label: &str,
) -> Result<crate::state::TableData, String> {
    db::execute_logged_query(
        client,
        sql,
        db::QueryExecutionContext::new("operations", label).with_timeout_secs(OPERATION_TIMEOUT_SECS),
    )
    .await
}

async fn table_count(client: &mut db::DbClient, schema: &str, table: &str) -> Result<i64, String> {
    let sql = format!("SELECT COUNT_BIG(*) AS total_rows FROM {}", quote_table(schema, table));
    let data = db::execute_raw_query(client, &sql).await?;
    Ok(count_from_query(&data))
}

fn normalized_type(column: &ColumnInfo) -> String {
    match column.data_type.to_ascii_lowercase().as_str() {
        "numeric" => "decimal".to_string(),
        "integer" => "int".to_string(),
        value => value.to_string(),
    }
}

fn unsupported_type(column: &ColumnInfo) -> bool {
    matches!(
        normalized_type(column).as_str(),
        "rowversion" | "timestamp" | "sql_variant" | "hierarchyid" | "geography" | "geometry"
    )
}

fn table_order(tables: &[TransferTableSpec], relationships: &[RelationshipInfo]) -> Result<Vec<TransferTableSpec>, String> {
    let selected = tables.iter().map(TransferTableSpec::source_key).collect::<HashSet<_>>();
    let mut edges: HashMap<String, HashSet<String>> = selected.iter().map(|key| (key.clone(), HashSet::new())).collect();
    let mut indegree: HashMap<String, usize> = selected.iter().map(|key| (key.clone(), 0)).collect();

    for relationship in relationships {
        let child = format!("{}.{}", relationship.source_schema, relationship.source_table);
        let parent = format!("{}.{}", relationship.target_schema, relationship.target_table);
        if selected.contains(&child) && selected.contains(&parent) && parent != child {
            let inserted = edges.get_mut(&parent).expect("selected parent").insert(child.clone());
            if inserted {
                *indegree.get_mut(&child).expect("selected child") += 1;
            }
        }
    }

    let mut ready = indegree
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    ready.sort();
    let mut queue = VecDeque::from(ready);
    let mut ordered_keys = Vec::new();
    while let Some(key) = queue.pop_front() {
        ordered_keys.push(key.clone());
        let mut children = edges.remove(&key).unwrap_or_default().into_iter().collect::<Vec<_>>();
        children.sort();
        for child in children {
            let count = indegree.get_mut(&child).expect("selected child");
            *count -= 1;
            if *count == 0 {
                queue.push_back(child);
            }
        }
    }
    if ordered_keys.len() != tables.len() {
        return Err("Selected tables contain a foreign-key cycle. Transfer a compatible cycle manually after disabling constraints under an approved maintenance procedure.".to_string());
    }
    let by_key = tables.iter().map(|table| (table.source_key(), table.clone())).collect::<HashMap<_, _>>();
    Ok(ordered_keys.into_iter().filter_map(|key| by_key.get(&key).cloned()).collect())
}

fn empty_preflight(kind: &str, summary: &str, errors: Vec<String>, warnings: Vec<String>) -> OperationPreflight {
    OperationPreflight {
        kind: kind.to_string(),
        allowed: false,
        summary: summary.to_string(),
        required_confirmation: String::new(),
        warnings,
        errors,
        tables: Vec::new(),
        ordered_tables: Vec::new(),
        source_edition: String::new(),
        target_edition: String::new(),
        path_host: String::new(),
        backup_header: None,
        suggested_path: String::new(),
    }
}

async fn object_count(client: &mut db::DbClient, sql: &str) -> i64 {
    db::execute_raw_query(client, sql).await.map(|data| count_from_query(&data)).unwrap_or(0)
}

async fn preflight_transfer_inner(
    state: &AppState,
    spec: &TransferSpec,
    resume_progress: Option<&[OperationTableProgress]>,
) -> Result<OperationPreflight, String> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut checks = Vec::new();
    if spec.source_connection_id.trim().is_empty() || spec.target_connection_id.trim().is_empty() {
        errors.push("Select both a source and a target connection.".to_string());
    }
    if spec.tables.is_empty() {
        errors.push("Select at least one table to transfer.".to_string());
    }
    if spec.conflict_strategy != "fail" {
        errors.push("v7 only permits the safe 'fail on existing data' conflict strategy.".to_string());
    }
    if !errors.is_empty() {
        return Ok(empty_preflight("transfer", "Transfer cannot start.", errors, warnings));
    }

    let source_config = connection_for_operation(state, &spec.source_connection_id, &spec.source_database)?;
    let target_config = connection_for_operation(state, &spec.target_connection_id, &spec.target_database)?;
    let source_db = resolved_database(&source_config, &spec.source_database);
    let target_db = resolved_database(&target_config, &spec.target_database);
    if source_db.trim().is_empty() || target_db.trim().is_empty() {
        errors.push("Select a source database and a target database.".to_string());
        return Ok(empty_preflight("transfer", "Transfer cannot start.", errors, warnings));
    }
    if transfer_same_database(spec, &source_db, &target_db) {
        errors.push("Source and target must be different databases.".to_string());
        return Ok(empty_preflight("transfer", "Transfer cannot start.", errors, warnings));
    }
    let mut source_config = source_config;
    let mut target_config = target_config;
    source_config.database = source_db.clone();
    target_config.database = target_db.clone();
    db::test_connection(&source_config).await.map_err(|error| format!("Source connection failed: {error}"))?;
    db::test_connection(&target_config).await.map_err(|error| format!("Target connection failed: {error}"))?;

    let mut source_client = db::connect(&source_config).await?;
    let mut target_client = db::connect(&target_config).await?;
    let source_edition = crate::commands::detect_sage_schema(&mut source_client).await.ok().map(|result| result.schema.edition.as_str().to_string()).unwrap_or_default();
    let target_edition = crate::commands::detect_sage_schema(&mut target_client).await.ok().map(|result| result.schema.edition.as_str().to_string()).unwrap_or_default();
    if !source_edition.is_empty() && !target_edition.is_empty() && source_edition != target_edition && source_edition != "generic" && target_edition != "generic" {
        if spec.edition_mismatch_confirmation.trim() == "EDITION-MISMATCH" {
            warnings.push(format!("Sage editions differ ({source_edition} → {target_edition}). Copying anyway after explicit confirmation."));
        } else {
            errors.push(format!("Sage editions differ ({source_edition} → {target_edition}). Type EDITION-MISMATCH only if this copy is approved."));
        }
    }

    let relationships = db::get_relationships(&mut source_client).await?;
    let selected = spec.tables.iter().map(TransferTableSpec::source_key).collect::<HashSet<_>>();
    let mut missing_parents: HashSet<String> = HashSet::new();
    for relationship in &relationships {
        let child = format!("{}.{}", relationship.source_schema, relationship.source_table);
        let parent = format!("{}.{}", relationship.target_schema, relationship.target_table);
        if selected.contains(&child) && !selected.contains(&parent) {
            warnings.push(format!("Child {child} is selected without parent {parent}."));
            missing_parents.insert(child);
        }
    }
    let ordered_tables = match table_order(&spec.tables, &relationships) {
        Ok(tables) => tables,
        Err(error) => {
            errors.push(error);
            Vec::new()
        }
    };

    let resume_by_source = resume_progress
        .unwrap_or_default()
        .iter()
        .map(|progress| (format!("{}.{}", progress.source_schema, progress.source_table), progress))
        .collect::<HashMap<_, _>>();

    for table in &spec.tables {
        let mut messages = Vec::new();
        let mut table_warnings = Vec::new();
        let mut compatible = true;
        if table.schema.trim().is_empty() || table.table.trim().is_empty() {
            errors.push("Each selected table requires a schema and table name.".to_string());
            continue;
        }
        let source_columns = db::get_columns(&mut source_client, &table.schema, &table.table).await?;
        let target_columns = match db::get_columns(&mut target_client, table.target_schema(), table.target_table()).await {
            Ok(columns) => columns,
            Err(_) => {
                compatible = false;
                messages.push("Target table does not exist. The app will not create it.".to_string());
                Vec::new()
            }
        };
        let target_exists = !target_columns.is_empty() || messages.is_empty();
        let source_rows = table_count(&mut source_client, &table.schema, &table.table).await.unwrap_or(0);
        let target_rows = if target_columns.is_empty() && !target_exists {
            0
        } else {
            table_count(&mut target_client, table.target_schema(), table.target_table()).await.unwrap_or(0)
        };
        let primary_key = source_columns.iter().filter(|column| column.is_primary_key).map(|column| column.name.clone()).collect::<Vec<_>>();
        if primary_key.is_empty() {
            compatible = false;
            messages.push("A primary key is required for deterministic checkpoints and safe resume.".to_string());
        }
        let target_by_name = target_columns.iter().map(|column| (column.name.to_ascii_lowercase(), column)).collect::<HashMap<_, _>>();
        for source_column in &source_columns {
            if unsupported_type(source_column) {
                compatible = false;
                messages.push(format!("Column '{}' uses unsupported type '{}'.", source_column.name, source_column.data_type));
            }
            match target_by_name.get(&source_column.name.to_ascii_lowercase()) {
                Some(target_column) if normalized_type(source_column) == normalized_type(target_column) => {
                    if would_truncate(source_column, target_column) {
                        compatible = false;
                        messages.push(format!("Column '{}' would truncate ({} → {}).", source_column.name, source_column.max_length.unwrap_or(0), target_column.max_length.unwrap_or(0)));
                    }
                    if would_lose_precision(source_column, target_column) {
                        compatible = false;
                        messages.push(format!("Column '{}' would lose numeric precision.", source_column.name));
                    }
                }
                Some(target_column) => {
                    compatible = false;
                    messages.push(format!("Column '{}' type differs ({} vs {}).", source_column.name, source_column.data_type, target_column.data_type));
                }
                None if !target_columns.is_empty() || target_exists => {
                    compatible = false;
                    messages.push(format!("Target is missing column '{}'.", source_column.name));
                }
                None => {}
            }
        }
        let source_names = source_columns.iter().map(|column| column.name.to_ascii_lowercase()).collect::<HashSet<_>>();
        for target_column in &target_columns {
            if source_names.contains(&target_column.name.to_ascii_lowercase()) {
                continue;
            }
            if !target_column.is_nullable {
                compatible = false;
                messages.push(format!("Target requires non-null column '{}' that is not supplied by the source.", target_column.name));
            } else {
                table_warnings.push(format!("Target extra column '{}' will stay NULL.", target_column.name));
            }
        }
        let expected_resume_rows = resume_by_source.get(&table.source_key()).map(|progress| progress.transferred_rows).unwrap_or(0);
        if resume_progress.is_some() {
            if target_rows != expected_resume_rows {
                compatible = false;
                messages.push(format!("Target has {} rows but checkpoint records {}; resume is unsafe.", target_rows, expected_resume_rows));
            }
        } else if target_rows > 0 {
            compatible = false;
            messages.push(format!("Target already contains {} rows; safe mode blocks writes.", target_rows));
        }
        if source_rows == 0 {
            table_warnings.push("Source table is empty. Confirm this is the intended table.".to_string());
        }
        if source_rows > 1_000_000 {
            table_warnings.push("Table has more than 1,000,000 rows. Take a copy-only backup before copying.".to_string());
        }
        let identity_sql = format!(
            "SELECT COUNT(*) FROM sys.identity_columns ic INNER JOIN sys.tables t ON ic.object_id = t.object_id INNER JOIN sys.schemas s ON t.schema_id = s.schema_id WHERE s.name = {} AND t.name = {}",
            quote_sql_string(table.target_schema()), quote_sql_string(table.target_table())
        );
        let trigger_sql = format!(
            "SELECT COUNT(*) FROM sys.triggers tr INNER JOIN sys.tables t ON tr.parent_id = t.object_id INNER JOIN sys.schemas s ON t.schema_id = s.schema_id WHERE s.name = {} AND t.name = {} AND tr.is_ms_shipped = 0",
            quote_sql_string(table.target_schema()), quote_sql_string(table.target_table())
        );
        let has_identity = object_count(&mut target_client, &identity_sql).await > 0;
        let trigger_count = object_count(&mut target_client, &trigger_sql).await;
        if trigger_count > 0 {
            table_warnings.push(format!("Target has {trigger_count} trigger(s). Inserts will fire Sage business logic."));
        }
        let missing_parent = missing_parents.contains(&table.source_key());
        warnings.extend(table_warnings.iter().map(|message| format!("{}: {}", table.source_key(), message)));
        if !compatible {
            errors.extend(messages.iter().map(|message| format!("{}: {}", table.source_key(), message)));
        }
        checks.push(OperationTableCheck {
            source_schema: table.schema.clone(), source_table: table.table.clone(),
            target_schema: table.target_schema().to_string(), target_table: table.target_table().to_string(),
            source_rows, target_rows, primary_key, compatible, messages, warnings: table_warnings,
            target_exists, has_identity, trigger_count, missing_parent,
        });
    }
    if spec.batch_size.clamp(1, MAX_BATCH_SIZE) != spec.batch_size {
        warnings.push(format!("Batch size will be limited to {} rows.", MAX_BATCH_SIZE));
    }
    let total_rows = checks.iter().map(|check| check.source_rows).sum::<i64>();
    Ok(OperationPreflight {
        kind: "transfer".to_string(), allowed: errors.is_empty(),
        summary: format!("{} table(s), {} source row(s), {} → {}.", checks.len(), total_rows, source_db, target_db),
        required_confirmation: target_db, warnings, errors, tables: checks, ordered_tables,
        source_edition, target_edition, path_host: String::new(), backup_header: None, suggested_path: String::new(),
    })
}

async fn preflight_backup_inner(state: &AppState, spec: &BackupSpec) -> Result<OperationPreflight, String> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    if spec.connection_id.trim().is_empty() || spec.backup_path.trim().is_empty() {
        errors.push("Select a connection and a server-visible .bak path.".to_string());
    }
    if !spec.backup_path.to_ascii_lowercase().ends_with(".bak") {
        errors.push("Native SQL Server backups must use a .bak file.".to_string());
    }
    if spec.backup_path.contains('\n') || spec.backup_path.contains('\r') {
        errors.push("The backup path is invalid.".to_string());
    }
    if !errors.is_empty() {
        return Ok(empty_preflight("backup", "Backup cannot start.", errors, warnings));
    }
    let config = connection_for_operation(state, &spec.connection_id, &spec.database)?;
    if config.database.trim().is_empty() {
        errors.push("The backup connection must select a database.".to_string());
        return Ok(empty_preflight("backup", "Backup cannot start.", errors, warnings));
    }
    db::test_connection(&config).await.map_err(|error| format!("Backup connection failed: {error}"))?;
    let mut client = db::connect(&config).await?;
    let permission = db::execute_raw_query(
        &mut client,
        "SELECT CASE
            WHEN IS_SRVROLEMEMBER('sysadmin') = 1 THEN 1
            WHEN IS_MEMBER('db_owner') = 1 THEN 1
            WHEN HAS_PERMS_BY_NAME(DB_NAME(), 'DATABASE', 'BACKUP DATABASE') = 1 THEN 1
            ELSE 0
         END AS can_backup",
    ).await?;
    if count_from_query(&permission) != 1 {
        errors.push("The SQL login does not have BACKUP DATABASE permission for this database.".to_string());
    }
    warnings.push(format!(
        "The .bak path is resolved by SQL Server on {} as the service account, not this desktop computer.",
        if config.instance_name.trim().is_empty() { config.host.clone() } else { format!("{}\\{}", config.host, config.instance_name) }
    ));
    if !is_local_sql_host(&config.host) && spec.backup_path.chars().nth(1) == Some(':') {
        warnings.push("This looks like a local drive letter. Remote SQL Server will resolve it on the server, not this PC.".to_string());
    }
    let default_dir = default_backup_directory(&mut client).await;
    let suggested_path = if default_dir.trim().is_empty() {
        String::new()
    } else {
        let stamp = chrono::Utc::now().format("%Y%m%d%H%M").to_string();
        format!("{}\\{}-{}.bak", default_dir.trim_end_matches(['\\', '/']), config.database, stamp)
    };
    if is_user_profile_path(&spec.backup_path) {
        errors.push(friendly_sql_path_error(
            "Cannot open backup device. Operating system error 5(Access is denied.). Native error: 3201",
            &spec.backup_path,
            &suggested_path,
        ));
    } else {
        let parent = parent_sql_path(&spec.backup_path);
        let (_, path_warnings) = list_directory_entries(&mut client, &parent).await.unwrap_or_default();
        if path_warnings.iter().any(|warning| warning.to_ascii_lowercase().contains("access") || warning.to_ascii_lowercase().contains("permission") || warning.to_ascii_lowercase().contains("denied")) {
            errors.push(friendly_sql_path_error(&path_warnings.join(" "), &spec.backup_path, &suggested_path));
        }
    }
    Ok(OperationPreflight {
        kind: "backup".to_string(), allowed: errors.is_empty(),
        summary: format!("Native backup of {} to {}.", config.database, spec.backup_path),
        required_confirmation: config.database.clone(), warnings, errors, tables: Vec::new(), ordered_tables: Vec::new(),
        source_edition: String::new(), target_edition: String::new(),
        path_host: config.host.clone(), backup_header: None, suggested_path,
    })
}

async fn master_connection(state: &AppState, connection_id: &str) -> Result<ConnectionConfig, String> {
    let mut config = state.resolve_connection_config(connection_id)?;
    config.database = "master".to_string();
    Ok(config)
}

async fn inspect_backup_inner(state: &AppState, connection_id: &str, backup_path: &str, extra_paths: &[String]) -> Result<BackupInspection, String> {
    let config = master_connection(state, connection_id).await?;
    let mut client = db::connect(&config).await?;
    let all_paths = backup_media_paths(backup_path, extra_paths);
    let header_sql = format!("RESTORE HEADERONLY FROM DISK = {}", quote_sql_string(backup_path));
    let header_result = execute_operation_sql(&mut client, &header_sql, "restore header").await;
    let (header_ok, header, database_name, backup_name, backup_finish_date, backup_size, server_name, mut header_error) = match header_result {
        Ok(data) => {
            let row = data.rows.first();
            let database_name = row.and_then(|row| row_value_by_name(&data, row, "DatabaseName")).unwrap_or_default();
            let backup_name = row.and_then(|row| row_value_by_name(&data, row, "BackupName")).unwrap_or_default();
            let backup_finish_date = row.and_then(|row| row_value_by_name(&data, row, "BackupFinishDate")).unwrap_or_default();
            let backup_size = row.and_then(|row| row_value_by_name(&data, row, "BackupSize")).unwrap_or_default();
            let server_name = row.and_then(|row| row_value_by_name(&data, row, "ServerName"))
                .filter(|value| !value.is_empty())
                .or_else(|| row.and_then(|row| row_value_by_name(&data, row, "MachineName")))
                .unwrap_or_default();
            let header = row.map(|row| json!({
                "databaseName": row_value_by_name(&data, row, "DatabaseName"),
                "backupName": row_value_by_name(&data, row, "BackupName"),
                "backupFinishDate": row_value_by_name(&data, row, "BackupFinishDate"),
                "backupType": row_value_by_name(&data, row, "BackupType"),
                "compressed": row_value_by_name(&data, row, "Compressed"),
                "backupSize": row_value_by_name(&data, row, "BackupSize"),
                "hasBackupChecksums": row_value_by_name(&data, row, "HasBackupChecksums"),
                "isDamaged": row_value_by_name(&data, row, "IsDamaged"),
                "serverName": row_value_by_name(&data, row, "ServerName"),
                "machineName": row_value_by_name(&data, row, "MachineName"),
            }));
            (true, header, database_name, backup_name, backup_finish_date, backup_size, server_name, String::new())
        }
        Err(error) => (false, None, String::new(), String::new(), String::new(), String::new(), String::new(), error),
    };
    let mut family_count = 1u32;
    let mut family_sequence = 1u32;
    let mut media_set_id = String::new();
    let mut provided_families = Vec::new();
    for path in &all_paths {
        let label_sql = format!("RESTORE LABELONLY FROM DISK = {}", quote_sql_string(path));
        if let Ok(data) = execute_operation_sql(&mut client, &label_sql, "restore label").await {
            if let Some(row) = data.rows.first() {
                family_count = row_value_by_name(&data, row, "FamilyCount").and_then(|value| value.parse().ok()).unwrap_or(family_count).max(1);
                let sequence = row_value_by_name(&data, row, "FamilySequenceNumber").and_then(|value| value.parse().ok()).unwrap_or(1);
                let set_id = row_value_by_name(&data, row, "MediaSetId").unwrap_or_default();
                if media_set_id.is_empty() {
                    media_set_id = set_id.clone();
                    family_sequence = sequence;
                } else if !set_id.is_empty() && !set_id.eq_ignore_ascii_case(&media_set_id) {
                    header_error.push_str(&format!(" {path} belongs to a different media set."));
                }
                if !provided_families.contains(&sequence) {
                    provided_families.push(sequence);
                }
            }
        }
    }
    if provided_families.is_empty() {
        provided_families.push(family_sequence);
    }
    provided_families.sort_unstable();
    provided_families.dedup();
    let file_list_sql = format!("RESTORE FILELISTONLY FROM DISK = {}", quote_sql_string(backup_path));
    let file_list_ok = execute_operation_sql(&mut client, &file_list_sql, "restore file list").await.is_ok();
    let verify_sql = format!("RESTORE VERIFYONLY FROM {}", from_disks(&all_paths));
    let (verify_ok, verify_error) = match execute_operation_sql(&mut client, &verify_sql, "restore verify").await {
        Ok(_) => (true, String::new()),
        Err(error) => (false, error),
    };
    if let Some((reported, _)) = parse_media_family_counts(&verify_error) {
        family_count = family_count.max(reported);
    }
    let missing_families = (1..=family_count.max(1)).filter(|sequence| !provided_families.contains(sequence)).collect::<Vec<_>>();
    let classification = classify_backup(header_ok, &backup_name, verify_ok, &header_error, &verify_error, family_count, provided_families.len() as u32).to_string();
    let unrecoverable = matches!(classification.as_str(), "header_destroyed" | "incomplete");
    let salvageable = classification == "checksum_damage";
    let mut messages = Vec::new();
    if !header_error.is_empty() { messages.push(header_error); }
    if !verify_error.is_empty() { messages.push(verify_error); }
    match classification.as_str() {
        "healthy" => messages.insert(0, "Backup verified. A normal restore can proceed.".to_string()),
        "incomplete" => messages.insert(0, "This backup was interrupted while it was being written (*** INCOMPLETE ***). SQL Server cannot salvage it. Use another .bak.".to_string()),
        "header_destroyed" => messages.insert(0, "The backup header is unreadable. Microsoft does not reconstruct a destroyed media family. Find another .bak.".to_string()),
        "checksum_damage" => messages.insert(0, "The header is readable but VERIFYONLY failed. A salvage restore can copy what is still readable into a separate *_repaired database.".to_string()),
        "access_denied" => messages.insert(0, "This file is in a folder SQL Server cannot open. The app will copy it into the SQL backup folder automatically.".to_string()),
        "missing_media_family" => messages.insert(0, format!("This is stripe {family_sequence} of a {family_count}-file striped backup. Provide the missing family file(s) {missing}. CONTINUE_AFTER_ERROR cannot invent them.", missing = missing_families.iter().map(|value| value.to_string()).collect::<Vec<_>>().join(", "))),
        _ => {}
    }
    let suggested_target = unique_repaired_name(&mut client, &database_name).await.unwrap_or_else(|_| repaired_database_name(&database_name));
    Ok(BackupInspection {
        suggested_target,
        classification, salvageable, unrecoverable, header_ok, file_list_ok, verify_ok,
        database_name, backup_name, backup_finish_date, backup_size, family_count, family_sequence,
        media_set_id, server_name, provided_families, missing_families, messages, header,
    })
}

async fn unique_repaired_name(client: &mut db::DbClient, preferred: &str) -> Result<String, String> {
    let mut candidate = repaired_database_name(preferred);
    for index in 2..50 {
        let exists = db::execute_raw_query(client, &format!("SELECT CASE WHEN DB_ID({}) IS NULL THEN 0 ELSE 1 END AS exists_flag", quote_sql_string(&candidate))).await?;
        if count_from_query(&exists) == 0 {
            return Ok(candidate);
        }
        candidate = format!("{}_{}", repaired_database_name(preferred).trim_end_matches(|character: char| character.is_ascii_digit() || character == '_'), index);
        if !salvage_target_allowed(&candidate) {
            candidate = format!("{}_repaired_{}", preferred.trim(), index);
        }
    }
    Err("Could not allocate a unique *_repaired database name.".to_string())
}

async fn preflight_restore_inner(state: &AppState, spec: &RestoreSpec) -> Result<OperationPreflight, String> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    if spec.connection_id.trim().is_empty() || spec.backup_path.trim().is_empty() || spec.target_database.trim().is_empty() {
        errors.push("Select a server, .bak path, and target database.".to_string());
    }
    if !spec.backup_path.to_ascii_lowercase().ends_with(".bak") {
        errors.push("Native SQL Server restores require a .bak file.".to_string());
    }
    if spec.backup_path.contains('\n') || spec.backup_path.contains('\r') || spec.target_database.contains('\n') || spec.target_database.contains('\r') {
        errors.push("The restore path or target database name is invalid.".to_string());
    }
    if spec.salvage && !salvage_target_allowed(&spec.target_database) {
        errors.push("Salvage restore must target a database whose name contains '_repaired' so production is never overwritten.".to_string());
    }
    if spec.salvage && spec.repair_level == "allow_data_loss" && spec.allow_data_loss_confirmation.trim() != "ALLOW-DATA-LOSS" {
        errors.push("Type ALLOW-DATA-LOSS to permit DBCC CHECKDB REPAIR_ALLOW_DATA_LOSS. Microsoft warns this can delete pages.".to_string());
    }
    if !errors.is_empty() {
        return Ok(empty_preflight("restore", "Restore cannot start.", errors, warnings));
    }
    let config = master_connection(state, &spec.connection_id).await?;
    db::test_connection(&config).await.map_err(|error| format!("Restore connection failed: {error}"))?;
    let inspection = inspect_backup_inner(state, &spec.connection_id, &spec.backup_path, &spec.extra_backup_paths).await?;
    match inspection.classification.as_str() {
        "header_destroyed" | "incomplete" => {
            errors.extend(inspection.messages.clone());
        }
        "missing_media_family" => {
            let missing = inspection.missing_families.iter().map(|value| value.to_string()).collect::<Vec<_>>().join(", ");
            errors.push(format!(
                "This backup is stripe {} of {}. Attach the missing family file(s) {} in the rescue chamber, then inspect again. Salvage cannot invent a missing media family.",
                inspection.family_sequence.max(1),
                inspection.family_count.max(1),
                if missing.is_empty() { "still missing".to_string() } else { missing },
            ));
            warnings.extend(inspection.messages.clone());
        }
        "access_denied" => {
            errors.push("This backup is still in a folder SQL Server cannot open. Pick the file again — the app copies it into the SQL backup folder for you.".to_string());
            warnings.extend(inspection.messages.clone());
        }
        _ if !inspection.verify_ok && !spec.salvage => {
            errors.push(format!("Backup verification failed. Use salvage restore into {} if you must recover what is still readable.", inspection.suggested_target));
            warnings.extend(inspection.messages.clone());
        }
        _ => warnings.extend(inspection.messages.clone()),
    }
    if spec.salvage && !inspection.salvageable && !inspection.verify_ok {
        if !inspection.unrecoverable {
            warnings.push("VERIFYONLY passed; a normal restore is safer than salvage.".to_string());
        }
    }
    let mut client = db::connect(&config).await?;
    let target_exists = db::execute_raw_query(&mut client, &format!("SELECT CASE WHEN DB_ID({}) IS NULL THEN 0 ELSE 1 END AS exists_flag", quote_sql_string(spec.target_database.trim()))).await?;
    if count_from_query(&target_exists) == 1 && !spec.replace_existing {
        errors.push("The target database already exists. Enable replace only after confirming an approved recovery procedure.".to_string());
    }
    if spec.replace_existing && spec.salvage && !salvage_target_allowed(&spec.target_database) {
        errors.push("Salvage cannot replace a database unless its name contains '_repaired'.".to_string());
    }
    if spec.replace_existing && !spec.salvage {
        warnings.push("Replacing a database disconnects active users and overwrites its current data.".to_string());
    }
    if spec.salvage {
        warnings.push("Salvage restore uses CONTINUE_AFTER_ERROR into a side-by-side database. Do not treat it as production until accountants review CHECKDB.".to_string());
    }
    Ok(OperationPreflight {
        kind: if spec.salvage { "restore_salvage".to_string() } else { "restore".to_string() },
        allowed: errors.is_empty(),
        summary: format!("Restore {} to {} ({}).", spec.backup_path, spec.target_database.trim(), inspection.classification),
        required_confirmation: spec.target_database.trim().to_string(), warnings, errors, tables: Vec::new(), ordered_tables: Vec::new(),
        source_edition: String::new(), target_edition: String::new(),
        path_host: config.host.clone(), backup_header: inspection.header.clone(), suggested_path: String::new(),
    })
}

fn emit_job(app: &AppHandle, job: &OperationJob) {
    let _ = app.emit_all("operation_job_updated", job.clone());
}

fn persist_job(state: &AppState, job: OperationJob) -> Result<OperationJob, String> {
    {
        let mut config = state.config.lock().map_err(|error| error.to_string())?;
        let jobs = &mut config.operation_jobs;
        if let Some(position) = jobs.iter().position(|existing| existing.id == job.id) {
            jobs[position] = job.clone();
        } else {
            jobs.push(job.clone());
        }
        jobs.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        if jobs.len() > MAX_OPERATION_JOBS {
            jobs.truncate(MAX_OPERATION_JOBS);
        }
    }
    state.save_config()?;
    Ok(job)
}

fn update_job<F>(app: &AppHandle, id: &str, update: F) -> Result<OperationJob, String>
where
    F: FnOnce(&mut OperationJob),
{
    let state = app.state::<AppState>();
    let job = {
        let config = state.config.lock().map_err(|error| error.to_string())?;
        config.operation_jobs.iter().find(|job| job.id == id).cloned().ok_or_else(|| format!("Operation job '{}' was not found", id))?
    };
    let mut job = job;
    update(&mut job);
    let job = persist_job(&state, job)?;
    emit_job(app, &job);
    Ok(job)
}

fn append_log(app: &AppHandle, id: &str, level: &str, message: impl Into<String>) -> Result<(), String> {
    let message = message.into();
    update_job(app, id, |job| {
        job.logs.push(OperationLogEntry { at: now(), level: level.to_string(), message });
        if job.logs.len() > MAX_OPERATION_LOGS {
            let excess = job.logs.len() - MAX_OPERATION_LOGS;
            job.logs.drain(..excess);
        }
    })?;
    Ok(())
}

fn is_cancelled(app: &AppHandle, id: &str) -> bool {
    let state = app.state::<AppState>();
    state.config.lock().ok().and_then(|config| config.operation_jobs.iter().find(|job| job.id == id).map(|job| job.cancel_requested)).unwrap_or(false)
}

fn cancel_if_requested(app: &AppHandle, id: &str) -> Result<bool, String> {
    if is_cancelled(app, id) {
        update_job(app, id, |job| {
            job.status = "cancelled".to_string();
            job.phase = "cancelled".to_string();
            job.finished_at = Some(now());
            job.cancellable = false;
        })?;
        return Ok(true);
    }
    Ok(false)
}

fn start_job(state: &AppState, kind: &str, label: String, spec: Value, table_progress: Vec<OperationTableProgress>) -> Result<OperationJob, String> {
    let total_rows = table_progress.iter().map(|table| table.expected_rows).sum();
    persist_job(state, OperationJob {
        id: uuid::Uuid::new_v4().to_string(), kind: kind.to_string(), label, spec,
        status: "queued".to_string(), phase: "queued".to_string(), processed_rows: 0, total_rows,
        bytes_written: 0, percent: 0, output_paths: Vec::new(), table_progress, logs: vec![OperationLogEntry { at: now(), level: "info".to_string(), message: "Operation queued.".to_string() }],
        created_at: now(), started_at: None, finished_at: None, error: None, cancellable: true, cancel_requested: false,
    })
}

fn find_operation_job(state: &AppState, job_id: &str) -> Result<OperationJob, String> {
    state
        .config
        .lock()
        .map_err(|error| error.to_string())?
        .operation_jobs
        .iter()
        .find(|job| job.id == job_id)
        .cloned()
        .ok_or_else(|| "Operation job not found".to_string())
}

async fn run_transfer(app: AppHandle, job_id: String, spec: TransferSpec, resume: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let job_before = find_operation_job(&state, &job_id)?;
    let report = preflight_transfer_inner(&state, &spec, resume.then_some(job_before.table_progress.as_slice())).await?;
    if !report.allowed {
        return Err(report.errors.join(" "));
    }
    let _permit = state.operation_slots.clone().acquire_owned().await.map_err(|error| error.to_string())?;
    if cancel_if_requested(&app, &job_id)? { return Ok(()); }
    update_job(&app, &job_id, |job| {
        job.status = "running".to_string(); job.phase = "preparing transfer".to_string(); job.started_at.get_or_insert_with(now);
    })?;
    append_log(&app, &job_id, "info", "Transfer preflight passed.")?;
    let mut source_config = connection_for_operation(&state, &spec.source_connection_id, &spec.source_database)?;
    let mut target_config = connection_for_operation(&state, &spec.target_connection_id, &spec.target_database)?;
    source_config.database = resolved_database(&source_config, &spec.source_database);
    target_config.database = resolved_database(&target_config, &spec.target_database);
    let mut source_client = db::connect(&source_config).await?;
    let mut target_client = db::connect(&target_config).await?;
    let batch_size = spec.batch_size.clamp(1, MAX_BATCH_SIZE);

    for table in report.ordered_tables {
        if cancel_if_requested(&app, &job_id)? { return Ok(()); }
        let source_columns = db::get_columns(&mut source_client, &table.schema, &table.table).await?;
        let primary_key = source_columns.iter().filter(|column| column.is_primary_key).map(|column| quote_identifier(&column.name)).collect::<Vec<_>>();
        let source_key = table.source_key();
        let mut transferred = find_operation_job(&state, &job_id)?.table_progress.iter().find(|progress| format!("{}.{}", progress.source_schema, progress.source_table) == source_key).map(|progress| progress.transferred_rows).unwrap_or(0);
        let expected = table_count(&mut source_client, &table.schema, &table.table).await?;
        update_job(&app, &job_id, |job| job.phase = format!("copying {}", source_key))?;
        append_log(&app, &job_id, "info", format!("Copying {} rows from {}.", expected, source_key))?;
        let columns_sql = source_columns.iter().map(|column| quote_identifier(&column.name)).collect::<Vec<_>>().join(", ");
        let full_source = quote_table(&table.schema, &table.table);
        let full_target = quote_table(table.target_schema(), table.target_table());
        let identity_query = format!(
            "SELECT COUNT(*) AS identity_columns FROM sys.identity_columns ic INNER JOIN sys.tables t ON ic.object_id = t.object_id INNER JOIN sys.schemas s ON t.schema_id = s.schema_id WHERE s.name = {} AND t.name = {}",
            quote_sql_string(table.target_schema()), quote_sql_string(table.target_table())
        );
        let target_has_identity = count_from_query(&db::execute_raw_query(&mut target_client, &identity_query).await?) > 0;
        while transferred < expected {
            if cancel_if_requested(&app, &job_id)? { return Ok(()); }
            let select = format!(
                "SELECT {} FROM {} ORDER BY {} OFFSET {} ROWS FETCH NEXT {} ROWS ONLY",
                columns_sql, full_source, primary_key.join(", "), transferred, batch_size
            );
            let batch = db::execute_raw_query(&mut source_client, &select).await?;
            if batch.rows.is_empty() {
                return Err(format!("Source table {} changed while transferring; checkpoint {} cannot be completed safely.", source_key, transferred));
            }
            let values = batch.rows.iter().map(|row| {
                let values = row.iter().zip(source_columns.iter()).map(|(value, column)| sql_value(value, column)).collect::<Vec<_>>().join(", ");
                format!("({})", values)
            }).collect::<Vec<_>>().join(",\n");
            let identity_on = if target_has_identity { format!("SET IDENTITY_INSERT {} ON;", full_target) } else { String::new() };
            let identity_off = if target_has_identity { format!("SET IDENTITY_INSERT {} OFF;", full_target) } else { String::new() };
            let insert = format!(
                "SET XACT_ABORT ON; BEGIN TRY BEGIN TRANSACTION; {} INSERT INTO {} ({}) VALUES {}; {} COMMIT TRANSACTION; END TRY BEGIN CATCH IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION; THROW; END CATCH",
                identity_on, full_target, columns_sql, values, identity_off
            );
            execute_operation_sql(&mut target_client, &insert, "transfer insert").await?;
            transferred += batch.rows.len() as i64;
            update_job(&app, &job_id, |job| {
                if let Some(progress) = job.table_progress.iter_mut().find(|progress| format!("{}.{}", progress.source_schema, progress.source_table) == source_key) {
                    progress.transferred_rows = transferred;
                }
                job.processed_rows = job.table_progress.iter().map(|progress| progress.transferred_rows).sum();
                job.percent = if job.total_rows == 0 { 99 } else { ((job.processed_rows * 100) / job.total_rows).clamp(0, 99) as u8 };
            })?;
        }
        let target_rows = table_count(&mut target_client, table.target_schema(), table.target_table()).await?;
        if target_rows != expected {
            return Err(format!("Validation failed for {}: source has {} rows but target has {} rows.", source_key, expected, target_rows));
        }
        update_job(&app, &job_id, |job| {
            if let Some(progress) = job.table_progress.iter_mut().find(|progress| format!("{}.{}", progress.source_schema, progress.source_table) == source_key) {
                progress.validated = true;
            }
        })?;
        append_log(&app, &job_id, "info", format!("Validated {} rows in {}.", expected, source_key))?;
    }
    update_job(&app, &job_id, |job| {
        job.status = "completed".to_string(); job.phase = "completed".to_string(); job.percent = 100; job.finished_at = Some(now()); job.cancellable = false;
    })?;
    Ok(())
}

async fn run_backup(app: AppHandle, job_id: String, spec: BackupSpec) -> Result<(), String> {
    let state = app.state::<AppState>();
    let report = preflight_backup_inner(&state, &spec).await?;
    if !report.allowed { return Err(report.errors.join(" ")); }
    let _permit = state.operation_slots.clone().acquire_owned().await.map_err(|error| error.to_string())?;
    if cancel_if_requested(&app, &job_id)? { return Ok(()); }
    update_job(&app, &job_id, |job| { job.status = "running".to_string(); job.phase = "creating native backup".to_string(); job.started_at = Some(now()); })?;
    let mut config = connection_for_operation(&state, &spec.connection_id, &spec.database)?;
    config.database = resolved_database(&config, &spec.database);
    let database = quote_identifier(&config.database);
    let mut client = db::connect(&config).await?;
    let options = if spec.copy_only { "COPY_ONLY, CHECKSUM, INIT" } else { "CHECKSUM, INIT" };
    let backup_with_compression = format!("BACKUP DATABASE {} TO DISK = {} WITH {}, COMPRESSION", database, quote_sql_string(&spec.backup_path), options);
    let backup_without_compression = format!("BACKUP DATABASE {} TO DISK = {} WITH {}", database, quote_sql_string(&spec.backup_path), options);
    match execute_operation_sql(&mut client, &backup_with_compression, "backup database").await {
        Ok(_) => {}
        Err(error) if error.to_ascii_lowercase().contains("compress") => {
            append_log(&app, &job_id, "warning", "This SQL Server edition does not support backup compression; retrying without it.")?;
            execute_operation_sql(&mut client, &backup_without_compression, "backup database").await.map_err(|retry_error| {
                friendly_sql_path_error(&retry_error, &spec.backup_path, &report.suggested_path)
            })?;
        }
        Err(error) => return Err(friendly_sql_path_error(&error, &spec.backup_path, &report.suggested_path)),
    }
    update_job(&app, &job_id, |job| { job.phase = "verifying backup".to_string(); job.percent = 85; })?;
    if cancel_if_requested(&app, &job_id)? { return Ok(()); }
    execute_operation_sql(&mut client, &format!("RESTORE VERIFYONLY FROM DISK = {}", quote_sql_string(&spec.backup_path)), "backup verify").await?;
    update_job(&app, &job_id, |job| {
        job.status = "completed".to_string(); job.phase = "verified".to_string(); job.percent = 100; job.finished_at = Some(now()); job.cancellable = false; job.output_paths = vec![spec.backup_path.clone()];
    })?;
    Ok(())
}

fn row_value_by_name(data: &crate::state::TableData, row: &[Value], name: &str) -> Option<String> {
    data.columns.iter().position(|column| column.name.eq_ignore_ascii_case(name)).and_then(|index| row.get(index)).and_then(|value| value.as_str().map(str::to_string).or_else(|| value.as_i64().map(|value| value.to_string())))
}

async fn run_restore(app: AppHandle, job_id: String, spec: RestoreSpec) -> Result<(), String> {
    let state = app.state::<AppState>();
    let report = preflight_restore_inner(&state, &spec).await?;
    if !report.allowed { return Err(report.errors.join(" ")); }
    let _permit = state.operation_slots.clone().acquire_owned().await.map_err(|error| error.to_string())?;
    if cancel_if_requested(&app, &job_id)? { return Ok(()); }
    update_job(&app, &job_id, |job| { job.status = "running".to_string(); job.phase = "inspecting backup".to_string(); job.started_at = Some(now()); })?;
    let config = master_connection(&state, &spec.connection_id).await?;
    let mut client = db::connect(&config).await?;
    let media = backup_media_paths(&spec.backup_path, &spec.extra_backup_paths);
    let file_list = execute_operation_sql(&mut client, &format!("RESTORE FILELISTONLY FROM DISK = {}", quote_sql_string(&spec.backup_path)), "restore file list").await?;
    if file_list.rows.is_empty() { return Err("Backup did not contain any logical files.".to_string()); }
    let default_paths = db::execute_raw_query(&mut client, "SELECT CONVERT(nvarchar(260), SERVERPROPERTY('InstanceDefaultDataPath')) AS data_path, CONVERT(nvarchar(260), SERVERPROPERTY('InstanceDefaultLogPath')) AS log_path").await?;
    let default_row = default_paths.rows.first().ok_or_else(|| "SQL Server did not return its default data paths.".to_string())?;
    let data_path = row_value_by_name(&default_paths, default_row, "data_path").filter(|value| !value.trim().is_empty()).ok_or_else(|| "SQL Server did not report an InstanceDefaultDataPath; restore requires a server default data path.".to_string())?;
    let log_path = row_value_by_name(&default_paths, default_row, "log_path").filter(|value| !value.trim().is_empty()).unwrap_or_else(|| data_path.clone());
    let target = spec.target_database.trim();
    let mut moves = Vec::new();
    let mut data_number = 0usize;
    let mut log_number = 0usize;
    for row in &file_list.rows {
        let logical = row_value_by_name(&file_list, row, "LogicalName").ok_or_else(|| "Backup file list is missing LogicalName.".to_string())?;
        let file_type = row_value_by_name(&file_list, row, "Type").unwrap_or_default();
        let (base_path, extension) = if file_type.eq_ignore_ascii_case("L") {
            log_number += 1;
            (&log_path, if log_number == 1 { "ldf" } else { "ndf" })
        } else {
            data_number += 1;
            (&data_path, if data_number == 1 { "mdf" } else { "ndf" })
        };
        let separator = if base_path.ends_with('\\') || base_path.ends_with('/') { "" } else { "\\" };
        let suffix = if data_number + log_number == 1 { String::new() } else { format!("_{}", data_number + log_number) };
        let physical = format!("{}{}{}{}.{}", base_path, separator, target, suffix, extension);
        moves.push(format!("MOVE {} TO {}", quote_sql_string(&logical), quote_sql_string(&physical)));
    }
    update_job(&app, &job_id, |job| { job.phase = "restoring database".to_string(); job.percent = 30; })?;
    let existing_preamble = if spec.replace_existing {
        format!("IF DB_ID({}) IS NOT NULL ALTER DATABASE {} SET SINGLE_USER WITH ROLLBACK IMMEDIATE;", quote_sql_string(target), quote_identifier(target))
    } else { String::new() };
    let replace = if spec.replace_existing { ", REPLACE" } else { "" };
    let continue_after_error = if spec.salvage { ", CONTINUE_AFTER_ERROR, CHECKSUM" } else { "" };
    let restore = format!(
        "{} RESTORE DATABASE {} FROM {} WITH {}, RECOVERY{}{}; ALTER DATABASE {} SET MULTI_USER;",
        existing_preamble, quote_identifier(target), from_disks(&media), moves.join(", "), replace, continue_after_error, quote_identifier(target)
    );
    let restore_result = execute_operation_sql(&mut client, &restore, "restore database").await;
    if let Err(error) = restore_result {
        if spec.salvage {
            append_log(&app, &job_id, "warning", format!("Salvage restore continued after: {error}"))?;
        } else {
            return Err(error);
        }
    }
    update_job(&app, &job_id, |job| { job.phase = "verifying restored database".to_string(); job.percent = 90; })?;
    let verify = db::execute_raw_query(&mut client, &format!("SELECT CASE WHEN DB_ID({}) IS NULL THEN 0 ELSE 1 END AS restored", quote_sql_string(target))).await?;
    if count_from_query(&verify) != 1 { return Err("SQL Server did not report the restored database as available.".to_string()); }
    if spec.salvage {
        update_job(&app, &job_id, |job| { job.phase = "running DBCC CHECKDB".to_string(); job.percent = 93; })?;
        let check = format!(
            "ALTER DATABASE {} SET SINGLE_USER WITH ROLLBACK IMMEDIATE; BEGIN TRY DBCC CHECKDB ({}) WITH ALL_ERRORMSGS, NO_INFOMSGS, TABLOCK; END TRY BEGIN CATCH PRINT ERROR_MESSAGE(); END CATCH; ALTER DATABASE {} SET MULTI_USER;",
            quote_identifier(target), quote_sql_string(target), quote_identifier(target)
        );
        match execute_operation_sql(&mut client, &check, "checkdb").await {
            Ok(_) => append_log(&app, &job_id, "info", "DBCC CHECKDB completed.")?,
            Err(error) => append_log(&app, &job_id, "warning", format!("DBCC CHECKDB reported: {error}"))?,
        }
        if spec.repair_level == "rebuild" || spec.repair_level == "allow_data_loss" {
            let repair = if spec.repair_level == "allow_data_loss" { "REPAIR_ALLOW_DATA_LOSS" } else { "REPAIR_REBUILD" };
            update_job(&app, &job_id, |job| { job.phase = format!("repairing with {repair}"); job.percent = 96; })?;
            let repair_sql = format!(
                "ALTER DATABASE {} SET SINGLE_USER WITH ROLLBACK IMMEDIATE; BEGIN TRY DBCC CHECKDB ({}, {}) WITH ALL_ERRORMSGS, TABLOCK; END TRY BEGIN CATCH PRINT ERROR_MESSAGE(); END CATCH; ALTER DATABASE {} SET MULTI_USER;",
                quote_identifier(target), quote_sql_string(target), repair, quote_identifier(target)
            );
            if let Err(error) = execute_operation_sql(&mut client, &repair_sql, "checkdb repair").await {
                append_log(&app, &job_id, "warning", format!("{repair} reported: {error}"))?;
            }
        }
        let marker = json!({
            "sourceBak": spec.backup_path,
            "salvagedAt": now(),
            "continueAfterError": true,
            "repairLevel": spec.repair_level,
        }).to_string();
        let property = format!(
            "USE {}; EXEC sys.sp_addextendedproperty @name = N'SageDataBridge_Salvage', @value = {};",
            quote_identifier(target), quote_sql_string(&marker)
        );
        let _ = execute_operation_sql(&mut client, &property, "salvage marker").await;
        append_log(&app, &job_id, "warning", format!("Salvage finished as {}. Do not use it as production until accountants review CHECKDB.", target))?;
    }
    update_job(&app, &job_id, |job| { job.status = "completed".to_string(); job.phase = if spec.salvage { "salvaged".to_string() } else { "completed".to_string() }; job.percent = 100; job.finished_at = Some(now()); job.cancellable = false; job.output_paths = vec![spec.backup_path.clone()]; })?;
    Ok(())
}

fn spawn_operation<F>(app: AppHandle, job_id: String, future: F)
where
    F: std::future::Future<Output = Result<(), String>> + Send + 'static,
{
    tauri::async_runtime::spawn(async move {
        if let Err(error) = future.await {
            let _ = append_log(&app, &job_id, "error", error.clone());
            let _ = update_job(&app, &job_id, |job| {
                job.status = if job.cancel_requested { "cancelled".to_string() } else { "failed".to_string() };
                job.phase = job.status.clone(); job.error = Some(error); job.finished_at = Some(now()); job.cancellable = false;
            });
        }
    });
}

async fn query_scalar_string(client: &mut db::DbClient, sql: &str, column: &str) -> Option<String> {
    let data = db::execute_raw_query(client, sql).await.ok()?;
    let row = data.rows.first()?;
    row_value_by_name(&data, row, column).filter(|value| !value.trim().is_empty())
}

async fn default_backup_directory(client: &mut db::DbClient) -> String {
    if let Some(path) = query_scalar_string(client, "SELECT CONVERT(nvarchar(4000), SERVERPROPERTY('InstanceDefaultBackupPath')) AS backup_path", "backup_path").await {
        return path;
    }
    let registry = r#"
DECLARE @dir nvarchar(4000);
BEGIN TRY
    EXEC master.dbo.xp_instance_regread N'HKEY_LOCAL_MACHINE', N'Software\Microsoft\MSSQLServer\MSSQLServer', N'BackupDirectory', @dir OUTPUT;
END TRY
BEGIN CATCH
    SET @dir = NULL;
END CATCH
SELECT @dir AS backup_path;
"#;
    query_scalar_string(client, registry, "backup_path").await.unwrap_or_default()
}

fn parse_path_listing(data: &crate::state::TableData, directory: &str) -> Vec<SqlServerPathEntry> {
    data.rows.iter().filter_map(|row| {
        let name = row_value_by_name(data, row, "file_or_directory_name")
            .or_else(|| row_value_by_name(data, row, "subdirectory"))
            .or_else(|| row_value_by_name(data, row, "name"))
            .or_else(|| row.first().and_then(|value| value.as_str().map(str::to_string)))?;
        if name.trim().is_empty() {
            return None;
        }
        let is_directory = row_value_by_name(data, row, "is_directory")
            .and_then(|value| value.parse::<i64>().ok())
            .or_else(|| row_value_by_name(data, row, "file").and_then(|value| value.parse::<i64>().ok()).map(|file| if file == 0 { 1 } else { 0 }))
            .unwrap_or(1);
        let size = row_value_by_name(data, row, "size_in_bytes").and_then(|value| value.parse().ok());
        let path = if directory.len() == 2 && directory.ends_with(':') {
            format!("{directory}\\{name}")
        } else {
            join_sql_path(directory, &name)
        };
        Some(SqlServerPathEntry {
            is_backup: name.to_ascii_lowercase().ends_with(".bak"),
            kind: if is_directory == 1 { "folder".to_string() } else { "file".to_string() },
            size_bytes: size,
            path,
            name,
        })
    }).collect()
}

async fn list_directory_entries(client: &mut db::DbClient, directory: &str) -> Result<(Vec<SqlServerPathEntry>, Vec<String>), String> {
    let mut warnings = Vec::new();
    if directory.trim().is_empty() {
        let drives = db::execute_raw_query(client, "EXEC master.dbo.xp_fixeddrives").await;
        let mut entries = Vec::new();
        if let Ok(data) = drives {
            for row in &data.rows {
                if let Some(drive) = row_value_by_name(&data, row, "drive").or_else(|| row.first().and_then(|value| value.as_str().map(str::to_string))) {
                    let letter = drive.trim().trim_end_matches(':');
                    entries.push(SqlServerPathEntry {
                        name: format!("{letter}:"),
                        path: format!("{letter}:\\"),
                        kind: "drive".to_string(),
                        is_backup: false,
                        size_bytes: None,
                    });
                }
            }
        } else if let Err(error) = drives {
            warnings.push(format!("SQL Server did not list drives: {error}"));
        }
        return Ok((entries, warnings));
    }
    let enumerate = format!(
        "SELECT file_or_directory_name, CAST(is_directory AS int) AS is_directory, size_in_bytes FROM sys.dm_os_enumerate_filesystem({}, N'*')",
        quote_sql_string(directory)
    );
    match db::execute_raw_query(client, &enumerate).await {
        Ok(data) => return Ok((parse_path_listing(&data, directory), warnings)),
        Err(error) => warnings.push(format!("sys.dm_os_enumerate_filesystem is unavailable: {error}")),
    }
    let dirtree = format!("EXEC master.dbo.xp_dirtree {}, 1, 1", quote_sql_string(directory));
    match db::execute_raw_query(client, &dirtree).await {
        Ok(data) => Ok((parse_path_listing(&data, directory), warnings)),
        Err(error) => {
            warnings.push(format!("xp_dirtree is unavailable: {error}"));
            Ok((Vec::new(), warnings))
        }
    }
}

#[tauri::command]
pub async fn preflight_transfer(state: State<'_, AppState>, spec: TransferSpec) -> Result<OperationPreflight, String> {
    preflight_transfer_inner(&state, &spec, None).await
}

#[tauri::command]
pub async fn preflight_backup(state: State<'_, AppState>, spec: BackupSpec) -> Result<OperationPreflight, String> {
    preflight_backup_inner(&state, &spec).await
}

#[tauri::command]
pub async fn preflight_restore(state: State<'_, AppState>, spec: RestoreSpec) -> Result<OperationPreflight, String> {
    preflight_restore_inner(&state, &spec).await
}

#[tauri::command]
pub async fn start_transfer(window: Window, state: State<'_, AppState>, spec: TransferSpec) -> Result<OperationJob, String> {
    let report = preflight_transfer_inner(&state, &spec, None).await?;
    if !report.allowed { return Err(report.errors.join(" ")); }
    if spec.confirmation_text.trim() != report.required_confirmation { return Err(format!("Type '{}' exactly to approve this target write.", report.required_confirmation)); }
    let progress = report.tables.iter().map(|table| OperationTableProgress { source_schema: table.source_schema.clone(), source_table: table.source_table.clone(), target_schema: table.target_schema.clone(), target_table: table.target_table.clone(), expected_rows: table.source_rows, transferred_rows: 0, validated: false }).collect();
    let job = start_job(&state, "transfer", format!("Transfer to {}", report.required_confirmation), serde_json::to_value(&spec).map_err(|error| error.to_string())?, progress)?;
    emit_job(&window.app_handle(), &job);
    let app = window.app_handle(); let job_id = job.id.clone();
    spawn_operation(app.clone(), job_id.clone(), run_transfer(app, job_id, spec, false));
    Ok(job)
}

#[tauri::command]
pub async fn start_backup(window: Window, state: State<'_, AppState>, spec: BackupSpec) -> Result<OperationJob, String> {
    let report = preflight_backup_inner(&state, &spec).await?;
    if !report.allowed { return Err(report.errors.join(" ")); }
    if spec.confirmation_text.trim() != report.required_confirmation { return Err(format!("Type '{}' exactly to approve this backup.", report.required_confirmation)); }
    let job = start_job(&state, "backup", format!("Backup {}", report.required_confirmation), serde_json::to_value(&spec).map_err(|error| error.to_string())?, Vec::new())?;
    emit_job(&window.app_handle(), &job);
    let app = window.app_handle(); let job_id = job.id.clone();
    spawn_operation(app.clone(), job_id.clone(), run_backup(app, job_id, spec));
    Ok(job)
}

#[tauri::command]
pub async fn start_restore(window: Window, state: State<'_, AppState>, spec: RestoreSpec) -> Result<OperationJob, String> {
    let report = preflight_restore_inner(&state, &spec).await?;
    if !report.allowed { return Err(report.errors.join(" ")); }
    if spec.confirmation_text.trim() != report.required_confirmation { return Err(format!("Type '{}' exactly to approve this restore.", report.required_confirmation)); }
    let job_kind = if spec.salvage { "restore_salvage" } else { "restore" };
    let job = start_job(&state, job_kind, format!("Restore {}", report.required_confirmation), serde_json::to_value(&spec).map_err(|error| error.to_string())?, Vec::new())?;
    emit_job(&window.app_handle(), &job);
    let app = window.app_handle(); let job_id = job.id.clone();
    spawn_operation(app.clone(), job_id.clone(), run_restore(app, job_id, spec));
    Ok(job)
}

#[tauri::command]
pub fn list_operation_jobs(state: State<'_, AppState>) -> Result<Vec<OperationJob>, String> {
    let mut jobs = state.config.lock().map_err(|error| error.to_string())?.operation_jobs.clone();
    jobs.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(jobs)
}

#[tauri::command]
pub fn get_operation_job(state: State<'_, AppState>, job_id: String) -> Result<OperationJob, String> {
    find_operation_job(&state, &job_id)
}

#[tauri::command]
pub fn cancel_operation_job(window: Window, state: State<'_, AppState>, job_id: String) -> Result<(), String> {
    let app = window.app_handle();
    let job = update_job(&app, &job_id, |job| {
        if matches!(job.status.as_str(), "queued" | "running") {
            job.cancel_requested = true; job.phase = "cancelling".to_string();
        }
    })?;
    let _ = state;
    emit_job(&app, &job);
    Ok(())
}

#[tauri::command]
pub async fn resume_operation_job(window: Window, state: State<'_, AppState>, job_id: String) -> Result<OperationJob, String> {
    let job = find_operation_job(&state, &job_id)?;
    if job.kind != "transfer" { return Err("Only interrupted transfer jobs can be resumed.".to_string()); }
    if !matches!(job.status.as_str(), "failed" | "cancelled" | "interrupted") { return Err("This operation is not ready to resume.".to_string()); }
    let spec: TransferSpec = serde_json::from_value(job.spec.clone()).map_err(|_| "Saved transfer specification is invalid.".to_string())?;
    let report = preflight_transfer_inner(&state, &spec, Some(&job.table_progress)).await?;
    if !report.allowed { return Err(report.errors.join(" ")); }
    let app = window.app_handle();
    let resumed = update_job(&app, &job_id, |job| { job.status = "queued".to_string(); job.phase = "queued for resume".to_string(); job.error = None; job.finished_at = None; job.cancel_requested = false; job.cancellable = true; })?;
    let app_for_task = app.clone();
    spawn_operation(app_for_task.clone(), job_id.clone(), run_transfer(app_for_task, job_id, spec, true));
    Ok(resumed)
}

#[tauri::command]
pub async fn operations_list_databases(state: State<'_, AppState>, connection_id: String) -> Result<Vec<String>, String> {
    let mut config = state.resolve_connection_config(&connection_id)?;
    if config.database.trim().is_empty() {
        config.database = "master".to_string();
    }
    let mut client = db::connect(&config).await?;
    db::get_databases(&mut client).await
}

#[tauri::command]
pub async fn operations_list_tables(
    state: State<'_, AppState>,
    connection_id: String,
    database: String,
) -> Result<Vec<OperationTableInfo>, String> {
    let config = connection_for_operation(&state, &connection_id, &database)?;
    if config.database.trim().is_empty() {
        return Err("Select a database first.".to_string());
    }
    let mut client = db::connect(&config).await?;
    let sql = r#"
        SELECT
            s.name AS schema_name,
            t.name AS table_name,
            CAST(SUM(p.rows) AS BIGINT) AS row_count,
            CAST(MAX(CASE WHEN pk.object_id IS NULL THEN 0 ELSE 1 END) AS int) AS has_primary_key
        FROM sys.tables t
        INNER JOIN sys.schemas s ON t.schema_id = s.schema_id
        INNER JOIN sys.indexes i ON t.object_id = i.object_id AND i.index_id IN (0, 1)
        INNER JOIN sys.partitions p ON i.object_id = p.object_id AND i.index_id = p.index_id
        LEFT JOIN sys.indexes pk ON t.object_id = pk.object_id AND pk.is_primary_key = 1
        WHERE t.type = 'U' AND t.is_ms_shipped = 0
        GROUP BY s.name, t.name
        ORDER BY s.name, t.name
    "#;
    let data = db::execute_raw_query(&mut client, sql).await?;
    Ok(data.rows.iter().filter_map(|row| {
        let schema = row_value_by_name(&data, row, "schema_name")?;
        let name = row_value_by_name(&data, row, "table_name")?;
        let row_count = row_value_by_name(&data, row, "row_count").and_then(|value| value.parse().ok());
        let has_primary_key = row_value_by_name(&data, row, "has_primary_key").and_then(|value| value.parse::<i64>().ok()).unwrap_or(0) == 1;
        Some(OperationTableInfo {
            full_name: format!("{schema}.{name}"),
            schema,
            name,
            row_count,
            has_primary_key,
        })
    }).collect())
}

#[tauri::command]
pub async fn preview_transfer_table(
    state: State<'_, AppState>,
    connection_id: String,
    database: String,
    schema: String,
    table: String,
) -> Result<crate::state::TableData, String> {
    let config = connection_for_operation(&state, &connection_id, &database)?;
    let mut client = db::connect(&config).await?;
    let sql = format!("SELECT TOP 20 * FROM {}", quote_table(&schema, &table));
    db::execute_raw_query(&mut client, &sql).await
}

#[tauri::command]
pub async fn list_sql_server_paths(
    state: State<'_, AppState>,
    connection_id: String,
    directory: Option<String>,
) -> Result<SqlServerPathListing, String> {
    let config = master_connection(&state, &connection_id).await?;
    let mut client = db::connect(&config).await?;
    let default_backup_directory = default_backup_directory(&mut client).await;
    let requested = directory.unwrap_or_default().trim().to_string();
    let directory = if requested.is_empty() { default_backup_directory.clone() } else { requested };
    let (mut entries, mut warnings) = list_directory_entries(&mut client, &directory).await?;
    if directory.is_empty() && !default_backup_directory.is_empty() {
        entries.insert(0, SqlServerPathEntry {
            name: default_backup_directory.clone(),
            path: default_backup_directory.clone(),
            kind: "folder".to_string(),
            is_backup: false,
            size_bytes: None,
        });
    }
    if entries.is_empty() {
        warnings.push("SQL Server did not return folders. Type a server path or a UNC share, then verify it.".to_string());
    }
    entries.sort_by(|left, right| left.kind.cmp(&right.kind).then(left.name.to_ascii_lowercase().cmp(&right.name.to_ascii_lowercase())));
    Ok(SqlServerPathListing {
        host: config.host.clone(),
        instance_name: config.instance_name.clone(),
        local_host: is_local_sql_host(&config.host),
        directory,
        default_backup_directory,
        entries,
        warnings,
    })
}

async fn sql_service_account(client: &mut db::DbClient, instance: &str) -> String {
    let sql = "SELECT TOP (1) service_account FROM sys.dm_server_services WHERE servicename LIKE N'SQL Server (%'";
    if let Some(account) = query_scalar_string(client, sql, "service_account").await {
        if !account.trim().is_empty() {
            return account;
        }
    }
    if instance.trim().is_empty() || instance.eq_ignore_ascii_case("MSSQLSERVER") {
        "NT SERVICE\\MSSQLSERVER".to_string()
    } else {
        format!("NT SERVICE\\MSSQL${}", instance.trim())
    }
}

fn grant_sql_read_acl(path: &str, account: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        if account.trim().is_empty() {
            return Ok(());
        }
        let permission = if Path::new(path).is_dir() {
            format!("{account}:(OI)(CI)RX")
        } else {
            format!("{account}:(R)")
        };
        let output = std::process::Command::new("icacls")
            .args([path, "/grant", &permission])
            .output()
            .map_err(|error| format!("Unable to update NTFS permissions: {error}"))?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
    }
    let _ = (path, account);
    Ok(())
}

fn same_sql_path(left: &str, right: &str) -> bool {
    let normalize = |value: &str| value.replace('/', "\\").trim_end_matches('\\').to_ascii_lowercase();
    if normalize(left) == normalize(right) {
        return true;
    }
    match (Path::new(left).canonicalize(), Path::new(right).canonicalize()) {
        (Ok(source), Ok(destination)) => source == destination,
        _ => false,
    }
}

fn unique_stage_destination(directory: &str, file_name: &str, source: &Path) -> String {
    let destination = join_sql_path(directory, file_name);
    let dest_path = Path::new(&destination);
    if !dest_path.exists() {
        return destination;
    }
    if same_sql_path(&source.to_string_lossy(), &destination) {
        return destination;
    }
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("backup");
    let ext = Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("bak");
    for index in 1..100 {
        let candidate = join_sql_path(directory, &format!("{stem}-sdb{index}.{ext}"));
        if !Path::new(&candidate).exists() {
            return candidate;
        }
    }
    destination
}

fn copy_bak_into_directory(source: &Path, directory: &str, account: &str) -> Result<String, String> {
    fs::create_dir_all(directory).map_err(|error| format!("Unable to create the SQL backup folder: {error}"))?;
    let _ = grant_sql_read_acl(directory, account);
    let file_name = source.file_name().and_then(|name| name.to_str()).unwrap_or("staged.bak");
    let destination = unique_stage_destination(directory, file_name, source);
    if same_sql_path(&source.to_string_lossy(), &destination) {
        let _ = grant_sql_read_acl(&destination, account);
        return Ok(destination);
    }
    fs::copy(source, &destination).map_err(|error| format!("Unable to copy the backup file: {error}"))?;
    if let Err(error) = grant_sql_read_acl(&destination, account) {
        // Copy succeeded; SQL may still read the file if the folder grant worked.
        let _ = error;
    }
    Ok(destination)
}

async fn backup_stage_plan_inner(state: &AppState, connection_id: &str, source_path: &str) -> Result<BackupStagePlan, String> {
    let source = Path::new(source_path);
    if !source.is_file() {
        return Err("Choose an existing .bak file on this computer first.".to_string());
    }
    if !source_path.to_ascii_lowercase().ends_with(".bak") {
        return Err("Only .bak files can be staged for SQL Server.".to_string());
    }
    let config = master_connection(state, connection_id).await?;
    let local_host = is_local_sql_host(&config.host);
    let mut warnings = Vec::new();
    if !local_host {
        warnings.push("This SQL Server is on another computer, so the file cannot be copied onto that machine from here. Put the .bak on a shared folder that computer can open, then pick it again.".to_string());
        return Ok(BackupStagePlan {
            source_path: source_path.to_string(),
            destination_path: String::new(),
            bytes: source.metadata().map(|meta| meta.len()).unwrap_or(0),
            sql_account: String::new(),
            local_host,
            needs_stage: false,
            warnings,
        });
    }
    let mut client = db::connect(&config).await?;
    let mut destination_dir = default_backup_directory(&mut client).await;
    if destination_dir.trim().is_empty() {
        destination_dir = FALLBACK_BACKUP_DIRECTORY.to_string();
        warnings.push(format!("SQL Server did not report a default backup folder. The file will be copied to {FALLBACK_BACKUP_DIRECTORY}."));
    }
    let file_name = source.file_name().and_then(|name| name.to_str()).unwrap_or("staged.bak");
    let destination_path = unique_stage_destination(&destination_dir, file_name, source);
    let needs_stage = !same_sql_path(source_path, &destination_path);
    if needs_stage {
        warnings.push("This file is in a folder SQL Server cannot open. It will be copied into the SQL backup folder automatically. Your original file is left in place.".to_string());
    }
    Ok(BackupStagePlan {
        source_path: source_path.to_string(),
        destination_path,
        bytes: source.metadata().map(|meta| meta.len()).unwrap_or(0),
        sql_account: sql_service_account(&mut client, &config.instance_name).await,
        local_host,
        needs_stage,
        warnings,
    })
}

#[tauri::command]
pub async fn inspect_backup(state: State<'_, AppState>, connection_id: String, backup_path: String, extra_paths: Option<Vec<String>>) -> Result<BackupInspection, String> {
    inspect_backup_inner(&state, &connection_id, &backup_path, extra_paths.as_deref().unwrap_or(&[])).await
}

#[tauri::command]
pub async fn preview_backup_stage(state: State<'_, AppState>, connection_id: String, source_path: String) -> Result<BackupStagePlan, String> {
    backup_stage_plan_inner(&state, &connection_id, &source_path).await
}

#[tauri::command]
pub async fn confirm_backup_stage(state: State<'_, AppState>, connection_id: String, source_path: String, confirmed: bool) -> Result<BackupStagePlan, String> {
    if !confirmed {
        return Err("Confirm the copy before the file is moved into a SQL Server folder.".to_string());
    }
    let mut plan = backup_stage_plan_inner(&state, &connection_id, &source_path).await?;
    if !plan.local_host {
        return Ok(plan);
    }
    if !plan.needs_stage && same_sql_path(&plan.source_path, &plan.destination_path) {
        plan.needs_stage = false;
        return Ok(plan);
    }
    let source = Path::new(&plan.source_path);
    let preferred_dir = Path::new(&plan.destination_path)
        .parent()
        .map(|parent| parent.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| FALLBACK_BACKUP_DIRECTORY.to_string());
    let copied = match copy_bak_into_directory(source, &preferred_dir, &plan.sql_account) {
        Ok(destination) => destination,
        Err(error) if !same_sql_path(&preferred_dir, FALLBACK_BACKUP_DIRECTORY) => {
            plan.warnings.push(format!("Could not copy into the SQL backup folder ({error}). Trying {FALLBACK_BACKUP_DIRECTORY} instead."));
            copy_bak_into_directory(source, FALLBACK_BACKUP_DIRECTORY, &plan.sql_account)?
        }
        Err(error) => return Err(error),
    };
    plan.destination_path = copied;
    plan.needs_stage = false;
    plan.warnings.insert(0, format!("Copied to {}. SQL Server can now open this backup. Your original file is unchanged.", plan.destination_path));
    Ok(plan)
}

#[tauri::command]
pub fn sql_host_is_local(state: State<'_, AppState>, connection_id: String) -> Result<bool, String> {
    let config = state.resolve_connection_config(&connection_id)?;
    Ok(is_local_sql_host(&config.host))
}

#[tauri::command]
pub fn list_operation_templates(state: State<'_, AppState>) -> Result<Vec<OperationTemplate>, String> {
    let mut templates = state.config.lock().map_err(|error| error.to_string())?.operation_templates.clone();
    templates.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(templates)
}

#[tauri::command]
pub fn save_operation_template(state: State<'_, AppState>, mut template: OperationTemplate) -> Result<OperationTemplate, String> {
    if !matches!(template.kind.as_str(), "transfer" | "backup" | "restore") { return Err("Unsupported operation template type.".to_string()); }
    if template.label.trim().is_empty() { return Err("A template name is required.".to_string()); }
    if template.id.trim().is_empty() { template.id = uuid::Uuid::new_v4().to_string(); }
    template.updated_at = now();
    {
        let mut config = state.config.lock().map_err(|error| error.to_string())?;
        if let Some(position) = config.operation_templates.iter().position(|existing| existing.id == template.id) { config.operation_templates[position] = template.clone(); } else { config.operation_templates.push(template.clone()); }
    }
    state.save_config()?;
    Ok(template)
}

#[tauri::command]
pub fn delete_operation_template(state: State<'_, AppState>, template_id: String) -> Result<(), String> {
    state.config.lock().map_err(|error| error.to_string())?.operation_templates.retain(|template| template.id != template_id);
    state.save_config()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticBundle {
    generated_at: String,
    app: String,
    job: OperationJob,
    connections: Vec<Value>,
}

#[tauri::command]
pub fn export_operation_diagnostic(state: State<'_, AppState>, job_id: String, file_path: String) -> Result<String, String> {
    let job = find_operation_job(&state, &job_id)?;
    let connections = state.config.lock().map_err(|error| error.to_string())?.connections.iter().map(|connection| json!({
        "id": connection.id, "name": connection.name, "host": connection.host, "instanceName": connection.instance_name,
        "port": connection.port, "database": connection.database, "usesWindowsAuth": connection.use_windows_auth,
        "encrypt": connection.encrypt, "trustCert": connection.trust_cert
    })).collect();
    let bundle = DiagnosticBundle { generated_at: now(), app: format!("Sage Data Bridge {}", env!("CARGO_PKG_VERSION")), job, connections };
    let path = Path::new(&file_path);
    if path.extension().and_then(|value| value.to_str()).map(|value| !value.eq_ignore_ascii_case("json")).unwrap_or(true) { return Err("Diagnostic bundles must be saved as .json files.".to_string()); }
    fs::write(path, serde_json::to_vec_pretty(&bundle).map_err(|error| error.to_string())?).map_err(|error| format!("Unable to write diagnostic bundle: {error}"))?;
    Ok(file_path)
}

pub fn recover_interrupted_jobs(config: &mut crate::state::AppConfig) {
    let timestamp = now();
    for job in &mut config.operation_jobs {
        if matches!(job.status.as_str(), "queued" | "running") {
            job.status = "interrupted".to_string(); job.phase = "interrupted".to_string(); job.finished_at = Some(timestamp.clone()); job.cancellable = false;
            job.logs.push(OperationLogEntry { at: timestamp.clone(), level: "warning".to_string(), message: "The application closed before this operation completed. A transfer may be resumed after its target checkpoint is verified.".to_string() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_backup, friendly_sql_path_error, is_backup_file_path, is_local_sql_host, parse_media_family_counts,
        is_user_profile_path, parent_sql_path, quote_identifier, repaired_database_name,
        same_sql_path, salvage_target_allowed, table_order, transfer_same_database, unique_stage_destination,
        would_lose_precision, would_truncate, TransferSpec, TransferTableSpec,
    };
    use crate::state::{ColumnInfo, RelationshipInfo};

    #[test]
    fn orders_parent_before_child() {
        let parent = TransferTableSpec { schema: "dbo".to_string(), table: "parent".to_string(), target_schema: String::new(), target_table: String::new() };
        let child = TransferTableSpec { schema: "dbo".to_string(), table: "child".to_string(), target_schema: String::new(), target_table: String::new() };
        let relation = RelationshipInfo { constraint_name: "fk".to_string(), source_schema: "dbo".to_string(), source_table: "child".to_string(), source_column: "parent_id".to_string(), target_schema: "dbo".to_string(), target_table: "parent".to_string(), target_column: "id".to_string(), ordinal: 1 };
        let ordered = table_order(&[child, parent], &[relation]).unwrap();
        assert_eq!(ordered[0].table, "parent");
    }

    #[test]
    fn quotes_identifiers() {
        assert_eq!(quote_identifier("a]b"), "[a]]b]");
    }

    fn column(data_type: &str, max_length: Option<i32>, precision: Option<i32>, scale: Option<i32>) -> ColumnInfo {
        ColumnInfo {
            name: "col".to_string(),
            data_type: data_type.to_string(),
            is_nullable: true,
            is_primary_key: false,
            ordinal: 1,
            max_length,
            precision,
            scale,
        }
    }

    #[test]
    fn local_hosts_include_loopback_and_dot() {
        assert!(is_local_sql_host("localhost"));
        assert!(is_local_sql_host("127.0.0.1"));
        assert!(is_local_sql_host(".\\SQLEXPRESS"));
        assert!(is_local_sql_host("(local)"));
        assert!(!is_local_sql_host("sql-prod.example.local"));
    }

    #[test]
    fn backup_paths_must_be_bak_files() {
        assert!(is_backup_file_path(r"D:\SQLBackups\company.bak"));
        assert!(!is_backup_file_path(r"D:\SQLBackups\company.sql"));
        assert!(!is_backup_file_path("D:\\x.bak\nDROP"));
        assert_eq!(parent_sql_path(r"D:\SQLBackups\company.bak"), r"D:\SQLBackups");
        assert!(is_user_profile_path(r"C:\Users\USER\Desktop\cool\BF.bak"));
        assert!(!is_user_profile_path(r"C:\Program Files\Microsoft SQL Server\MSSQL16.MSSQLSERVER\MSSQL\Backup\BF.bak"));
        let message = friendly_sql_path_error(
            "Cannot open backup device. Operating system error 5(Access is denied.). Native error: 3201",
            r"C:\Users\USER\Desktop\cool\BF.bak",
            r"C:\Program Files\Microsoft SQL Server\MSSQL\Backup\BF.bak",
        );
        assert!(message.contains("service account"));
        assert!(message.contains("C:\\Program Files"));
    }

    #[test]
    fn salvage_names_and_classifications() {
        assert_eq!(repaired_database_name("BF"), "BF_repaired");
        assert_eq!(repaired_database_name("BF_repaired"), "BF_repaired");
        assert!(salvage_target_allowed("BF_repaired_2"));
        assert!(!salvage_target_allowed("BF"));
        assert_eq!(classify_backup(false, "", false, "media family incorrectly formed", "", 1, 1), "header_destroyed");
        assert_eq!(classify_backup(true, "*** INCOMPLETE ***", false, "", "", 1, 1), "incomplete");
        assert_eq!(classify_backup(true, "BF full", true, "", "", 1, 1), "healthy");
        assert_eq!(classify_backup(true, "BF full", false, "", "checksum failed", 1, 1), "checksum_damage");
        assert_eq!(classify_backup(false, "", false, "Operating system error 5(Access is denied.)", "", 1, 1), "access_denied");
        assert_eq!(
            classify_backup(true, "OCDE full", false, "", "The media set has 2 media families but only 1 are provided. All members must be provided.", 2, 1),
            "missing_media_family"
        );
        assert_eq!(classify_backup(true, "OCDE full", true, "", "", 2, 2), "healthy");
        assert_eq!(
            parse_media_family_counts("The media set has 2 media families but only 1 are provided. All members must be provided."),
            Some((2, 1))
        );
        assert!(same_sql_path(r"C:\SQL\Backup\BF.bak", r"C:/SQL/Backup/BF.bak"));
        assert!(!same_sql_path(r"C:\SQL\Backup\BF.bak", r"C:\SQL\Backup\NTIT.bak"));
        let temp = std::env::temp_dir().join(format!("sdb-stage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp).unwrap();
        let source = temp.join("NTIT.bak");
        std::fs::write(&source, b"one").unwrap();
        let existing = temp.join("NTIT.bak");
        assert_eq!(
            unique_stage_destination(&temp.to_string_lossy(), "NTIT.bak", &source),
            existing.to_string_lossy()
        );
        let other = temp.join("other.bak");
        std::fs::write(&other, b"two").unwrap();
        let uniqued = unique_stage_destination(&temp.to_string_lossy(), "NTIT.bak", &other);
        assert!(uniqued.to_ascii_lowercase().ends_with("ntit-sdb1.bak"));
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn truncation_and_precision_loss_are_blocked() {
        assert!(would_truncate(&column("nvarchar", Some(100), None, None), &column("nvarchar", Some(40), None, None)));
        assert!(!would_truncate(&column("nvarchar", Some(40), None, None), &column("nvarchar", Some(100), None, None)));
        assert!(would_lose_precision(&column("decimal", None, Some(18), Some(4)), &column("decimal", None, Some(18), Some(2))));
        assert!(!would_lose_precision(&column("decimal", None, Some(18), Some(2)), &column("decimal", None, Some(18), Some(4))));
    }

    #[test]
    fn same_connection_can_target_another_database() {
        let spec = TransferSpec {
            source_connection_id: "a".to_string(),
            target_connection_id: "a".to_string(),
            source_database: "src".to_string(),
            target_database: "dst".to_string(),
            tables: Vec::new(),
            batch_size: 500,
            conflict_strategy: "fail".to_string(),
            confirmation_text: String::new(),
            edition_mismatch_confirmation: String::new(),
        };
        assert!(!transfer_same_database(&spec, "src", "dst"));
        assert!(transfer_same_database(&spec, "src", "src"));
    }

    #[test]
    fn rejects_foreign_key_cycles() {
        let left = TransferTableSpec { schema: "dbo".to_string(), table: "left".to_string(), target_schema: String::new(), target_table: String::new() };
        let right = TransferTableSpec { schema: "dbo".to_string(), table: "right".to_string(), target_schema: String::new(), target_table: String::new() };
        let relations = [
            RelationshipInfo { constraint_name: "fk_left".to_string(), source_schema: "dbo".to_string(), source_table: "left".to_string(), source_column: "right_id".to_string(), target_schema: "dbo".to_string(), target_table: "right".to_string(), target_column: "id".to_string(), ordinal: 1 },
            RelationshipInfo { constraint_name: "fk_right".to_string(), source_schema: "dbo".to_string(), source_table: "right".to_string(), source_column: "left_id".to_string(), target_schema: "dbo".to_string(), target_table: "left".to_string(), target_column: "id".to_string(), ordinal: 1 },
        ];
        assert!(table_order(&[left, right], &relations).is_err());
    }
}
