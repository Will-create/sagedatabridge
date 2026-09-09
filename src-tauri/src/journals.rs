use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use tauri::State;

use crate::analytics::TableXlsxWriter;
use crate::db;
use crate::sage_compat::{SageEdition, SageSchema};
use crate::sage_entity_service::{
    self, parse_f64, parse_i64, parse_string, sql_string, ResolvedTable,
};
use crate::state::{AppState, TableInfo};

const JOURNAL_TYPES: &[&str] = &["purchase", "sales", "treasury", "general", "other"];
const JOURNAL_STREAM_CHUNK_SIZE: i64 = 2_000;
const JOURNAL_EXPORT_ROWS_PER_PART: i64 = 800_000;
const XLSX_WRITE_BUFFER_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalDefinition {
    pub code: String,
    pub name: String,
    pub journal_type: String,
    pub raw_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JournalCatalogResponse {
    pub journals: Vec<JournalDefinition>,
    pub date_min: Option<String>,
    pub date_max: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JournalEntry {
    pub date: String,
    pub journal_code: String,
    pub piece_number: String,
    pub account_number: String,
    pub label: String,
    pub debit: f64,
    pub credit: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JournalMonthlyStat {
    pub month: String,
    pub entry_count: i64,
    pub debit: f64,
    pub credit: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JournalBreakdownStat {
    pub journal_code: String,
    pub journal_name: String,
    pub journal_type: String,
    pub entry_count: i64,
    pub debit: f64,
    pub credit: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct JournalStatsResponse {
    pub total_entries: i64,
    pub total_debit: f64,
    pub total_credit: f64,
    pub balance: f64,
    pub monthly: Vec<JournalMonthlyStat>,
    pub by_journal: Vec<JournalBreakdownStat>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct JournalStreamTotalPayload {
    request_id: String,
    total: i64,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct JournalStreamChunkPayload {
    request_id: String,
    rows: Vec<JournalEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct JournalStreamCompletePayload {
    request_id: String,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct JournalStreamErrorPayload {
    request_id: String,
    error: String,
}

#[derive(Debug, Clone, Serialize)]
struct JournalExportProgressPayload {
    request_id: String,
    loaded_rows: i64,
    total_rows: i64,
    percent: u8,
    current_sheet: u32,
    current_part: u32,
    total_parts: u32,
    phase: String,
    output_path: Option<String>,
}

#[derive(Debug, Clone)]
struct EntrySqlContext {
    from_sql: String,
    date_expr: String,
    journal_expr: String,
    piece_expr: String,
    account_expr: String,
    label_expr: String,
    debit_expr: String,
    credit_expr: String,
}

fn q(alias: &str, column: &str) -> String {
    sage_entity_service::qualify(alias, column)
}

fn push_candidate(candidates: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !value.trim().is_empty()
        && !candidates
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&value))
    {
        candidates.push(value);
    }
}

fn pick(table: &ResolvedTable, hint: Option<&str>, candidates: &[&str]) -> Option<String> {
    hint.filter(|value| !value.trim().is_empty())
        .and_then(|value| table.column(value).map(|column| column.name.clone()))
        .or_else(|| sage_entity_service::pick_optional(table, candidates))
}

async fn load_optional(
    client: &mut db::DbClient,
    tables: &[TableInfo],
    candidates: Vec<String>,
) -> Option<ResolvedTable> {
    sage_entity_service::load_table(client, tables, &candidates)
        .await
        .ok()
}

fn direct_amount_exprs(
    table: &ResolvedTable,
    alias: &str,
    schema: &SageSchema,
) -> Result<(String, String), String> {
    let debit = pick(
        table,
        Some(&schema.col_debit),
        &["EC_Debit", "DEBIT", "MVT_DEBIT", "MOUVEMENT_DEBIT"],
    );
    let credit = pick(
        table,
        Some(&schema.col_credit),
        &["EC_Credit", "CREDIT", "MVT_CREDIT", "MOUVEMENT_CREDIT"],
    );
    if let (Some(debit), Some(credit)) = (debit, credit) {
        return Ok((
            format!(
                "COALESCE(TRY_CONVERT(DECIMAL(38,6), {}), 0)",
                q(alias, &debit)
            ),
            format!(
                "COALESCE(TRY_CONVERT(DECIMAL(38,6), {}), 0)",
                q(alias, &credit)
            ),
        ));
    }

    let amount = pick(table, None, &["EC_Montant", "MONTANT", "AMOUNT"]);
    let direction = pick(table, None, &["EC_Sens", "SENS", "ENTRY_SIDE"]);
    if let (Some(amount), Some(direction)) = (amount, direction) {
        let amount = format!(
            "COALESCE(TRY_CONVERT(DECIMAL(38,6), {}), 0)",
            q(alias, &amount)
        );
        let direction = q(alias, &direction);
        return Ok((
            format!(
                "CASE WHEN COALESCE(TRY_CONVERT(INT, {direction}), 0) = 0 THEN {amount} ELSE 0 END"
            ),
            format!(
                "CASE WHEN COALESCE(TRY_CONVERT(INT, {direction}), 0) = 1 THEN {amount} ELSE 0 END"
            ),
        ));
    }

    if let Some(amount) = pick(table, None, &["AMTCUR", "AMOUNT_CURRENCY"]) {
        let amount = format!(
            "COALESCE(TRY_CONVERT(DECIMAL(38,6), {}), 0)",
            q(alias, &amount)
        );
        return Ok((
            format!("CASE WHEN {amount} > 0 THEN {amount} ELSE 0 END"),
            format!("CASE WHEN {amount} < 0 THEN ABS({amount}) ELSE 0 END"),
        ));
    }

    Err(format!(
        "{} is missing supported debit/credit or amount/direction columns",
        table.quoted_name()
    ))
}

async fn resolve_direct_context(
    client: &mut db::DbClient,
    tables: &[TableInfo],
    schema: &SageSchema,
) -> Result<EntrySqlContext, String> {
    let mut candidates = Vec::new();
    push_candidate(&mut candidates, schema.table_ecritures.clone());
    for candidate in [
        "F_ECRITUREC",
        "ECRITUREC",
        "GACCENTRY",
        "JOURNAL_ENTRIES",
        "GL_ENTRIES",
    ] {
        push_candidate(&mut candidates, candidate);
    }
    let entries = sage_entity_service::load_table(client, tables, &candidates).await?;

    let date = pick(
        &entries,
        Some(&schema.col_date),
        &[
            "EC_Date",
            "ACCDAT",
            "DATE_ECR",
            "ENTRY_DATE",
            "JM_Date",
            "DATE",
        ],
    )
    .ok_or_else(|| {
        format!(
            "{} is missing a supported date column",
            entries.quoted_name()
        )
    })?;
    let journal = pick(
        &entries,
        Some(&schema.col_journal),
        &["JO_Num", "JOU", "CODE_JOURNAL", "JRN_CODE", "JOURNAL_CODE"],
    )
    .ok_or_else(|| {
        format!(
            "{} is missing a supported journal column",
            entries.quoted_name()
        )
    })?;
    let account = sage_entity_service::pick_optional(
        &entries,
        &["CG_Num", "ACC", "COMPTE", "ACCOUNT_NO", "CT_Num"],
    )
    .or_else(|| pick(&entries, Some(&schema.col_compte), &[]))
    .ok_or_else(|| {
        format!(
            "{} is missing a supported account column",
            entries.quoted_name()
        )
    })?;
    let piece = pick(
        &entries,
        Some(&schema.col_piece),
        &["EC_Piece", "EC_RefPiece", "NUM", "PIECE_NO", "REFERENCE"],
    );
    let label = pick(
        &entries,
        Some(&schema.col_libelle),
        &[
            "EC_Intitule",
            "EC_Libelle",
            "DES",
            "LIBELLE",
            "DESCRIPTION",
            "NARRATION",
        ],
    );
    let (debit_expr, credit_expr) = direct_amount_exprs(&entries, "e", schema)?;

    Ok(EntrySqlContext {
        from_sql: format!("{} e", entries.quoted_name()),
        date_expr: q("e", &date),
        journal_expr: q("e", &journal),
        piece_expr: piece
            .map(|column| q("e", &column))
            .unwrap_or_else(|| "N''".to_string()),
        account_expr: q("e", &account),
        label_expr: label
            .map(|column| q("e", &column))
            .unwrap_or_else(|| "N''".to_string()),
        debit_expr,
        credit_expr,
    })
}

async fn resolve_sage1000_context(
    client: &mut db::DbClient,
    tables: &[TableInfo],
    schema: &SageSchema,
) -> Result<EntrySqlContext, String> {
    let entries = sage_entity_service::load_table(
        client,
        tables,
        &[schema.table_ecritures.clone(), "TECRITURE".to_string()],
    )
    .await?;
    let pieces = load_optional(client, tables, vec!["TPIECE".to_string()]).await;
    let journals = load_optional(client, tables, vec!["TJOURNAL".to_string()]).await;
    let accounts = load_optional(client, tables, vec!["TCOMPTEGENERAL".to_string()]).await;

    let date = pick(
        &entries,
        Some(&schema.col_date),
        &["eDate", "date", "ENTRY_DATE"],
    )
    .ok_or_else(|| {
        format!(
            "{} is missing a supported date column",
            entries.quoted_name()
        )
    })?;
    let label = pick(
        &entries,
        Some(&schema.col_libelle),
        &["reference", "libelle", "Caption"],
    );
    let (debit_expr, credit_expr) = direct_amount_exprs(&entries, "e", schema)?;

    let mut joins = Vec::new();
    let mut piece_expr = pick(&entries, Some(&schema.col_piece), &["numero", "reference"])
        .map(|column| q("e", &column));
    let mut journal_expr = pick(
        &entries,
        Some(&schema.col_journal),
        &["codeJournal", "journal"],
    )
    .map(|column| q("e", &column));

    if let Some(piece_table) = pieces.as_ref() {
        if let (Some(entry_piece), Some(piece_id)) = (
            pick(&entries, None, &["oidpiece", "oidPiece"]),
            pick(piece_table, None, &["oid", "id"]),
        ) {
            joins.push(format!(
                "LEFT JOIN {} p ON {} = {}",
                piece_table.quoted_name(),
                q("e", &entry_piece),
                q("p", &piece_id)
            ));
            piece_expr = pick(piece_table, None, &["numero", "reference", "code"])
                .map(|column| q("p", &column))
                .or(piece_expr);

            if let Some(journal_table) = journals.as_ref() {
                if let (Some(piece_journal), Some(journal_id)) = (
                    pick(piece_table, None, &["oidjournal", "oidJournal"]),
                    pick(journal_table, None, &["oid", "id"]),
                ) {
                    joins.push(format!(
                        "LEFT JOIN {} j ON {} = {}",
                        journal_table.quoted_name(),
                        q("p", &piece_journal),
                        q("j", &journal_id)
                    ));
                    journal_expr = pick(
                        journal_table,
                        None,
                        &["code", "codeJournal", "numero", "Caption"],
                    )
                    .map(|column| q("j", &column))
                    .or(journal_expr);
                }
            }
        }
    }

    let mut account_expr = pick(
        &entries,
        Some(&schema.col_compte),
        &["codeCompte", "compteGeneral", "ACCOUNT_NO"],
    )
    .map(|column| q("e", &column));
    if let Some(account_table) = accounts.as_ref() {
        if let (Some(entry_account), Some(account_id)) = (
            pick(&entries, None, &["oidcompteGeneral", "oidCompteGeneral"]),
            pick(account_table, None, &["oid", "id"]),
        ) {
            joins.push(format!(
                "LEFT JOIN {} c ON {} = {}",
                account_table.quoted_name(),
                q("e", &entry_account),
                q("c", &account_id)
            ));
            account_expr = pick(account_table, None, &["codeCompte", "code", "numero"])
                .map(|column| q("c", &column))
                .or(account_expr);
        }
    }

    Ok(EntrySqlContext {
        from_sql: format!(
            "{} e{}",
            entries.quoted_name(),
            if joins.is_empty() {
                String::new()
            } else {
                format!("\n{}", joins.join("\n"))
            }
        ),
        date_expr: q("e", &date),
        journal_expr: journal_expr.ok_or_else(|| {
            "TECRITURE/TPIECE/TJOURNAL is missing a resolvable journal code".to_string()
        })?,
        piece_expr: piece_expr.unwrap_or_else(|| "N''".to_string()),
        account_expr: account_expr.ok_or_else(|| {
            "TECRITURE/TCOMPTEGENERAL is missing a resolvable account code".to_string()
        })?,
        label_expr: label
            .map(|column| q("e", &column))
            .unwrap_or_else(|| "N''".to_string()),
        debit_expr,
        credit_expr,
    })
}

async fn resolve_entry_context(
    client: &mut db::DbClient,
    schema: &SageSchema,
) -> Result<EntrySqlContext, String> {
    let tables = db::get_tables(client).await?;
    let has_sage1000 = tables
        .iter()
        .any(|table| table.name.eq_ignore_ascii_case("TECRITURE"));
    if matches!(schema.edition, SageEdition::Sage1000) || has_sage1000 {
        resolve_sage1000_context(client, &tables, schema).await
    } else {
        resolve_direct_context(client, &tables, schema).await
    }
}

fn normalize_text(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .replace(['é', 'è', 'ê', 'ë'], "e")
        .replace(['à', 'â', 'ä'], "a")
        .replace(['ô', 'ö'], "o")
        .replace(['ù', 'û', 'ü'], "u")
        .replace(['î', 'ï'], "i")
        .replace('ç', "c")
}

fn normalize_journal_type(raw: &str, name: &str, code: &str, edition: &SageEdition) -> String {
    let raw_normalized = normalize_text(raw);
    if matches!(edition, SageEdition::Sage100) {
        match raw_normalized.as_str() {
            "0" => return "purchase".to_string(),
            "1" => return "sales".to_string(),
            "2" => return "treasury".to_string(),
            "4" => return "general".to_string(),
            _ => {}
        }
    }

    let combined = format!(
        "{} {} {}",
        raw_normalized,
        normalize_text(name),
        normalize_text(code)
    );
    let code = normalize_text(code).to_uppercase();
    if combined.contains("achat")
        || combined.contains("purchase")
        || code == "AC"
        || code.starts_with("ACH")
    {
        "purchase".to_string()
    } else if combined.contains("vente")
        || combined.contains("sales")
        || code.starts_with("VE")
        || code.starts_with("VT")
    {
        "sales".to_string()
    } else if combined.contains("tresor")
        || combined.contains("banque")
        || combined.contains("bank")
        || combined.contains("cash")
        || code.starts_with("BQ")
        || code.starts_with("BAN")
        || code.starts_with("CAI")
    {
        "treasury".to_string()
    } else if combined.contains("operation diverse")
        || combined.contains("general")
        || combined.contains("misc")
        || code.starts_with("OD")
    {
        "general".to_string()
    } else {
        "other".to_string()
    }
}

async fn load_catalog(
    client: &mut db::DbClient,
    schema: &SageSchema,
    entries: &EntrySqlContext,
) -> Result<JournalCatalogResponse, String> {
    let tables = db::get_tables(client).await?;
    let mut candidates = Vec::new();
    match schema.edition {
        SageEdition::Sage100 => {
            push_candidate(&mut candidates, "F_JOURNAUX");
            push_candidate(&mut candidates, "F_JOURNAL");
        }
        SageEdition::Sage1000 => push_candidate(&mut candidates, "TJOURNAL"),
        SageEdition::SageX3 => push_candidate(&mut candidates, "GJOURNAL"),
        SageEdition::Generic => {}
    }
    push_candidate(&mut candidates, schema.table_journaux.clone());
    for candidate in [
        "F_JOURNAUX",
        "F_JOURNAL",
        "TJOURNAL",
        "GJOURNAL",
        "JOURNALS",
    ] {
        push_candidate(&mut candidates, candidate);
    }

    let mut warnings = Vec::new();
    let mut journals = Vec::new();
    if let Some(table) = load_optional(client, &tables, candidates).await {
        if let Some(code) = sage_entity_service::pick_optional(
            &table,
            &[
                "JO_Num",
                "code",
                "codeJournal",
                "JOU",
                "numero",
                "JOURNAL_CODE",
            ],
        ) {
            let name = sage_entity_service::pick_optional(
                &table,
                &[
                    "JO_Intitule",
                    "Caption",
                    "DES",
                    "libelle",
                    "intitule",
                    "name",
                ],
            );
            let raw_type = sage_entity_service::pick_optional(
                &table,
                &[
                    "JO_Type",
                    "typeJournal",
                    "JOUTYP",
                    "type",
                    "nature",
                    "JOURNAL_TYPE",
                ],
            );
            let name_expr = name
                .map(|column| sage_entity_service::br(&column))
                .unwrap_or_else(|| sage_entity_service::br(&code));
            let type_expr = raw_type
                .map(|column| sage_entity_service::br(&column))
                .unwrap_or_else(|| "N''".to_string());
            let sql = format!(
                "SELECT CAST({code} AS NVARCHAR(100)), CAST({name} AS NVARCHAR(255)), CAST({kind} AS NVARCHAR(100)) FROM {table} WHERE NULLIF(LTRIM(RTRIM(CAST({code} AS NVARCHAR(100)))), N'') IS NOT NULL ORDER BY CAST({code} AS NVARCHAR(100))",
                code = sage_entity_service::br(&code),
                name = name_expr,
                kind = type_expr,
                table = table.quoted_name(),
            );
            let data = db::execute_raw_query(client, &sql).await?;
            for row in &data.rows {
                let code = parse_string(row.first());
                let name = parse_string(row.get(1));
                let raw_type = parse_string(row.get(2));
                journals.push(JournalDefinition {
                    journal_type: normalize_journal_type(&raw_type, &name, &code, &schema.edition),
                    code: code.clone(),
                    name: if name.is_empty() { code } else { name },
                    raw_type,
                });
            }
        }
    }

    if journals.is_empty() {
        warnings.push(
            "No native journal definition table was resolved; journal codes were derived from entries"
                .to_string(),
        );
        let sql = format!(
            "SELECT DISTINCT CAST({journal} AS NVARCHAR(100)) FROM {from_sql} WHERE NULLIF(LTRIM(RTRIM(CAST({journal} AS NVARCHAR(100)))), N'') IS NOT NULL ORDER BY CAST({journal} AS NVARCHAR(100))",
            journal = entries.journal_expr,
            from_sql = entries.from_sql,
        );
        let data = db::execute_raw_query(client, &sql).await?;
        for row in &data.rows {
            let code = parse_string(row.first());
            journals.push(JournalDefinition {
                journal_type: normalize_journal_type("", &code, &code, &schema.edition),
                code: code.clone(),
                name: code,
                raw_type: String::new(),
            });
        }
    }

    journals.sort_by(|left, right| left.code.to_lowercase().cmp(&right.code.to_lowercase()));
    journals.dedup_by(|left, right| left.code.eq_ignore_ascii_case(&right.code));
    Ok(JournalCatalogResponse {
        journals,
        date_min: None,
        date_max: None,
        warnings,
    })
}

async fn load_period_bounds(
    client: &mut db::DbClient,
    entries: &EntrySqlContext,
) -> Result<(Option<String>, Option<String>), String> {
    let period_sql = format!(
        "SELECT CONVERT(char(10), MIN(TRY_CAST({date} AS date)), 23), CONVERT(char(10), MAX(TRY_CAST({date} AS date)), 23) FROM {from_sql}",
        date = entries.date_expr,
        from_sql = entries.from_sql,
    );
    let period_data = db::execute_raw_query(client, &period_sql).await?;
    let date_min = period_data
        .rows
        .first()
        .map(|row| parse_string(row.first()))
        .filter(|value| !value.is_empty());
    let date_max = period_data
        .rows
        .first()
        .map(|row| parse_string(row.get(1)))
        .filter(|value| !value.is_empty());
    Ok((date_min, date_max))
}

fn resolved_schema(state: &State<'_, AppState>, id: &str) -> Result<SageSchema, String> {
    let connection = state.resolve_connection_config(id)?;
    Ok(state.get_connection_schema(id).unwrap_or_else(|| {
        connection.custom_schema.unwrap_or_else(|| {
            SageSchema::for_edition(&SageEdition::from_edition_str(&connection.sage_edition))
        })
    }))
}

fn validate_type(value: Option<String>) -> Result<Option<String>, String> {
    let value = value
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| !item.is_empty() && item != "all");
    if let Some(ref value) = value {
        if !JOURNAL_TYPES.contains(&value.as_str()) {
            return Err(format!("Unsupported journal type: {value}"));
        }
    }
    Ok(value)
}

fn filter_codes(
    catalog: &[JournalDefinition],
    journal_type: Option<&str>,
    journal_code: Option<&str>,
) -> Result<Vec<String>, String> {
    if let Some(code) = journal_code.filter(|value| !value.trim().is_empty()) {
        let definition = catalog
            .iter()
            .find(|journal| journal.code.eq_ignore_ascii_case(code.trim()))
            .ok_or_else(|| format!("Unknown journal code: {}", code.trim()))?;
        if journal_type
            .map(|kind| kind != definition.journal_type)
            .unwrap_or(false)
        {
            return Ok(Vec::new());
        }
        return Ok(vec![definition.code.clone()]);
    }

    Ok(catalog
        .iter()
        .filter(|journal| {
            journal_type
                .map(|kind| kind == journal.journal_type)
                .unwrap_or(true)
        })
        .map(|journal| journal.code.clone())
        .collect())
}

fn validate_date_range(date_from: &str, date_to: &str) -> Result<(String, String), String> {
    let from = NaiveDate::parse_from_str(date_from, "%Y-%m-%d")
        .map_err(|_| "Invalid journal start date; expected YYYY-MM-DD".to_string())?;
    let to = NaiveDate::parse_from_str(date_to, "%Y-%m-%d")
        .map_err(|_| "Invalid journal end date; expected YYYY-MM-DD".to_string())?;
    if from > to {
        return Err("Journal start date must not be after end date".to_string());
    }
    Ok((
        from.format("%Y-%m-%d").to_string(),
        to.format("%Y-%m-%d").to_string(),
    ))
}

fn where_clause(
    context: &EntrySqlContext,
    codes: &[String],
    filtering: bool,
    date_from: &str,
    date_to: &str,
) -> String {
    let mut conditions = vec![format!(
        "NULLIF(LTRIM(RTRIM(CAST({} AS NVARCHAR(100)))), N'') IS NOT NULL",
        context.journal_expr
    )];
    conditions.push(format!(
        "TRY_CAST({} AS date) BETWEEN CAST({} AS date) AND CAST({} AS date)",
        context.date_expr,
        sql_string(date_from),
        sql_string(date_to),
    ));
    if filtering && codes.is_empty() {
        conditions.push("1 = 0".to_string());
    } else if filtering {
        let values = codes
            .iter()
            .map(|code| sql_string(code))
            .collect::<Vec<_>>()
            .join(", ");
        conditions.push(format!(
            "CAST({} AS NVARCHAR(100)) IN ({values})",
            context.journal_expr
        ));
    }
    format!("WHERE {}", conditions.join(" AND "))
}

fn entries_sql(context: &EntrySqlContext, where_sql: &str, offset: i64, limit: i64) -> String {
    format!(
        r#"
SELECT
    CONVERT(char(10), TRY_CAST({date} AS date), 23) AS [entry_date],
    CAST({journal} AS NVARCHAR(100)) AS [journal_code],
    COALESCE(CAST({piece} AS NVARCHAR(128)), N'') AS [piece_number],
    COALESCE(CAST({account} AS NVARCHAR(100)), N'') AS [account_number],
    COALESCE(CAST({label} AS NVARCHAR(255)), N'') AS [label],
    CAST({debit} AS DECIMAL(38,6)) AS [debit],
    CAST({credit} AS DECIMAL(38,6)) AS [credit]
FROM {from_sql}
{where_sql}
ORDER BY TRY_CAST({date} AS date) DESC, CAST({journal} AS NVARCHAR(100)), CAST({piece} AS NVARCHAR(128)), CAST({account} AS NVARCHAR(100))
OFFSET {offset} ROWS FETCH NEXT {limit} ROWS ONLY
"#,
        date = context.date_expr,
        journal = context.journal_expr,
        piece = context.piece_expr,
        account = context.account_expr,
        label = context.label_expr,
        debit = context.debit_expr,
        credit = context.credit_expr,
        from_sql = context.from_sql,
    )
}

fn parse_entries(data: &crate::state::TableData) -> Vec<JournalEntry> {
    data.rows
        .iter()
        .map(|row| JournalEntry {
            date: parse_string(row.first()),
            journal_code: parse_string(row.get(1)),
            piece_number: parse_string(row.get(2)),
            account_number: parse_string(row.get(3)),
            label: parse_string(row.get(4)),
            debit: parse_f64(row.get(5)),
            credit: parse_f64(row.get(6)),
        })
        .collect()
}

async fn resolve_filter_codes(
    client: &mut db::DbClient,
    schema: &SageSchema,
    context: &EntrySqlContext,
    journal_type: Option<&str>,
    journal_code: Option<&str>,
) -> Result<(Vec<String>, Vec<String>, bool), String> {
    let filtering = journal_type.is_some() || journal_code.is_some();
    if !filtering {
        return Ok((Vec::new(), Vec::new(), false));
    }
    let catalog = load_catalog(client, schema, context).await?;
    let codes = filter_codes(&catalog.journals, journal_type, journal_code)?;
    Ok((codes, catalog.warnings, true))
}

fn query_timeout_secs(state: &AppState, minimum: u64) -> u64 {
    state
        .config
        .lock()
        .ok()
        .map(|config| config.dashboard_timeout_secs)
        .unwrap_or(90)
        .max(minimum)
}

#[tauri::command]
pub async fn get_accounting_journals(
    state: State<'_, AppState>,
    id: String,
) -> Result<JournalCatalogResponse, String> {
    let schema = resolved_schema(&state, &id)?;
    let mut client = db::get_cached_client_snapshot(state.inner(), &id).await?;
    let entries = resolve_entry_context(&mut client, &schema).await?;
    let mut catalog = load_catalog(&mut client, &schema, &entries).await?;
    let (date_min, date_max) = load_period_bounds(&mut client, &entries).await?;
    catalog.date_min = date_min;
    catalog.date_max = date_max;
    Ok(catalog)
}

#[tauri::command]
pub async fn get_accounting_journal_stats(
    state: State<'_, AppState>,
    id: String,
    date_from: String,
    date_to: String,
    journal_type: Option<String>,
    journal_code: Option<String>,
) -> Result<JournalStatsResponse, String> {
    let (date_from, date_to) = validate_date_range(&date_from, &date_to)?;
    let journal_type = validate_type(journal_type)?;
    let journal_code = journal_code
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let schema = resolved_schema(&state, &id)?;
    let mut client = db::get_cached_client_snapshot(state.inner(), &id).await?;
    let context = resolve_entry_context(&mut client, &schema).await?;
    let catalog = load_catalog(&mut client, &schema, &context).await?;
    let filtering = journal_type.is_some() || journal_code.is_some();
    let codes = if filtering {
        filter_codes(
            &catalog.journals,
            journal_type.as_deref(),
            journal_code.as_deref(),
        )?
    } else {
        Vec::new()
    };
    let where_sql = where_clause(&context, &codes, filtering, &date_from, &date_to);
    let timeout_secs = query_timeout_secs(state.inner(), 90);
    let totals_sql = format!(
        "SELECT COUNT_BIG(*), COALESCE(SUM(CAST({debit} AS DECIMAL(38,6))), 0), COALESCE(SUM(CAST({credit} AS DECIMAL(38,6))), 0) FROM {from_sql} {where_sql}",
        debit = context.debit_expr,
        credit = context.credit_expr,
        from_sql = context.from_sql,
    );
    let totals_data = db::execute_logged_query(
        &mut client,
        &totals_sql,
        db::QueryExecutionContext::new("journals", "stats_totals")
            .with_connection_id(&id)
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let totals_row = totals_data.rows.first();
    let total_entries = totals_row.map(|row| parse_i64(row.first())).unwrap_or(0);
    let total_debit = totals_row.map(|row| parse_f64(row.get(1))).unwrap_or(0.0);
    let total_credit = totals_row.map(|row| parse_f64(row.get(2))).unwrap_or(0.0);

    let monthly_sql = format!(
        "SELECT CONVERT(char(7), TRY_CAST({date} AS date), 120), COUNT_BIG(*), COALESCE(SUM(CAST({debit} AS DECIMAL(38,6))), 0), COALESCE(SUM(CAST({credit} AS DECIMAL(38,6))), 0) FROM {from_sql} {where_sql} GROUP BY CONVERT(char(7), TRY_CAST({date} AS date), 120) ORDER BY CONVERT(char(7), TRY_CAST({date} AS date), 120)",
        date = context.date_expr,
        debit = context.debit_expr,
        credit = context.credit_expr,
        from_sql = context.from_sql,
    );
    let monthly_data = db::execute_logged_query(
        &mut client,
        &monthly_sql,
        db::QueryExecutionContext::new("journals", "stats_monthly")
            .with_connection_id(&id)
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let monthly = monthly_data
        .rows
        .iter()
        .map(|row| JournalMonthlyStat {
            month: parse_string(row.first()),
            entry_count: parse_i64(row.get(1)),
            debit: parse_f64(row.get(2)),
            credit: parse_f64(row.get(3)),
        })
        .collect();

    let breakdown_sql = format!(
        "SELECT CAST({journal} AS NVARCHAR(100)), COUNT_BIG(*), COALESCE(SUM(CAST({debit} AS DECIMAL(38,6))), 0), COALESCE(SUM(CAST({credit} AS DECIMAL(38,6))), 0) FROM {from_sql} {where_sql} GROUP BY CAST({journal} AS NVARCHAR(100)) ORDER BY COUNT_BIG(*) DESC, CAST({journal} AS NVARCHAR(100))",
        journal = context.journal_expr,
        debit = context.debit_expr,
        credit = context.credit_expr,
        from_sql = context.from_sql,
    );
    let breakdown_data = db::execute_logged_query(
        &mut client,
        &breakdown_sql,
        db::QueryExecutionContext::new("journals", "stats_breakdown")
            .with_connection_id(&id)
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let by_journal = breakdown_data
        .rows
        .iter()
        .map(|row| {
            let code = parse_string(row.first());
            let definition = catalog
                .journals
                .iter()
                .find(|journal| journal.code.eq_ignore_ascii_case(&code));
            JournalBreakdownStat {
                journal_code: code.clone(),
                journal_name: definition
                    .map(|journal| journal.name.clone())
                    .unwrap_or_else(|| code.clone()),
                journal_type: definition
                    .map(|journal| journal.journal_type.clone())
                    .unwrap_or_else(|| "other".to_string()),
                entry_count: parse_i64(row.get(1)),
                debit: parse_f64(row.get(2)),
                credit: parse_f64(row.get(3)),
            }
        })
        .collect();

    Ok(JournalStatsResponse {
        total_entries,
        total_debit,
        total_credit,
        balance: total_debit - total_credit,
        monthly,
        by_journal,
        warnings: catalog.warnings,
    })
}

#[tauri::command]
pub async fn stream_accounting_journal_entries(
    window: tauri::Window,
    state: State<'_, AppState>,
    id: String,
    request_id: String,
    date_from: String,
    date_to: String,
    journal_type: Option<String>,
    journal_code: Option<String>,
) -> Result<(), String> {
    let result = do_stream_accounting_journal_entries(
        &window,
        state.inner(),
        &id,
        &request_id,
        &date_from,
        &date_to,
        journal_type,
        journal_code,
    )
    .await;
    if let Err(ref error) = result {
        let _ = window.emit(
            "journals_stream_error",
            JournalStreamErrorPayload {
                request_id,
                error: error.clone(),
            },
        );
    }
    result
}

async fn do_stream_accounting_journal_entries(
    window: &tauri::Window,
    state: &AppState,
    id: &str,
    request_id: &str,
    date_from: &str,
    date_to: &str,
    journal_type: Option<String>,
    journal_code: Option<String>,
) -> Result<(), String> {
    if request_id.trim().is_empty() {
        return Err("Journal stream request id is required".to_string());
    }
    let (date_from, date_to) = validate_date_range(date_from, date_to)?;
    let journal_type = validate_type(journal_type)?;
    let journal_code = journal_code
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let connection = state.resolve_connection_config(id)?;
    let schema = state.get_connection_schema(id).unwrap_or_else(|| {
        connection.custom_schema.unwrap_or_else(|| {
            SageSchema::for_edition(&SageEdition::from_edition_str(&connection.sage_edition))
        })
    });
    let mut client = db::get_cached_client_snapshot(state, id).await?;
    let context = resolve_entry_context(&mut client, &schema).await?;
    let (codes, warnings, filtering) = resolve_filter_codes(
        &mut client,
        &schema,
        &context,
        journal_type.as_deref(),
        journal_code.as_deref(),
    )
    .await?;
    let where_sql = where_clause(&context, &codes, filtering, &date_from, &date_to);
    let timeout_secs = query_timeout_secs(state, 90);

    let count_sql = format!(
        "SELECT COUNT_BIG(*) FROM {from_sql} {where_sql}",
        from_sql = context.from_sql,
    );
    let count_data = db::execute_logged_query(
        &mut client,
        &count_sql,
        db::QueryExecutionContext::new("journals", "stream_count")
            .with_connection_id(id)
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let total = count_data
        .rows
        .first()
        .map(|row| parse_i64(row.first()))
        .unwrap_or(0);
    let _ = window.emit(
        "journals_stream_total",
        JournalStreamTotalPayload {
            request_id: request_id.to_string(),
            total,
            warnings: warnings.clone(),
        },
    );

    let mut offset = 0i64;
    while offset < total {
        let sql = entries_sql(&context, &where_sql, offset, JOURNAL_STREAM_CHUNK_SIZE);
        let data = db::execute_logged_query(
            &mut client,
            &sql,
            db::QueryExecutionContext::new("journals", "stream_chunk")
                .with_connection_id(id)
                .with_timeout_secs(timeout_secs),
        )
        .await?;
        let fetched = data.rows.len() as i64;
        if fetched == 0 {
            break;
        }
        let _ = window.emit(
            "journals_stream_chunk",
            JournalStreamChunkPayload {
                request_id: request_id.to_string(),
                rows: parse_entries(&data),
            },
        );
        offset += fetched;
        if fetched < JOURNAL_STREAM_CHUNK_SIZE {
            break;
        }
    }

    let _ = window.emit(
        "journals_stream_complete",
        JournalStreamCompletePayload {
            request_id: request_id.to_string(),
            warnings,
        },
    );
    Ok(())
}

fn journal_part_path(base_path: &Path, part: u32, total_parts: u32) -> Result<PathBuf, String> {
    if total_parts <= 1 {
        return Ok(base_path.to_path_buf());
    }
    let stem = base_path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Invalid journal Excel export file name".to_string())?;
    let extension = base_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("xlsx");
    Ok(base_path.with_file_name(format!("{stem}.part-{part:03}.{extension}")))
}

fn journal_temp_path(final_path: &Path) -> Result<PathBuf, String> {
    let file_name = final_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Invalid journal Excel export file path".to_string())?;
    Ok(final_path.with_file_name(format!(".{file_name}.journals.tmp")))
}

fn cleanup_journal_temp_files(base_path: &Path) {
    let Some(parent) = base_path.parent() else {
        return;
    };
    let Some(stem) = base_path.file_stem().and_then(|value| value.to_str()) else {
        return;
    };
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let matches = path
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value.starts_with(&format!(".{stem}")) && value.ends_with(".journals.tmp"))
            .unwrap_or(false);
        if matches {
            let _ = fs::remove_file(path);
        }
    }
}

fn replace_journal_export(tmp_path: &Path, final_path: &Path) -> Result<(), String> {
    let backup_path = final_path.with_file_name(format!(
        ".{}.journals.backup",
        final_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "Invalid journal Excel export file path".to_string())?
    ));
    let _ = fs::remove_file(&backup_path);
    let mut moved_existing = false;
    if final_path.exists() {
        fs::rename(final_path, &backup_path)
            .map_err(|error| format!("Unable to preserve existing Excel file: {error}"))?;
        moved_existing = true;
    }
    if let Err(error) = fs::rename(tmp_path, final_path) {
        if moved_existing {
            let _ = fs::rename(&backup_path, final_path);
        }
        return Err(format!("Unable to finalize journal Excel export: {error}"));
    }
    if moved_existing {
        let _ = fs::remove_file(backup_path);
    }
    Ok(())
}

fn journal_headers(locale: &str) -> Vec<String> {
    if locale.to_ascii_lowercase().starts_with("fr") {
        [
            "Date",
            "Code journal",
            "Numéro de pièce",
            "Numéro de compte",
            "Libellé",
            "Débit",
            "Crédit",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    } else {
        [
            "Date",
            "Journal Code",
            "Piece Number",
            "Account Number",
            "Label",
            "Debit",
            "Credit",
        ]
        .into_iter()
        .map(str::to_string)
        .collect()
    }
}

fn create_journal_writer(
    path: &Path,
    locale: &str,
) -> Result<TableXlsxWriter<BufWriter<File>>, String> {
    let file = File::create(path)
        .map_err(|error| format!("Failed to create journal Excel export: {error}"))?;
    TableXlsxWriter::new(
        BufWriter::with_capacity(XLSX_WRITE_BUFFER_BYTES, file),
        journal_headers(locale),
    )
}

#[tauri::command]
pub async fn export_accounting_journals_xlsx(
    window: tauri::Window,
    state: State<'_, AppState>,
    id: String,
    request_id: String,
    date_from: String,
    date_to: String,
    journal_type: Option<String>,
    journal_code: Option<String>,
    file_path: String,
    locale: String,
) -> Result<Vec<String>, String> {
    cleanup_journal_temp_files(Path::new(&file_path));
    let result = do_export_accounting_journals_xlsx(
        &window,
        state.inner(),
        &id,
        &request_id,
        &date_from,
        &date_to,
        journal_type,
        journal_code,
        &file_path,
        &locale,
    )
    .await;
    if let Err(ref error) = result {
        cleanup_journal_temp_files(Path::new(&file_path));
        let _ = window.emit(
            "journals_export_error",
            JournalStreamErrorPayload {
                request_id,
                error: error.clone(),
            },
        );
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn do_export_accounting_journals_xlsx(
    window: &tauri::Window,
    state: &AppState,
    id: &str,
    request_id: &str,
    date_from: &str,
    date_to: &str,
    journal_type: Option<String>,
    journal_code: Option<String>,
    file_path: &str,
    locale: &str,
) -> Result<Vec<String>, String> {
    if request_id.trim().is_empty() {
        return Err("Journal export request id is required".to_string());
    }
    let (date_from, date_to) = validate_date_range(date_from, date_to)?;
    let journal_type = validate_type(journal_type)?;
    let journal_code = journal_code
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let connection = state.resolve_connection_config(id)?;
    let schema = state.get_connection_schema(id).unwrap_or_else(|| {
        connection.custom_schema.unwrap_or_else(|| {
            SageSchema::for_edition(&SageEdition::from_edition_str(&connection.sage_edition))
        })
    });
    let mut client = db::get_cached_client_snapshot(state, id).await?;
    let context = resolve_entry_context(&mut client, &schema).await?;
    let (codes, _warnings, filtering) = resolve_filter_codes(
        &mut client,
        &schema,
        &context,
        journal_type.as_deref(),
        journal_code.as_deref(),
    )
    .await?;
    let where_sql = where_clause(&context, &codes, filtering, &date_from, &date_to);
    let timeout_secs = query_timeout_secs(state, 300);
    let count_sql = format!(
        "SELECT COUNT_BIG(*) FROM {from_sql} {where_sql}",
        from_sql = context.from_sql,
    );
    let count_data = db::execute_logged_query(
        &mut client,
        &count_sql,
        db::QueryExecutionContext::new("journals", "export_count")
            .with_connection_id(id)
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let total = count_data
        .rows
        .first()
        .map(|row| parse_i64(row.first()))
        .unwrap_or(0);
    let total_parts = u32::try_from(
        ((total.max(1) + JOURNAL_EXPORT_ROWS_PER_PART - 1) / JOURNAL_EXPORT_ROWS_PER_PART).max(1),
    )
    .map_err(|_| "Journal export would create too many Excel files".to_string())?;
    let base_path = Path::new(file_path);
    let mut output_paths = Vec::new();
    let mut current_part = 1u32;
    let mut current_part_rows = 0i64;
    let mut final_path = journal_part_path(base_path, current_part, total_parts)?;
    let mut temp_path = journal_temp_path(&final_path)?;
    let _ = fs::remove_file(&temp_path);
    let mut writer = create_journal_writer(&temp_path, locale)?;
    let mut loaded = 0i64;
    let mut offset = 0i64;
    let _ = window.emit(
        "journals_export_progress",
        JournalExportProgressPayload {
            request_id: request_id.to_string(),
            loaded_rows: 0,
            total_rows: total,
            percent: 0,
            current_sheet: 1,
            current_part,
            total_parts,
            phase: "writing".to_string(),
            output_path: Some(final_path.display().to_string()),
        },
    );

    loop {
        let sql = entries_sql(&context, &where_sql, offset, JOURNAL_STREAM_CHUNK_SIZE);
        let data = db::execute_logged_query(
            &mut client,
            &sql,
            db::QueryExecutionContext::new("journals", "export_chunk")
                .with_connection_id(id)
                .with_timeout_secs(timeout_secs),
        )
        .await?;
        let fetched = data.rows.len() as i64;
        for entry in parse_entries(&data) {
            if current_part_rows >= JOURNAL_EXPORT_ROWS_PER_PART {
                writer.finish()?;
                replace_journal_export(&temp_path, &final_path)?;
                output_paths.push(final_path.display().to_string());
                current_part += 1;
                current_part_rows = 0;
                final_path = journal_part_path(base_path, current_part, total_parts)?;
                temp_path = journal_temp_path(&final_path)?;
                let _ = fs::remove_file(&temp_path);
                writer = create_journal_writer(&temp_path, locale)?;
            }
            writer.write_row(
                &[
                    entry.date,
                    entry.journal_code,
                    entry.piece_number,
                    entry.account_number,
                    entry.label,
                    entry.debit.to_string(),
                    entry.credit.to_string(),
                ],
                &[false, false, false, false, false, true, true],
            )?;
            current_part_rows += 1;
            loaded += 1;
        }
        writer.flush()?;
        let percent = if total <= 0 {
            100
        } else {
            ((loaded.min(total) * 100) / total).clamp(0, 99) as u8
        };
        let _ = window.emit(
            "journals_export_progress",
            JournalExportProgressPayload {
                request_id: request_id.to_string(),
                loaded_rows: loaded,
                total_rows: total,
                percent,
                current_sheet: 1,
                current_part,
                total_parts,
                phase: "writing".to_string(),
                output_path: Some(final_path.display().to_string()),
            },
        );
        if fetched < JOURNAL_STREAM_CHUNK_SIZE {
            break;
        }
        offset += fetched;
    }

    let sheet_count = writer.finish()?;
    replace_journal_export(&temp_path, &final_path)?;
    output_paths.push(final_path.display().to_string());
    let payload = JournalExportProgressPayload {
        request_id: request_id.to_string(),
        loaded_rows: loaded,
        total_rows: total,
        percent: 100,
        current_sheet: sheet_count,
        current_part,
        total_parts,
        phase: "complete".to_string(),
        output_path: Some(final_path.display().to_string()),
    };
    let _ = window.emit("journals_export_progress", payload.clone());
    let _ = window.emit("journals_export_complete", payload);
    Ok(output_paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal(code: &str, journal_type: &str) -> JournalDefinition {
        JournalDefinition {
            code: code.to_string(),
            name: code.to_string(),
            journal_type: journal_type.to_string(),
            raw_type: String::new(),
        }
    }

    #[test]
    fn maps_sage100_numeric_types() {
        let edition = SageEdition::Sage100;
        assert_eq!(normalize_journal_type("0", "", "AC", &edition), "purchase");
        assert_eq!(normalize_journal_type("1", "", "VE", &edition), "sales");
        assert_eq!(normalize_journal_type("2", "", "BQ", &edition), "treasury");
        assert_eq!(normalize_journal_type("4", "", "OD", &edition), "general");
        assert_eq!(
            normalize_journal_type("3", "Situation", "SI", &edition),
            "other"
        );
    }

    #[test]
    fn maps_textual_types_and_common_codes() {
        let edition = SageEdition::Generic;
        assert_eq!(
            normalize_journal_type("", "Journal d'achats", "X", &edition),
            "purchase"
        );
        assert_eq!(normalize_journal_type("sales", "", "X", &edition), "sales");
        assert_eq!(
            normalize_journal_type("", "Trésorerie", "X", &edition),
            "treasury"
        );
        assert_eq!(normalize_journal_type("", "", "OD", &edition), "general");
    }

    #[test]
    fn filters_codes_by_type_and_exact_code() {
        let catalog = vec![journal("AC", "purchase"), journal("VE", "sales")];
        assert_eq!(
            filter_codes(&catalog, Some("purchase"), None).unwrap(),
            vec!["AC"]
        );
        assert_eq!(
            filter_codes(&catalog, Some("purchase"), Some("ac")).unwrap(),
            vec!["AC"]
        );
        assert!(filter_codes(&catalog, Some("sales"), Some("AC"))
            .unwrap()
            .is_empty());
        assert!(filter_codes(&catalog, None, Some("missing")).is_err());
    }

    #[test]
    fn validates_public_type_values() {
        assert_eq!(validate_type(Some("ALL".into())).unwrap(), None);
        assert_eq!(
            validate_type(Some("sales".into())).unwrap(),
            Some("sales".into())
        );
        assert!(validate_type(Some("invalid".into())).is_err());
    }

    #[test]
    fn validates_inclusive_journal_date_ranges() {
        assert_eq!(
            validate_date_range("2026-01-01", "2026-12-31").unwrap(),
            ("2026-01-01".to_string(), "2026-12-31".to_string())
        );
        assert!(validate_date_range("2026-12-31", "2026-01-01").is_err());
        assert!(validate_date_range("31/12/2026", "2026-12-31").is_err());
    }

    #[test]
    fn journal_predicate_combines_dates_and_codes() {
        let context = EntrySqlContext {
            from_sql: "[dbo].[F_ECRITUREC] e".to_string(),
            date_expr: "e.[EC_Date]".to_string(),
            journal_expr: "e.[JO_Num]".to_string(),
            piece_expr: "e.[EC_Piece]".to_string(),
            account_expr: "e.[CG_Num]".to_string(),
            label_expr: "e.[EC_Intitule]".to_string(),
            debit_expr: "e.[EC_Debit]".to_string(),
            credit_expr: "e.[EC_Credit]".to_string(),
        };
        let sql = where_clause(
            &context,
            &["AC".to_string(), "VE".to_string()],
            true,
            "2026-01-01",
            "2026-12-31",
        );
        assert!(sql.contains("TRY_CAST(e.[EC_Date] AS date) BETWEEN"));
        assert!(sql.contains("N'2026-01-01'"));
        assert!(sql.contains("N'2026-12-31'"));
        assert!(sql.contains("N'AC', N'VE'"));
    }

    #[test]
    fn journal_excel_headers_follow_locale() {
        assert_eq!(journal_headers("fr")[2], "Numéro de pièce");
        assert_eq!(journal_headers("en")[2], "Piece Number");
    }
}
