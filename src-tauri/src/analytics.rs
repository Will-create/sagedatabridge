use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Seek, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tauri::State;

use crate::db;
use crate::sage_compat::{SageEdition, SageSchema};
use crate::sage_entity_service;
use crate::state::{AppState, ConnectionConfig, TableData};

const ENTRY_TABLE_CANDIDATES: &[&str] =
    &["F_ECRITUREC", "ECRITUREC", "GL_ENTRIES", "JOURNAL_ENTRIES"];
const ACCOUNT_TABLE_CANDIDATES: &[&str] = &[
    "F_COMPTEG",
    "COMPTEG",
    "F_COMPTET",
    "COMPTET",
    "ACCOUNTS",
    "GL_ACCOUNTS",
];
const TIERS_TABLE_CANDIDATES: &[&str] = &[
    "F_COMPTET",
    "COMPTET",
    "F_TIERS",
    "TIERS",
    "CUSTOMERS",
    "SUPPLIERS",
];

#[derive(Debug, Clone)]
struct BalancePeriod {
    date_from: String,
    date_to: String,
    annual_from: String,
    annual_to: String,
}

fn resolve_balance_period(date_from: &str, date_to: &str) -> Result<BalancePeriod, String> {
    let from = NaiveDate::parse_from_str(date_from, "%Y-%m-%d")
        .map_err(|_| "Invalid Balance start date; expected YYYY-MM-DD".to_string())?;
    let to = NaiveDate::parse_from_str(date_to, "%Y-%m-%d")
        .map_err(|_| "Invalid Balance end date; expected YYYY-MM-DD".to_string())?;
    if from > to {
        return Err("Balance start date must not be after end date".to_string());
    }
    let year_end = NaiveDate::from_ymd_opt(from.year(), 12, 31)
        .ok_or_else(|| "Unable to determine Balance calendar year".to_string())?;
    Ok(BalancePeriod {
        date_from: from.format("%Y-%m-%d").to_string(),
        date_to: to.format("%Y-%m-%d").to_string(),
        annual_from: from.format("%Y-%m-%d").to_string(),
        annual_to: std::cmp::min(to, year_end).format("%Y-%m-%d").to_string(),
    })
}

fn annual_account_condition(account_expr: &str) -> String {
    format!("({account_expr} LIKE '6%' OR {account_expr} LIKE '7%')")
}

fn non_opening_journal_condition(journal_expr: &str, journal_available: bool) -> String {
    if journal_available {
        format!("UPPER(LTRIM(RTRIM({journal_expr}))) NOT IN ('RAN', 'AN', 'OUV')")
    } else {
        "1 = 1".to_string()
    }
}

fn balance_movement_condition(
    account_expr: &str,
    date_expr: &str,
    journal_expr: &str,
    journal_available: bool,
    period: &BalancePeriod,
) -> String {
    let annual = annual_account_condition(account_expr);
    let non_opening = non_opening_journal_condition(journal_expr, journal_available);
    format!(
        "((NOT {annual} AND {date_expr} BETWEEN {from} AND {to}) OR ({annual} AND {date_expr} BETWEEN {annual_from} AND {annual_to} AND {non_opening}))",
        from = sql_literal(&period.date_from),
        to = sql_literal(&period.date_to),
        annual_from = sql_literal(&period.annual_from),
        annual_to = sql_literal(&period.annual_to),
    )
}

#[cfg(test)]
mod balance_period_tests {
    use super::{balance_movement_condition, resolve_balance_period};

    #[test]
    fn balance_period_keeps_a_same_year_range() {
        let period = resolve_balance_period("2025-03-01", "2025-09-30").unwrap();
        assert_eq!(period.annual_from, "2025-03-01");
        assert_eq!(period.annual_to, "2025-09-30");
    }

    #[test]
    fn balance_period_clamps_annual_accounts_to_start_year() {
        let period = resolve_balance_period("2025-07-01", "2026-06-30").unwrap();
        assert_eq!(period.annual_from, "2025-07-01");
        assert_eq!(period.annual_to, "2025-12-31");
    }

    #[test]
    fn balance_period_rejects_invalid_or_reversed_dates() {
        assert!(resolve_balance_period("2025-13-01", "2025-12-31").is_err());
        assert!(resolve_balance_period("2025-12-31", "2025-01-01").is_err());
    }

    #[test]
    fn annual_movement_condition_excludes_opening_journals() {
        let period = resolve_balance_period("2025-01-01", "2026-03-31").unwrap();
        let sql = balance_movement_condition("account_no", "parsed_date", "journal", true, &period);
        assert!(sql.contains("account_no LIKE '6%'"));
        assert!(sql.contains("account_no LIKE '7%'"));
        assert!(sql.contains("'2025-12-31'"));
        assert!(sql.contains("NOT IN ('RAN', 'AN', 'OUV')"));
    }
}

fn dashboard_timeout_secs(state: &AppState) -> u64 {
    state
        .config
        .lock()
        .ok()
        .map(|config| config.dashboard_timeout_secs)
        .unwrap_or(db::DASHBOARD_QUERY_TIMEOUT_SECS)
        .max(1)
}

#[derive(Debug, Clone)]
struct DetectedTable {
    schema: String,
    name: String,
}

impl DetectedTable {
    fn sql_name(&self) -> String {
        format!(
            "[{}].[{}]",
            self.schema.replace(']', "]]"),
            self.name.replace(']', "]]")
        )
    }
}

type ColumnMap = HashMap<String, String>;

#[derive(Debug, Clone)]
struct EntryContext {
    table: DetectedTable,
    date_col: String,
    journal_col: Option<String>,
    account_col: String,
    account_label_col: Option<String>,
    tiers_code_col: Option<String>,
    description_col: Option<String>,
    ref_piece_col: Option<String>,
    lettrage_col: Option<String>,
    debit_expr: String,
    credit_expr: String,
}

#[derive(Debug, Clone)]
struct LookupContext {
    table: DetectedTable,
    code_col: String,
    label_col: String,
    type_col: Option<String>,
}

#[derive(Debug, Clone)]
struct Sage1000Context {
    entries: DetectedTable,
    accounts: DetectedTable,
    tiers: Option<DetectedTable>,
    role_tiers: Option<DetectedTable>,
    pieces: Option<DetectedTable>,
    journals: Option<DetectedTable>,
}

struct Sage1000DocumentSql {
    join_sql: String,
    journal_expr: String,
    ref_piece_expr: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DashboardResponse<T> {
    pub data: T,
    pub warning: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct GrandLivreRow {
    pub row_type: String,
    pub date: Option<String>,
    pub journal: Option<String>,
    pub account_no: String,
    pub account_label: String,
    pub ref_piece: Option<String>,
    pub lettrage: Option<String>,
    pub debit: Decimal,
    pub credit: Decimal,
    pub solde_cumule: Decimal,
    pub total_debit: Option<Decimal>,
    pub total_credit: Option<Decimal>,
    pub solde: Option<Decimal>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct BalanceRow {
    pub account_no: String,
    pub account_label: String,
    pub ouverture_debit: Decimal,
    pub ouverture_credit: Decimal,
    pub mvt_debit: Decimal,
    pub mvt_credit: Decimal,
    pub cloture_debit: Decimal,
    pub cloture_credit: Decimal,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct AuxiliaireRow {
    pub account_no: String,
    pub account_label: String,
    pub tiers_code: String,
    pub tiers_name: String,
    pub date: Option<String>,
    pub journal: Option<String>,
    pub description: Option<String>,
    pub ref_piece: Option<String>,
    pub lettrage: Option<String>,
    pub debit: Decimal,
    pub credit: Decimal,
    pub running_balance: Decimal,
    pub is_total_row: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DashboardTopItem {
    pub account: String,
    pub label: String,
    pub amount: Decimal,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct MonthlyEvolutionPoint {
    pub month: String,
    pub ventes: Decimal,
    pub achats: Decimal,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DashboardKpis {
    pub total_ventes: Decimal,
    pub total_achats: Decimal,
    pub solde_clients: Decimal,
    pub solde_fournisseurs: Decimal,
    pub nb_ecritures: i64,
    pub nb_tiers_actifs: i64,
    pub top_charges: Vec<DashboardTopItem>,
    pub top_produits: Vec<DashboardTopItem>,
    pub evolution_mensuelle: Vec<MonthlyEvolutionPoint>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct DashboardTiersRow {
    pub code: String,
    pub name: String,
    pub tiers_type: String,
    pub account_no: Option<String>,
    pub debit: Option<Decimal>,
    pub credit: Option<Decimal>,
    pub balance: Option<Decimal>,
    pub last_movement_date: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ExploitationAccountMonthRow {
    pub account_no: String,
    pub account_label: String,
    pub month: u32,
    pub debit: Decimal,
    pub credit: Decimal,
}

fn load_connection_config(
    state: &State<'_, AppState>,
    id: &str,
) -> Result<ConnectionConfig, String> {
    state.resolve_connection_config(id)
}

async fn get_active_client(state: &AppState, id: &str) -> Result<db::DbClient, String> {
    db::get_cached_client_snapshot(state, id).await
}

fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn qualify(alias: &str, column: &str) -> String {
    format!("{}.[{}]", alias, column.replace(']', "]]"))
}

fn decimal_or_zero(expr: &str) -> String {
    format!(
        "COALESCE(TRY_CAST({} AS DECIMAL(38, 6)), CAST(0 AS DECIMAL(38, 6)))",
        expr
    )
}

fn text_or_empty(expr: &str, len: usize) -> String {
    format!(
        "COALESCE(CAST({} AS NVARCHAR({})), CAST('' AS NVARCHAR({})))",
        expr, len, len
    )
}

fn sage1000_piece_join(context: &Sage1000Context) -> String {
    context
        .pieces
        .as_ref()
        .map(|table| {
            format!(
                "LEFT JOIN {} p ON {} = {}",
                table.sql_name(),
                qualify("e", "oidpiece"),
                qualify("p", "oid")
            )
        })
        .unwrap_or_default()
}

fn sage1000_document_sql(context: &Sage1000Context) -> Sage1000DocumentSql {
    let piece_join = sage1000_piece_join(context);
    let journal_join = match (context.pieces.as_ref(), context.journals.as_ref()) {
        (Some(_), Some(table)) => format!(
            "LEFT JOIN {} j ON {} = {}",
            table.sql_name(),
            qualify("p", "oidjournal"),
            qualify("j", "oid")
        ),
        _ => String::new(),
    };
    let join_sql = [piece_join, journal_join]
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n            ");

    let journal_expr = match (context.pieces.as_ref(), context.journals.as_ref()) {
        (Some(_), Some(_)) => format!(
            "COALESCE(CAST({} AS NVARCHAR(64)), CAST({} AS NVARCHAR(64)), CAST({} AS NVARCHAR(64)), CAST('' AS NVARCHAR(64)))",
            qualify("j", "code"),
            qualify("j", "Caption"),
            qualify("p", "numero")
        ),
        (Some(_), None) => text_or_empty(&qualify("p", "numero"), 64),
        _ => "CAST('' AS NVARCHAR(64))".to_string(),
    };
    let ref_piece_expr = if context.pieces.is_some() {
        text_or_empty(&qualify("p", "numero"), 128)
    } else {
        "CAST('' AS NVARCHAR(128))".to_string()
    };

    Sage1000DocumentSql {
        join_sql,
        journal_expr,
        ref_piece_expr,
    }
}

fn join_warnings(warnings: Vec<String>) -> Option<String> {
    let filtered: Vec<String> = warnings
        .into_iter()
        .filter(|warning| !warning.trim().is_empty())
        .collect();

    if filtered.is_empty() {
        None
    } else {
        Some(filtered.join(" | "))
    }
}

fn is_sage1000_schema(schema: Option<&SageSchema>) -> bool {
    matches!(schema, Some(value) if value.edition == SageEdition::Sage1000)
}

fn sage1000_route_source(
    schema: Option<&SageSchema>,
    has_sage1000_tables: bool,
) -> Option<&'static str> {
    if is_sage1000_schema(schema) {
        Some("schema_hint")
    } else if has_sage1000_tables {
        Some("database_probe")
    } else {
        None
    }
}

fn pick_column(columns: &ColumnMap, candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find_map(|candidate| columns.get(&candidate.to_ascii_lowercase()).cloned())
}

fn value_as_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(if *flag { "1" } else { "0" }.to_string()),
        other => Some(other.to_string()),
    }
}

fn value_as_decimal(value: Option<&Value>) -> Decimal {
    match value {
        Some(Value::Number(number)) => {
            Decimal::from_str(&number.to_string()).unwrap_or(Decimal::ZERO)
        }
        Some(Value::String(text)) => Decimal::from_str(text.trim()).unwrap_or(Decimal::ZERO),
        Some(Value::Bool(flag)) => {
            if *flag {
                Decimal::ONE
            } else {
                Decimal::ZERO
            }
        }
        _ => Decimal::ZERO,
    }
}

fn value_as_i64(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Number(number)) => number.as_i64().unwrap_or(0),
        Some(Value::String(text)) => text.trim().parse::<i64>().unwrap_or(0),
        Some(Value::Bool(flag)) => {
            if *flag {
                1
            } else {
                0
            }
        }
        _ => 0,
    }
}

#[allow(dead_code)]
fn value_as_bool(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_i64().unwrap_or(0) != 0,
        Some(Value::String(text)) => matches!(
            text.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        ),
        _ => false,
    }
}

fn column_index_map(data: &TableData) -> HashMap<String, usize> {
    data.columns
        .iter()
        .enumerate()
        .map(|(index, column)| (column.name.to_ascii_lowercase(), index))
        .collect()
}

fn row_string(row: &[Value], indexes: &HashMap<String, usize>, name: &str) -> Option<String> {
    indexes
        .get(&name.to_ascii_lowercase())
        .and_then(|index| value_as_string(row.get(*index)))
}

fn row_decimal(row: &[Value], indexes: &HashMap<String, usize>, name: &str) -> Decimal {
    indexes
        .get(&name.to_ascii_lowercase())
        .map(|index| value_as_decimal(row.get(*index)))
        .unwrap_or(Decimal::ZERO)
}

fn row_optional_decimal(
    row: &[Value],
    indexes: &HashMap<String, usize>,
    name: &str,
) -> Option<Decimal> {
    let value = indexes
        .get(&name.to_ascii_lowercase())
        .and_then(|index| row.get(*index))?;
    if matches!(value, Value::Null) {
        None
    } else {
        Some(value_as_decimal(Some(value)))
    }
}

fn row_i64(row: &[Value], indexes: &HashMap<String, usize>, name: &str) -> i64 {
    indexes
        .get(&name.to_ascii_lowercase())
        .map(|index| value_as_i64(row.get(*index)))
        .unwrap_or(0)
}

#[allow(dead_code)]
fn row_bool(row: &[Value], indexes: &HashMap<String, usize>, name: &str) -> bool {
    indexes
        .get(&name.to_ascii_lowercase())
        .map(|index| value_as_bool(row.get(*index)))
        .unwrap_or(false)
}

async fn detect_table(client: &mut db::DbClient, candidates: &[&str]) -> Option<DetectedTable> {
    for candidate in candidates {
        let sql = format!(
            r#"
            SELECT TOP 1
                TABLE_SCHEMA AS schema_name,
                TABLE_NAME AS table_name
            FROM INFORMATION_SCHEMA.TABLES
            WHERE TABLE_TYPE = 'BASE TABLE'
              AND TABLE_NAME = {}
            ORDER BY CASE WHEN TABLE_SCHEMA = 'dbo' THEN 0 ELSE 1 END, TABLE_SCHEMA
            "#,
            sql_literal(candidate)
        );

        let Ok(data) = db::execute_raw_query(client, &sql).await else {
            continue;
        };
        let indexes = column_index_map(&data);

        if let Some(row) = data.rows.first() {
            let schema =
                row_string(row, &indexes, "schema_name").unwrap_or_else(|| "dbo".to_string());
            let name =
                row_string(row, &indexes, "table_name").unwrap_or_else(|| candidate.to_string());
            return Some(DetectedTable { schema, name });
        }
    }

    None
}

async fn detect_columns(
    client: &mut db::DbClient,
    table: &DetectedTable,
) -> Result<ColumnMap, String> {
    let sql = format!(
        r#"
        SELECT COLUMN_NAME
        FROM INFORMATION_SCHEMA.COLUMNS
        WHERE TABLE_SCHEMA = {}
          AND TABLE_NAME = {}
        ORDER BY ORDINAL_POSITION
        "#,
        sql_literal(&table.schema),
        sql_literal(&table.name)
    );

    let data = db::execute_raw_query(client, &sql).await?;
    let indexes = column_index_map(&data);
    let mut columns = HashMap::new();

    for row in &data.rows {
        if let Some(column_name) = row_string(row, &indexes, "column_name") {
            columns.insert(column_name.to_ascii_lowercase(), column_name);
        }
    }

    Ok(columns)
}

async fn resolve_sage1000_context(
    client: &mut db::DbClient,
) -> Result<Option<Sage1000Context>, String> {
    let Some(entries) = detect_table(client, &["TECRITURE"]).await else {
        return Ok(None);
    };
    let Some(accounts) = detect_table(client, &["TCOMPTEGENERAL"]).await else {
        return Ok(None);
    };

    Ok(Some(Sage1000Context {
        entries,
        accounts,
        tiers: detect_table(client, &["TTIERS"]).await,
        role_tiers: detect_table(client, &["TROLETIERS"]).await,
        pieces: detect_table(client, &["TPIECE"]).await,
        journals: detect_table(client, &["TJOURNAL"]).await,
    }))
}

async fn should_use_sage1000_analytics(
    client: &mut db::DbClient,
    schema: Option<&SageSchema>,
    endpoint: &str,
) -> Result<bool, String> {
    if let Some(source) = sage1000_route_source(schema, is_sage1000_schema(schema)) {
        eprintln!(
            "[analytics] sage1000_route endpoint={} source={} database={}",
            endpoint, source, client.config.database
        );
        return Ok(true);
    }

    let has_sage1000_tables = resolve_sage1000_context(client).await?.is_some();
    if let Some(source) = sage1000_route_source(schema, has_sage1000_tables) {
        eprintln!(
            "[analytics] sage1000_route endpoint={} source={} hint_schema={} database={}",
            endpoint,
            source,
            schema.map(|value| value.edition.as_str()).unwrap_or("none"),
            client.config.database
        );
        return Ok(true);
    }

    Ok(false)
}

fn resolve_amount_exprs(alias: &str, columns: &ColumnMap) -> Option<(String, String)> {
    let amount_col = pick_column(columns, &["EC_Montant", "MONTANT", "AMOUNT"]);
    let sens_col = pick_column(columns, &["EC_Sens", "SENS", "ENTRY_SIDE"]);

    if let (Some(amount_col), Some(sens_col)) = (amount_col, sens_col) {
        let amount_expr = decimal_or_zero(&qualify(alias, &amount_col));
        let sens_expr = qualify(alias, &sens_col);

        return Some((
            format!(
                "CASE WHEN COALESCE(TRY_CAST({} AS INT), 0) = 0 THEN {} ELSE CAST(0 AS DECIMAL(38, 6)) END",
                sens_expr, amount_expr
            ),
            format!(
                "CASE WHEN COALESCE(TRY_CAST({} AS INT), 0) = 1 THEN {} ELSE CAST(0 AS DECIMAL(38, 6)) END",
                sens_expr, amount_expr
            ),
        ));
    }

    let debit_col = pick_column(columns, &["DEBIT", "MVT_DEBIT", "MOUVEMENT_DEBIT"]);
    let credit_col = pick_column(columns, &["CREDIT", "MVT_CREDIT", "MOUVEMENT_CREDIT"]);

    match (debit_col, credit_col) {
        (Some(debit_col), Some(credit_col)) => Some((
            decimal_or_zero(&qualify(alias, &debit_col)),
            decimal_or_zero(&qualify(alias, &credit_col)),
        )),
        _ => None,
    }
}

async fn resolve_entry_context(
    client: &mut db::DbClient,
) -> Result<(Option<EntryContext>, Vec<String>), String> {
    let mut warnings = Vec::new();

    let Some(table) = detect_table(client, ENTRY_TABLE_CANDIDATES).await else {
        warnings.push("No accounting entries table detected".to_string());
        return Ok((None, warnings));
    };

    let columns = detect_columns(client, &table).await?;
    let date_col = pick_column(
        &columns,
        &[
            "EC_Date",
            "DATE_ECR",
            "TRS_DATE",
            "ENTRY_DATE",
            "JM_Date",
            "DATE",
        ],
    );
    let account_col = pick_column(&columns, &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"]);
    let journal_col = pick_column(
        &columns,
        &["JO_Num", "CODE_JOURNAL", "JRN_CODE", "JOURNAL_CODE"],
    );
    let account_label_col = pick_column(
        &columns,
        &[
            "CG_Intitule",
            "CT_Intitule",
            "LIBELLE_COMPTE",
            "ACCOUNT_LABEL",
            "ACCOUNT_NAME",
        ],
    );
    let tiers_code_col = pick_column(
        &columns,
        &[
            "CT_Num",
            "TIERS_CODE",
            "CODE_TIERS",
            "CUSTOMER_NO",
            "SUPPLIER_NO",
            "AUX_ACCOUNT",
            "THIRD_PARTY_CODE",
        ],
    );
    let description_col = pick_column(
        &columns,
        &[
            "EC_Intitule",
            "EC_Libelle",
            "LIBELLE",
            "DESCRIPTION",
            "ENTRY_LABEL",
            "NARRATION",
        ],
    );
    let ref_piece_col = pick_column(
        &columns,
        &[
            "EC_RefPiece",
            "EC_Piece",
            "REF_PIECE",
            "PIECE_NO",
            "REFERENCE",
        ],
    );
    let lettrage_col = pick_column(&columns, &["EC_Lettrage", "LETTRAGE", "MATCHING_CODE"]);
    let amount_exprs = resolve_amount_exprs("e", &columns);

    if date_col.is_none() {
        warnings.push(format!(
            "{} is missing a supported date column",
            table.sql_name()
        ));
    }
    if account_col.is_none() {
        warnings.push(format!(
            "{} is missing a supported account column",
            table.sql_name()
        ));
    }
    if amount_exprs.is_none() {
        warnings.push(format!(
            "{} is missing supported debit/credit columns",
            table.sql_name()
        ));
    }

    let context = match (date_col, account_col, amount_exprs) {
        (Some(date_col), Some(account_col), Some((debit_expr, credit_expr))) => {
            Some(EntryContext {
                table,
                date_col,
                journal_col,
                account_col,
                account_label_col,
                tiers_code_col,
                description_col,
                ref_piece_col,
                lettrage_col,
                debit_expr,
                credit_expr,
            })
        }
        _ => None,
    };

    Ok((context, warnings))
}

async fn resolve_lookup_context(
    client: &mut db::DbClient,
    candidates: &[&str],
    code_candidates: &[&str],
    label_candidates: &[&str],
    type_candidates: &[&str],
) -> Result<Option<LookupContext>, String> {
    let Some(table) = detect_table(client, candidates).await else {
        return Ok(None);
    };
    let columns = detect_columns(client, &table).await?;

    let Some(code_col) = pick_column(&columns, code_candidates) else {
        return Ok(None);
    };
    let Some(label_col) = pick_column(&columns, label_candidates) else {
        return Ok(None);
    };

    Ok(Some(LookupContext {
        table,
        code_col,
        label_col,
        type_col: pick_column(&columns, type_candidates),
    }))
}

/// Build an EntryContext from a known SageSchema by detecting the actual table in the DB.
async fn schema_entry_context(
    client: &mut db::DbClient,
    schema: &SageSchema,
) -> Result<Option<EntryContext>, String> {
    if schema.table_ecritures.is_empty()
        || schema.col_date.is_empty()
        || schema.col_compte.is_empty()
    {
        return Ok(None);
    }

    let Some(table) = detect_table(client, &[schema.table_ecritures.as_str()]).await else {
        return Ok(None);
    };

    let debit_expr = if schema
        .col_debit
        .trim_start()
        .to_uppercase()
        .starts_with("CASE")
    {
        decimal_or_zero(&schema.col_debit)
    } else {
        decimal_or_zero(&qualify("e", &schema.col_debit))
    };
    let credit_expr = if schema
        .col_credit
        .trim_start()
        .to_uppercase()
        .starts_with("CASE")
    {
        decimal_or_zero(&schema.col_credit)
    } else {
        decimal_or_zero(&qualify("e", &schema.col_credit))
    };

    Ok(Some(EntryContext {
        table,
        date_col: schema.col_date.clone(),
        journal_col: Some(schema.col_journal.clone()).filter(|s| !s.is_empty()),
        account_col: schema.col_compte.clone(),
        account_label_col: Some(schema.col_libelle.clone()).filter(|s| !s.is_empty()),
        tiers_code_col: Some(schema.col_tiers.clone()).filter(|s| !s.is_empty()),
        description_col: Some(schema.col_libelle.clone()).filter(|s| !s.is_empty()),
        ref_piece_col: Some(schema.col_piece.clone()).filter(|s| !s.is_empty()),
        lettrage_col: Some(schema.col_lettrage.clone()).filter(|s| !s.is_empty()),
        debit_expr,
        credit_expr,
    }))
}

/// Build a LookupContext from a known SageSchema.
async fn schema_lookup_context(
    client: &mut db::DbClient,
    table_name: &str,
    code_col: &str,
    label_col: &str,
    type_col: &str,
) -> Result<Option<LookupContext>, String> {
    if table_name.is_empty() || code_col.is_empty() || label_col.is_empty() {
        return Ok(None);
    }
    let Some(table) = detect_table(client, &[table_name]).await else {
        return Ok(None);
    };
    Ok(Some(LookupContext {
        table,
        code_col: code_col.to_string(),
        label_col: label_col.to_string(),
        type_col: if type_col.is_empty() {
            None
        } else {
            Some(type_col.to_string())
        },
    }))
}

/// Resolve entry context: prefer stored schema, fall back to auto-detection.
async fn resolve_entry_context_smart(
    client: &mut db::DbClient,
    hint: Option<&SageSchema>,
) -> Result<(Option<EntryContext>, Vec<String>), String> {
    if let Some(schema) = hint {
        if !schema.edition.is_generic() {
            if let Ok(Some(ctx)) = schema_entry_context(client, schema).await {
                return Ok((Some(ctx), vec![]));
            }
        }
    }
    resolve_entry_context(client).await
}

fn empty_vec_response<T>(warning: Vec<String>) -> DashboardResponse<Vec<T>> {
    DashboardResponse {
        data: Vec::new(),
        warning: join_warnings(warning),
    }
}

fn empty_kpi_response(warning: Vec<String>) -> DashboardResponse<DashboardKpis> {
    DashboardResponse {
        data: DashboardKpis::default(),
        warning: join_warnings(warning),
    }
}

async fn get_grand_livre_sage1000(
    client: &mut db::DbClient,
    date_from: &str,
    date_to: &str,
    account_prefix: Option<&str>,
    timeout_secs: u64,
) -> Result<DashboardResponse<Vec<GrandLivreRow>>, String> {
    let Some(context) = resolve_sage1000_context(client).await? else {
        return Ok(empty_vec_response(vec![
            "Sage 1000 tables TECRITURE and TCOMPTEGENERAL were not detected".to_string(),
        ]));
    };

    let document_sql = sage1000_document_sql(&context);
    let account_no_expr = text_or_empty(&qualify("cg", "codeCompte"), 64);
    let account_label_expr = format!(
        "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
        qualify("cg", "Caption"),
        qualify("e", "reference")
    );
    let lettrage_expr = text_or_empty(&qualify("e", "CodeLettrageExterne"), 64);
    let debit_expr = decimal_or_zero(&qualify("e", "debit"));
    let credit_expr = decimal_or_zero(&qualify("e", "credit"));
    let account_filter_sql = account_prefix
        .filter(|prefix| !prefix.trim().is_empty())
        .map(|prefix| {
            format!(
                " AND {} LIKE {}",
                account_no_expr,
                sql_literal(&format!("{}%", prefix.trim()))
            )
        })
        .unwrap_or_default();

    let sql = format!(
        r#"
        WITH base AS (
            SELECT
                parsed_date = TRY_CAST({date_col} AS DATE),
                journal = {journal_expr},
                account_no = {account_no_expr},
                account_label = {account_label_expr},
                ref_piece = {ref_piece_expr},
                lettrage = {lettrage_expr},
                debit = {debit_expr},
                credit = {credit_expr},
                entry_number = COALESCE(TRY_CAST({entry_number_col} AS BIGINT), 0)
            FROM {entries_table} e
            LEFT JOIN {accounts_table} cg ON {entry_account_fk} = {account_pk}
            {document_join}
            WHERE TRY_CAST({date_col} AS DATE) BETWEEN {date_from} AND {date_to}
              AND {account_no_expr} <> ''
              {account_filter_sql}
        ),
        ranked AS (
            SELECT
                CONVERT(char(10), parsed_date, 103) AS [date],
                journal,
                account_no,
                account_label,
                ref_piece,
                lettrage,
                debit,
                credit,
                SUM(debit - credit) OVER (
                    PARTITION BY account_no
                    ORDER BY parsed_date, entry_number
                    ROWS UNBOUNDED PRECEDING
                ) AS solde_cumule,
                ROW_NUMBER() OVER (
                    PARTITION BY account_no
                    ORDER BY parsed_date, entry_number
                ) AS sort_row
            FROM base
            WHERE parsed_date IS NOT NULL
        ),
        subtotal AS (
            SELECT
                CAST(NULL AS NVARCHAR(10)) AS [date],
                CAST(NULL AS NVARCHAR(64)) AS journal,
                account_no,
                MAX(account_label) AS account_label,
                CAST(NULL AS NVARCHAR(128)) AS ref_piece,
                CAST(NULL AS NVARCHAR(64)) AS lettrage,
                CAST(0 AS DECIMAL(38, 6)) AS debit,
                CAST(0 AS DECIMAL(38, 6)) AS credit,
                SUM(debit - credit) AS solde_cumule,
                CAST(1 AS BIT) AS is_subtotal_row,
                SUM(debit) AS total_debit,
                SUM(credit) AS total_credit,
                SUM(debit - credit) AS solde,
                2147483647 AS sort_row
            FROM base
            WHERE parsed_date IS NOT NULL
            GROUP BY account_no
        )
        SELECT
            [date],
            journal,
            account_no,
            account_label,
            ref_piece,
            lettrage,
            debit,
            credit,
            solde_cumule,
            is_subtotal_row,
            total_debit,
            total_credit,
            solde
        FROM (
            SELECT
                [date],
                journal,
                account_no,
                account_label,
                ref_piece,
                lettrage,
                debit,
                credit,
                solde_cumule,
                CAST(0 AS BIT) AS is_subtotal_row,
                CAST(NULL AS DECIMAL(38, 6)) AS total_debit,
                CAST(NULL AS DECIMAL(38, 6)) AS total_credit,
                CAST(NULL AS DECIMAL(38, 6)) AS solde,
                sort_row
            FROM ranked
            UNION ALL
            SELECT
                [date],
                journal,
                account_no,
                account_label,
                ref_piece,
                lettrage,
                debit,
                credit,
                solde_cumule,
                is_subtotal_row,
                total_debit,
                total_credit,
                solde,
                sort_row
            FROM subtotal
        ) ledger
        ORDER BY account_no, sort_row
        "#,
        date_col = qualify("e", "eDate"),
        journal_expr = document_sql.journal_expr,
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        ref_piece_expr = document_sql.ref_piece_expr,
        lettrage_expr = lettrage_expr,
        debit_expr = debit_expr,
        credit_expr = credit_expr,
        entry_number_col = qualify("e", "numero"),
        entries_table = context.entries.sql_name(),
        accounts_table = context.accounts.sql_name(),
        entry_account_fk = qualify("e", "oidcompteGeneral"),
        account_pk = qualify("cg", "oid"),
        document_join = document_sql.join_sql,
        date_from = sql_literal(date_from),
        date_to = sql_literal(date_to),
        account_filter_sql = account_filter_sql
    );

    let data = db::execute_logged_query(
        client,
        &sql,
        db::QueryExecutionContext::new("get_grand_livre", "dashboard_grand_livre_sage1000")
            .with_table(context.entries.sql_name())
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let indexes = column_index_map(&data);
    let rows = data
        .rows
        .iter()
        .map(|row| {
            let is_subtotal = row_bool(row, &indexes, "is_subtotal_row");
            GrandLivreRow {
                row_type: if is_subtotal {
                    "subtotal".to_string()
                } else {
                    "entry".to_string()
                },
                date: row_string(row, &indexes, "date"),
                journal: row_string(row, &indexes, "journal"),
                account_no: row_string(row, &indexes, "account_no").unwrap_or_default(),
                account_label: row_string(row, &indexes, "account_label").unwrap_or_default(),
                ref_piece: row_string(row, &indexes, "ref_piece"),
                lettrage: row_string(row, &indexes, "lettrage"),
                debit: row_decimal(row, &indexes, "debit"),
                credit: row_decimal(row, &indexes, "credit"),
                solde_cumule: row_decimal(row, &indexes, "solde_cumule"),
                total_debit: indexes
                    .contains_key("total_debit")
                    .then(|| row_decimal(row, &indexes, "total_debit"))
                    .filter(|value| is_subtotal || *value != Decimal::ZERO),
                total_credit: indexes
                    .contains_key("total_credit")
                    .then(|| row_decimal(row, &indexes, "total_credit"))
                    .filter(|value| is_subtotal || *value != Decimal::ZERO),
                solde: indexes
                    .contains_key("solde")
                    .then(|| row_decimal(row, &indexes, "solde"))
                    .filter(|value| is_subtotal || *value != Decimal::ZERO),
            }
        })
        .collect();

    Ok(DashboardResponse {
        data: rows,
        warning: None,
    })
}

/// Build the Sage 1000 balance from Sage-maintained exercise and period
/// cumulatives. Summing TECRITURE over all history repeats annual carry-forward
/// entries and produces incorrect opening balances.
async fn get_balance_sage1000_cumulative(
    client: &mut db::DbClient,
    context: &Sage1000Context,
    period: &BalancePeriod,
    account_prefix: Option<&str>,
    timeout_secs: u64,
) -> Result<Option<DashboardResponse<Vec<BalanceRow>>>, String> {
    let Some(period_cumul) = detect_table(client, &["TCUMULPERIODECOMPTE"]).await else {
        return Ok(None);
    };
    let Some(opening_cumul) = detect_table(client, &["TCUMULANOUVEAUCOMPTE"]).await else {
        return Ok(None);
    };
    let Some(exercises) = detect_table(client, &["TEXERCICE"]).await else {
        return Ok(None);
    };
    let Some(periods) = detect_table(client, &["TPERIODE"]).await else {
        return Ok(None);
    };

    let account_no = text_or_empty(&qualify("cg", "codeCompte"), 64);
    let account_filter_sql = account_prefix
        .filter(|prefix| !prefix.trim().is_empty())
        .map(|prefix| {
            format!(
                " AND {account_no} LIKE {}",
                sql_literal(&format!("{}%", prefix.trim()))
            )
        })
        .unwrap_or_default();

    let sql = format!(
        r#"
        WITH target_exercise AS (
            SELECT TOP (1) ex.oid, ex.dateDebut, ex.dateFin
            FROM {exercises} ex
            WHERE {date_from} BETWEEN CAST(ex.dateDebut AS date) AND CAST(ex.dateFin AS date)
            ORDER BY ex.dateDebut DESC
        ),
        previous_exercise AS (
            SELECT TOP (1) ex.oid
            FROM {exercises} ex
            CROSS JOIN target_exercise tx
            WHERE CAST(ex.dateFin AS date) < CAST(tx.dateDebut AS date)
            ORDER BY ex.dateFin DESC
        ),
        current_an_available AS (
            SELECT has_values = CASE WHEN COALESCE(SUM(ABS(COALESCE(an.debit, 0)) + ABS(COALESCE(an.credit, 0))), 0) <> 0 THEN 1 ELSE 0 END
            FROM {opening_cumul} an
            JOIN target_exercise tx ON an.oidExercice = tx.oid
            WHERE COALESCE(TRY_CAST(an.ApprocheNationale AS int), 0) = 1
              AND COALESCE(TRY_CAST(an.ApprocheIAS AS int), 0) = 0
        ),
        current_an AS (
            SELECT an.oidcompteGeneral, SUM(COALESCE(an.debit, 0)) debit, SUM(COALESCE(an.credit, 0)) credit
            FROM {opening_cumul} an
            JOIN target_exercise tx ON an.oidExercice = tx.oid
            WHERE COALESCE(TRY_CAST(an.ApprocheNationale AS int), 0) = 1
              AND COALESCE(TRY_CAST(an.ApprocheIAS AS int), 0) = 0
            GROUP BY an.oidcompteGeneral
        ),
        previous_close AS (
            SELECT source.oidcompteGeneral, SUM(source.debit) debit, SUM(source.credit) credit
            FROM (
                SELECT an.oidcompteGeneral, COALESCE(an.debit, 0) debit, COALESCE(an.credit, 0) credit
                FROM {opening_cumul} an
                JOIN previous_exercise px ON an.oidExercice = px.oid
                WHERE COALESCE(TRY_CAST(an.ApprocheNationale AS int), 0) = 1
                  AND COALESCE(TRY_CAST(an.ApprocheIAS AS int), 0) = 0
                UNION ALL
                SELECT cp.oidcompteGeneral, COALESCE(cp.debit, 0), COALESCE(cp.credit, 0)
                FROM {period_cumul} cp
                JOIN {periods} per ON cp.oidPeriode = per.oid
                JOIN previous_exercise px ON per.oidexercice = px.oid
                WHERE COALESCE(TRY_CAST(cp.ApprocheNationale AS int), 0) = 1
                  AND COALESCE(TRY_CAST(cp.ApprocheIAS AS int), 0) = 0
                  AND COALESCE(TRY_CAST(cp.typeLot AS int), 1) = 1
            ) source
            GROUP BY source.oidcompteGeneral
        ),
        opening_periods AS (
            SELECT cp.oidcompteGeneral, SUM(COALESCE(cp.debit, 0)) debit, SUM(COALESCE(cp.credit, 0)) credit
            FROM {period_cumul} cp
            JOIN {periods} per ON cp.oidPeriode = per.oid
            JOIN target_exercise tx ON per.oidexercice = tx.oid
            WHERE CAST(per.dateFin AS date) < {date_from}
              AND COALESCE(TRY_CAST(cp.ApprocheNationale AS int), 0) = 1
              AND COALESCE(TRY_CAST(cp.ApprocheIAS AS int), 0) = 0
              AND COALESCE(TRY_CAST(cp.typeLot AS int), 1) = 1
            GROUP BY cp.oidcompteGeneral
        ),
        movement_periods AS (
            SELECT cp.oidcompteGeneral, SUM(COALESCE(cp.debit, 0)) debit, SUM(COALESCE(cp.credit, 0)) credit
            FROM {period_cumul} cp
            JOIN {periods} per ON cp.oidPeriode = per.oid
            WHERE CAST(per.dateDebut AS date) >= {date_from}
              AND CAST(per.dateFin AS date) <= {date_to}
              AND COALESCE(TRY_CAST(cp.ApprocheNationale AS int), 0) = 1
              AND COALESCE(TRY_CAST(cp.ApprocheIAS AS int), 0) = 0
              AND COALESCE(TRY_CAST(cp.typeLot AS int), 1) = 1
            GROUP BY cp.oidcompteGeneral
        ),
        partial_entries AS (
            SELECT e.oidcompteGeneral,
                   SUM(COALESCE(TRY_CAST(e.debit AS decimal(38, 6)), 0)) debit,
                   SUM(COALESCE(TRY_CAST(e.credit AS decimal(38, 6)), 0)) credit
            FROM {entries} e
            WHERE TRY_CAST(e.eDate AS date) BETWEEN {date_from} AND {date_to}
              AND NOT EXISTS (
                  SELECT 1 FROM {periods} covered
                  WHERE CAST(covered.dateDebut AS date) >= {date_from}
                    AND CAST(covered.dateFin AS date) <= {date_to}
                    AND TRY_CAST(e.eDate AS date) BETWEEN CAST(covered.dateDebut AS date) AND CAST(covered.dateFin AS date)
              )
            GROUP BY e.oidcompteGeneral
        ),
        account_base AS (
            SELECT account_no = {account_no},
                   account_label = {account_label},
                   opening_net = CASE WHEN ({account_no} LIKE '6%' OR {account_no} LIKE '7%') THEN CAST(0 AS decimal(38, 6)) ELSE
                       (CASE WHEN availability.has_values = 1
                             THEN COALESCE(can.debit, 0) - COALESCE(can.credit, 0)
                             ELSE COALESCE(pc.debit, 0) - COALESCE(pc.credit, 0) END)
                       + COALESCE(op.debit, 0) - COALESCE(op.credit, 0) END,
                   mvt_debit = COALESCE(mp.debit, 0) + COALESCE(pe.debit, 0),
                   mvt_credit = COALESCE(mp.credit, 0) + COALESCE(pe.credit, 0)
            FROM {accounts} cg
            CROSS JOIN current_an_available availability
            LEFT JOIN current_an can ON can.oidcompteGeneral = cg.oid
            LEFT JOIN previous_close pc ON pc.oidcompteGeneral = cg.oid
            LEFT JOIN opening_periods op ON op.oidcompteGeneral = cg.oid
            LEFT JOIN movement_periods mp ON mp.oidcompteGeneral = cg.oid
            LEFT JOIN partial_entries pe ON pe.oidcompteGeneral = cg.oid
            WHERE COALESCE(TRY_CAST(cg.enActivite AS int), 1) = 1
              AND {account_no} <> '' {account_filter_sql}
        )
        SELECT account_no, account_label,
               ouverture_debit = CASE WHEN opening_net > 0 THEN opening_net ELSE CAST(0 AS decimal(38, 6)) END,
               ouverture_credit = CASE WHEN opening_net < 0 THEN ABS(opening_net) ELSE CAST(0 AS decimal(38, 6)) END,
               mvt_debit, mvt_credit,
               cloture_debit = CASE WHEN opening_net + mvt_debit - mvt_credit > 0 THEN opening_net + mvt_debit - mvt_credit ELSE CAST(0 AS decimal(38, 6)) END,
               cloture_credit = CASE WHEN opening_net + mvt_debit - mvt_credit < 0 THEN ABS(opening_net + mvt_debit - mvt_credit) ELSE CAST(0 AS decimal(38, 6)) END
        FROM account_base
        WHERE opening_net <> 0 OR mvt_debit <> 0 OR mvt_credit <> 0
        ORDER BY account_no
        "#,
        exercises = exercises.sql_name(),
        opening_cumul = opening_cumul.sql_name(),
        period_cumul = period_cumul.sql_name(),
        periods = periods.sql_name(),
        entries = context.entries.sql_name(),
        accounts = context.accounts.sql_name(),
        date_from = sql_literal(&period.date_from),
        date_to = sql_literal(&period.date_to),
        account_no = account_no,
        account_label = text_or_empty(&qualify("cg", "Caption"), 255),
        account_filter_sql = account_filter_sql,
    );

    let data = db::execute_logged_query(
        client,
        &sql,
        db::QueryExecutionContext::new("get_balance", "dashboard_balance_sage1000_cumulative")
            .with_table(period_cumul.sql_name())
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let indexes = column_index_map(&data);
    let rows = data
        .rows
        .iter()
        .map(|row| BalanceRow {
            account_no: row_string(row, &indexes, "account_no").unwrap_or_default(),
            account_label: row_string(row, &indexes, "account_label").unwrap_or_default(),
            ouverture_debit: row_decimal(row, &indexes, "ouverture_debit"),
            ouverture_credit: row_decimal(row, &indexes, "ouverture_credit"),
            mvt_debit: row_decimal(row, &indexes, "mvt_debit"),
            mvt_credit: row_decimal(row, &indexes, "mvt_credit"),
            cloture_debit: row_decimal(row, &indexes, "cloture_debit"),
            cloture_credit: row_decimal(row, &indexes, "cloture_credit"),
        })
        .collect();

    Ok(Some(DashboardResponse { data: rows, warning: None }))
}

async fn get_balance_sage1000(
    client: &mut db::DbClient,
    period: &BalancePeriod,
    account_prefix: Option<&str>,
    timeout_secs: u64,
) -> Result<DashboardResponse<Vec<BalanceRow>>, String> {
    let Some(context) = resolve_sage1000_context(client).await? else {
        return Ok(empty_vec_response(vec![
            "Sage 1000 tables TECRITURE and TCOMPTEGENERAL were not detected".to_string(),
        ]));
    };

    if let Some(result) =
        get_balance_sage1000_cumulative(client, &context, period, account_prefix, timeout_secs)
            .await?
    {
        return Ok(result);
    }

    let account_no_expr = text_or_empty(&qualify("cg", "codeCompte"), 64);
    let account_label_expr = text_or_empty(&qualify("cg", "Caption"), 255);
    let document_sql = sage1000_document_sql(&context);
    let parsed_date_expr = format!("TRY_CAST({} AS DATE)", qualify("e", "eDate"));
    let annual_account = annual_account_condition(&account_no_expr);
    let movement_condition = balance_movement_condition(
        &account_no_expr,
        &parsed_date_expr,
        &document_sql.journal_expr,
        context.pieces.is_some() && context.journals.is_some(),
        period,
    );
    let movement_debit_expr = format!(
        "COALESCE(SUM(CASE WHEN {} THEN {} ELSE CAST(0 AS DECIMAL(38, 6)) END), CAST(0 AS DECIMAL(38, 6)))",
        movement_condition,
        decimal_or_zero(&qualify("e", "debit"))
    );
    let movement_credit_expr = format!(
        "COALESCE(SUM(CASE WHEN {} THEN {} ELSE CAST(0 AS DECIMAL(38, 6)) END), CAST(0 AS DECIMAL(38, 6)))",
        movement_condition,
        decimal_or_zero(&qualify("e", "credit"))
    );
    let opening_net_expr = format!(
        "COALESCE(SUM(CASE WHEN NOT {} AND {} < {} THEN {} - {} ELSE CAST(0 AS DECIMAL(38, 6)) END), CAST(0 AS DECIMAL(38, 6)))",
        annual_account,
        parsed_date_expr,
        sql_literal(&period.date_from),
        decimal_or_zero(&qualify("e", "debit")),
        decimal_or_zero(&qualify("e", "credit"))
    );
    let movement_net_expr = format!("({movement_debit_expr} - {movement_credit_expr})");
    let account_filter_sql = account_prefix
        .filter(|prefix| !prefix.trim().is_empty())
        .map(|prefix| {
            format!(
                " AND {} LIKE {}",
                account_no_expr,
                sql_literal(&format!("{}%", prefix.trim()))
            )
        })
        .unwrap_or_default();

    let sql = format!(
        r#"
        WITH account_base AS (
            SELECT
                account_pk = {account_pk},
                account_no = {account_no_expr},
                account_label = {account_label_expr},
                opening_net = {opening_net_expr},
                mvt_debit = {movement_debit_expr},
                mvt_credit = {movement_credit_expr},
                movement_net = {movement_net_expr}
             FROM {accounts_table} cg
             LEFT JOIN {entries_table} e ON {entry_account_fk} = {account_pk}
             {document_join}
            WHERE COALESCE(TRY_CAST({is_active_col} AS INT), 1) = 1
              AND {account_no_expr} <> ''
              {account_filter_sql}
            GROUP BY
                {account_pk},
                {account_no_col},
                {account_label_col}
        )
        SELECT
            account_no,
            account_label,
            ouverture_debit = CASE WHEN opening_net > 0 THEN opening_net ELSE CAST(0 AS DECIMAL(38, 6)) END,
            ouverture_credit = CASE WHEN opening_net < 0 THEN ABS(opening_net) ELSE CAST(0 AS DECIMAL(38, 6)) END,
            mvt_debit,
            mvt_credit,
            cloture_debit = CASE WHEN opening_net + movement_net > 0 THEN opening_net + movement_net ELSE CAST(0 AS DECIMAL(38, 6)) END,
            cloture_credit = CASE WHEN opening_net + movement_net < 0 THEN ABS(opening_net + movement_net) ELSE CAST(0 AS DECIMAL(38, 6)) END
        FROM account_base
        WHERE opening_net <> 0
           OR mvt_debit <> 0
           OR mvt_credit <> 0
        ORDER BY account_no
        "#,
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        opening_net_expr = opening_net_expr,
        movement_debit_expr = movement_debit_expr,
        movement_credit_expr = movement_credit_expr,
        movement_net_expr = movement_net_expr,
        accounts_table = context.accounts.sql_name(),
        entries_table = context.entries.sql_name(),
        entry_account_fk = qualify("e", "oidcompteGeneral"),
        document_join = document_sql.join_sql,
        account_pk = qualify("cg", "oid"),
        is_active_col = qualify("cg", "enActivite"),
        account_filter_sql = account_filter_sql,
        account_no_col = qualify("cg", "codeCompte"),
        account_label_col = qualify("cg", "Caption")
    );

    let data = db::execute_logged_query(
        client,
        &sql,
        db::QueryExecutionContext::new("get_balance", "dashboard_balance_sage1000")
            .with_table(context.entries.sql_name())
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let indexes = column_index_map(&data);
    let rows = data
        .rows
        .iter()
        .map(|row| BalanceRow {
            account_no: row_string(row, &indexes, "account_no").unwrap_or_default(),
            account_label: row_string(row, &indexes, "account_label").unwrap_or_default(),
            ouverture_debit: row_decimal(row, &indexes, "ouverture_debit"),
            ouverture_credit: row_decimal(row, &indexes, "ouverture_credit"),
            mvt_debit: row_decimal(row, &indexes, "mvt_debit"),
            mvt_credit: row_decimal(row, &indexes, "mvt_credit"),
            cloture_debit: row_decimal(row, &indexes, "cloture_debit"),
            cloture_credit: row_decimal(row, &indexes, "cloture_credit"),
        })
        .collect();

    Ok(DashboardResponse {
        data: rows,
        warning: None,
    })
}

async fn get_grand_livre_auxiliaire_sage1000(
    client: &mut db::DbClient,
    date_from: &str,
    date_to: &str,
    tiers_type: &str,
    account_prefix: Option<&str>,
    timeout_secs: u64,
) -> Result<DashboardResponse<Vec<AuxiliaireRow>>, String> {
    let Some(context) = resolve_sage1000_context(client).await? else {
        return Ok(empty_vec_response(vec![
            "Sage 1000 tables TECRITURE and TCOMPTEGENERAL were not detected".to_string(),
        ]));
    };
    let Some(role_tiers_table) = context.role_tiers.as_ref() else {
        return Ok(empty_vec_response(vec![
            "Sage 1000 table TROLETIERS was not detected".to_string(),
        ]));
    };
    let Some(tiers_table) = context.tiers.as_ref() else {
        return Ok(empty_vec_response(vec![
            "Sage 1000 table TTIERS was not detected".to_string(),
        ]));
    };

    let role_prefix = if tiers_type.eq_ignore_ascii_case("fournisseurs") {
        "40"
    } else {
        "41"
    };
    let document_sql = sage1000_document_sql(&context);
    let tier_oid_expr = if context.pieces.is_some() {
        format!(
            "COALESCE({}, {})",
            qualify("rt", "oidTiers"),
            qualify("p", "oidTiers")
        )
    } else {
        qualify("rt", "oidTiers")
    };
    let account_no_expr = text_or_empty(&qualify("cg", "codeCompte"), 64);
    let account_label_expr = format!(
        "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
        qualify("cg", "Caption"),
        qualify("e", "reference")
    );
    let tiers_code_expr = text_or_empty(&qualify("t", "code"), 64);
    let tiers_name_expr = format!(
        "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
        qualify("t", "raisonSociale"),
        qualify("t", "nom"),
        qualify("t", "code")
    );
    let description_expr = format!(
        "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
        qualify("e", "reference"),
        qualify("e", "Caption")
    );
    let lettrage_expr = text_or_empty(&qualify("e", "CodeLettrageExterne"), 64);
    let debit_expr = decimal_or_zero(&qualify("e", "debit"));
    let credit_expr = decimal_or_zero(&qualify("e", "credit"));
    let role_account_expr = text_or_empty(&qualify("ap", "codeCompte"), 64);
    let tier_kind_filter = format!(
        "(LEFT({role_account_expr}, 2) = {role_prefix} OR LEFT({account_no_expr}, 2) = {role_prefix})",
        role_account_expr = role_account_expr,
        account_no_expr = account_no_expr,
        role_prefix = sql_literal(role_prefix)
    );
    let account_filter_sql = account_prefix
        .filter(|prefix| !prefix.trim().is_empty())
        .map(|prefix| {
            format!(
                " AND {} LIKE {}",
                account_no_expr,
                sql_literal(&format!("{}%", prefix.trim()))
            )
        })
        .unwrap_or_default();

    let sql = format!(
        r#"
        WITH base AS (
            SELECT
                parsed_date = TRY_CAST({date_col} AS DATE),
                account_no = {account_no_expr},
                account_label = {account_label_expr},
                tiers_code = {tiers_code_expr},
                tiers_name = {tiers_name_expr},
                journal = {journal_expr},
                description = {description_expr},
                ref_piece = {ref_piece_expr},
                lettrage = {lettrage_expr},
                debit = {debit_expr},
                credit = {credit_expr},
                entry_number = COALESCE(TRY_CAST({entry_number_col} AS BIGINT), 0)
            FROM {entries_table} e
            LEFT JOIN {accounts_table} cg ON {entry_account_fk} = {account_pk}
            LEFT JOIN {role_tiers_table} rt ON {entry_role_fk} = {role_pk}
            {document_join}
            LEFT JOIN {tiers_table} t ON {tier_oid_expr} = {tiers_pk}
            LEFT JOIN {accounts_table} ap ON {role_account_fk} = {role_account_pk}
            WHERE TRY_CAST({date_col} AS DATE) BETWEEN {date_from} AND {date_to}
              AND {tier_kind_filter}
              AND {tiers_code_expr} <> ''
              AND {account_no_expr} <> ''
              {account_filter_sql}
        ),
        ranked AS (
            SELECT
                account_no,
                account_label,
                tiers_code,
                tiers_name,
                CONVERT(char(10), parsed_date, 103) AS [date],
                journal,
                description,
                ref_piece,
                lettrage,
                debit,
                credit,
                SUM(debit - credit) OVER (
                    PARTITION BY tiers_code
                    ORDER BY account_no, parsed_date, entry_number
                    ROWS UNBOUNDED PRECEDING
                ) AS running_balance,
                ROW_NUMBER() OVER (
                    PARTITION BY tiers_code
                    ORDER BY account_no, parsed_date, entry_number
                ) AS sort_row
            FROM base
            WHERE parsed_date IS NOT NULL
        ),
        totals AS (
            SELECT
                MAX(account_no) AS account_no,
                MAX(account_label) AS account_label,
                tiers_code,
                MAX(tiers_name) AS tiers_name,
                CAST(NULL AS NVARCHAR(10)) AS [date],
                CAST(NULL AS NVARCHAR(64)) AS journal,
                CAST(NULL AS NVARCHAR(255)) AS description,
                CAST(NULL AS NVARCHAR(128)) AS ref_piece,
                CAST(NULL AS NVARCHAR(64)) AS lettrage,
                SUM(debit) AS debit,
                SUM(credit) AS credit,
                SUM(debit - credit) AS running_balance,
                CAST(1 AS BIT) AS is_total_row,
                2147483647 AS sort_row
            FROM base
            WHERE parsed_date IS NOT NULL
            GROUP BY tiers_code
        )
        SELECT
            account_no,
            account_label,
            tiers_code,
            tiers_name,
            [date],
            journal,
            description,
            ref_piece,
            lettrage,
            debit,
            credit,
            running_balance,
            is_total_row
        FROM (
            SELECT
                account_no,
                account_label,
                tiers_code,
                tiers_name,
                [date],
                journal,
                description,
                ref_piece,
                lettrage,
                debit,
                credit,
                running_balance,
                CAST(0 AS BIT) AS is_total_row,
                sort_row
            FROM ranked
            UNION ALL
            SELECT
                account_no,
                account_label,
                tiers_code,
                tiers_name,
                [date],
                journal,
                description,
                ref_piece,
                lettrage,
                debit,
                credit,
                running_balance,
                is_total_row,
                sort_row
            FROM totals
        ) gl_aux
        ORDER BY account_no, tiers_code, sort_row
        "#,
        date_col = qualify("e", "eDate"),
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        tiers_code_expr = tiers_code_expr,
        tiers_name_expr = tiers_name_expr,
        journal_expr = document_sql.journal_expr,
        description_expr = description_expr,
        ref_piece_expr = document_sql.ref_piece_expr,
        lettrage_expr = lettrage_expr,
        debit_expr = debit_expr,
        credit_expr = credit_expr,
        entry_number_col = qualify("e", "numero"),
        entries_table = context.entries.sql_name(),
        accounts_table = context.accounts.sql_name(),
        role_tiers_table = role_tiers_table.sql_name(),
        tiers_table = tiers_table.sql_name(),
        entry_account_fk = qualify("e", "oidcompteGeneral"),
        account_pk = qualify("cg", "oid"),
        entry_role_fk = qualify("e", "oidroleTiers"),
        role_pk = qualify("rt", "oid"),
        tier_oid_expr = tier_oid_expr,
        tiers_pk = qualify("t", "oid"),
        role_account_fk = qualify("rt", "oidcomptePrivilegie"),
        role_account_pk = qualify("ap", "oid"),
        document_join = document_sql.join_sql,
        date_from = sql_literal(date_from),
        date_to = sql_literal(date_to),
        tier_kind_filter = tier_kind_filter,
        account_filter_sql = account_filter_sql
    );

    let data = db::execute_logged_query(
        client,
        &sql,
        db::QueryExecutionContext::new(
            "get_grand_livre_auxiliaire",
            "dashboard_grand_livre_auxiliaire_sage1000",
        )
        .with_table(context.entries.sql_name())
        .with_timeout_secs(timeout_secs),
    )
    .await?;
    let indexes = column_index_map(&data);
    let rows = data
        .rows
        .iter()
        .map(|row| AuxiliaireRow {
            account_no: row_string(row, &indexes, "account_no").unwrap_or_default(),
            account_label: row_string(row, &indexes, "account_label").unwrap_or_default(),
            tiers_code: row_string(row, &indexes, "tiers_code").unwrap_or_default(),
            tiers_name: row_string(row, &indexes, "tiers_name").unwrap_or_default(),
            date: row_string(row, &indexes, "date"),
            journal: row_string(row, &indexes, "journal"),
            description: row_string(row, &indexes, "description"),
            ref_piece: row_string(row, &indexes, "ref_piece"),
            lettrage: row_string(row, &indexes, "lettrage"),
            debit: row_decimal(row, &indexes, "debit"),
            credit: row_decimal(row, &indexes, "credit"),
            running_balance: row_decimal(row, &indexes, "running_balance"),
            is_total_row: row_bool(row, &indexes, "is_total_row"),
        })
        .collect();

    Ok(DashboardResponse {
        data: rows,
        warning: None,
    })
}

async fn get_dashboard_kpis_sage1000(
    client: &mut db::DbClient,
    date_from: &str,
    date_to: &str,
    account_prefix: Option<&str>,
    timeout_secs: u64,
) -> Result<DashboardResponse<DashboardKpis>, String> {
    let Some(context) = resolve_sage1000_context(client).await? else {
        return Ok(empty_kpi_response(vec![
            "Sage 1000 tables TECRITURE and TCOMPTEGENERAL were not detected".to_string(),
        ]));
    };

    let piece_join = sage1000_piece_join(&context);
    let tiers_join = match (context.role_tiers.as_ref(), context.tiers.as_ref()) {
        (Some(role_tiers), Some(tiers)) => {
            let tier_oid_expr = if context.pieces.is_some() {
                format!(
                    "COALESCE({}, {})",
                    qualify("rt", "oidTiers"),
                    qualify("p", "oidTiers")
                )
            } else {
                qualify("rt", "oidTiers")
            };
            format!(
                "{piece_join}\n            LEFT JOIN {} rt ON {} = {}\n            LEFT JOIN {} t ON {} = {}",
                role_tiers.sql_name(),
                qualify("e", "oidroleTiers"),
                qualify("rt", "oid"),
                tiers.sql_name(),
                tier_oid_expr,
                qualify("t", "oid"),
                piece_join = piece_join
            )
        }
        (None, Some(tiers)) if context.pieces.is_some() => format!(
            "{piece_join}\n            LEFT JOIN {} t ON {} = {}",
            tiers.sql_name(),
            qualify("p", "oidTiers"),
            qualify("t", "oid"),
            piece_join = piece_join
        ),
        _ if !piece_join.is_empty() => piece_join,
        _ => String::new(),
    };
    let tiers_code_expr = if context.tiers.is_some() {
        text_or_empty(&qualify("t", "code"), 64)
    } else {
        "CAST('' AS NVARCHAR(64))".to_string()
    };
    let account_no_expr = text_or_empty(&qualify("cg", "codeCompte"), 64);
    let account_label_expr = format!(
        "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
        qualify("cg", "Caption"),
        qualify("e", "reference")
    );
    let base_cte = format!(
        r#"
        WITH base AS (
            SELECT
                parsed_date = TRY_CAST({date_col} AS DATE),
                account_no = {account_no_expr},
                account_label = {account_label_expr},
                tiers_code = {tiers_code_expr},
                debit = {debit_expr},
                credit = {credit_expr}
            FROM {entries_table} e
            LEFT JOIN {accounts_table} cg ON {entry_account_fk} = {account_pk}
            {tiers_join}
        )
        "#,
        date_col = qualify("e", "eDate"),
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        tiers_code_expr = tiers_code_expr,
        debit_expr = decimal_or_zero(&qualify("e", "debit")),
        credit_expr = decimal_or_zero(&qualify("e", "credit")),
        entries_table = context.entries.sql_name(),
        accounts_table = context.accounts.sql_name(),
        entry_account_fk = qualify("e", "oidcompteGeneral"),
        account_pk = qualify("cg", "oid"),
        tiers_join = tiers_join
    );
    let account_filter_sql = account_prefix
        .filter(|prefix| !prefix.trim().is_empty())
        .map(|prefix| {
            format!(
                " AND account_no LIKE {}",
                sql_literal(&format!("{}%", prefix.trim()))
            )
        })
        .unwrap_or_default();

    let summary_sql = format!(
        r#"
        {base_cte}
        SELECT
            SUM(CASE WHEN parsed_date BETWEEN {date_from} AND {date_to} AND account_no LIKE '7%' THEN credit ELSE CAST(0 AS DECIMAL(38, 6)) END) AS total_ventes,
            SUM(CASE WHEN parsed_date BETWEEN {date_from} AND {date_to} AND account_no LIKE '6%' THEN debit ELSE CAST(0 AS DECIMAL(38, 6)) END) AS total_achats,
            SUM(CASE WHEN parsed_date <= {date_to} AND account_no LIKE '41%' THEN (debit - credit) ELSE CAST(0 AS DECIMAL(38, 6)) END) AS solde_clients,
            SUM(CASE WHEN parsed_date <= {date_to} AND account_no LIKE '40%' THEN (credit - debit) ELSE CAST(0 AS DECIMAL(38, 6)) END) AS solde_fournisseurs,
            SUM(CASE WHEN parsed_date BETWEEN {date_from} AND {date_to} THEN 1 ELSE 0 END) AS nb_ecritures,
            COUNT(DISTINCT CASE WHEN parsed_date BETWEEN {date_from} AND {date_to} THEN NULLIF(tiers_code, '') END) AS nb_tiers_actifs
        FROM base
        WHERE parsed_date IS NOT NULL
          AND account_no <> ''
          {account_filter_sql}
        "#,
        base_cte = base_cte,
        date_from = sql_literal(date_from),
        date_to = sql_literal(date_to),
        account_filter_sql = account_filter_sql
    );

    let top_charges_sql = format!(
        r#"
        {base_cte}
        SELECT TOP 5
            account_no AS account,
            MAX(account_label) AS label,
            SUM(debit) AS amount
        FROM base
        WHERE parsed_date BETWEEN {date_from} AND {date_to}
          AND account_no LIKE '6%'
          {account_filter_sql}
        GROUP BY account_no
        ORDER BY amount DESC, account_no
        "#,
        base_cte = base_cte,
        date_from = sql_literal(date_from),
        date_to = sql_literal(date_to),
        account_filter_sql = account_filter_sql
    );

    let top_produits_sql = format!(
        r#"
        {base_cte}
        SELECT TOP 5
            account_no AS account,
            MAX(account_label) AS label,
            SUM(credit) AS amount
        FROM base
        WHERE parsed_date BETWEEN {date_from} AND {date_to}
          AND account_no LIKE '7%'
          {account_filter_sql}
        GROUP BY account_no
        ORDER BY amount DESC, account_no
        "#,
        base_cte = base_cte,
        date_from = sql_literal(date_from),
        date_to = sql_literal(date_to),
        account_filter_sql = account_filter_sql
    );

    let evolution_sql = format!(
        r#"
        {base_cte}
        SELECT
            CONVERT(char(7), parsed_date, 120) AS [month],
            SUM(CASE WHEN account_no LIKE '7%' THEN credit ELSE CAST(0 AS DECIMAL(38, 6)) END) AS ventes,
            SUM(CASE WHEN account_no LIKE '6%' THEN debit ELSE CAST(0 AS DECIMAL(38, 6)) END) AS achats
        FROM base
        WHERE parsed_date BETWEEN {date_from} AND {date_to}
          AND account_no <> ''
          {account_filter_sql}
        GROUP BY CONVERT(char(7), parsed_date, 120)
        ORDER BY [month]
        "#,
        base_cte = base_cte,
        date_from = sql_literal(date_from),
        date_to = sql_literal(date_to),
        account_filter_sql = account_filter_sql
    );

    let summary = db::execute_logged_query(
        client,
        &summary_sql,
        db::QueryExecutionContext::new("get_dashboard_kpis", "dashboard_kpi_summary_sage1000")
            .with_table(context.entries.sql_name())
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let top_charges = db::execute_logged_query(
        client,
        &top_charges_sql,
        db::QueryExecutionContext::new("get_dashboard_kpis", "dashboard_kpi_top_charges_sage1000")
            .with_table(context.entries.sql_name())
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let top_produits = db::execute_logged_query(
        client,
        &top_produits_sql,
        db::QueryExecutionContext::new("get_dashboard_kpis", "dashboard_kpi_top_products_sage1000")
            .with_table(context.entries.sql_name())
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let evolution = db::execute_logged_query(
        client,
        &evolution_sql,
        db::QueryExecutionContext::new("get_dashboard_kpis", "dashboard_kpi_evolution_sage1000")
            .with_table(context.entries.sql_name())
            .with_timeout_secs(timeout_secs),
    )
    .await?;

    let summary_indexes = column_index_map(&summary);
    let top_charges_indexes = column_index_map(&top_charges);
    let top_produits_indexes = column_index_map(&top_produits);
    let evolution_indexes = column_index_map(&evolution);

    let summary_row = summary.rows.first();
    let kpis = DashboardKpis {
        total_ventes: summary_row
            .map(|row| row_decimal(row, &summary_indexes, "total_ventes"))
            .unwrap_or(Decimal::ZERO),
        total_achats: summary_row
            .map(|row| row_decimal(row, &summary_indexes, "total_achats"))
            .unwrap_or(Decimal::ZERO),
        solde_clients: summary_row
            .map(|row| row_decimal(row, &summary_indexes, "solde_clients"))
            .unwrap_or(Decimal::ZERO),
        solde_fournisseurs: summary_row
            .map(|row| row_decimal(row, &summary_indexes, "solde_fournisseurs"))
            .unwrap_or(Decimal::ZERO),
        nb_ecritures: summary_row
            .map(|row| row_i64(row, &summary_indexes, "nb_ecritures"))
            .unwrap_or(0),
        nb_tiers_actifs: summary_row
            .map(|row| row_i64(row, &summary_indexes, "nb_tiers_actifs"))
            .unwrap_or(0),
        top_charges: top_charges
            .rows
            .iter()
            .map(|row| DashboardTopItem {
                account: row_string(row, &top_charges_indexes, "account").unwrap_or_default(),
                label: row_string(row, &top_charges_indexes, "label").unwrap_or_default(),
                amount: row_decimal(row, &top_charges_indexes, "amount"),
            })
            .collect(),
        top_produits: top_produits
            .rows
            .iter()
            .map(|row| DashboardTopItem {
                account: row_string(row, &top_produits_indexes, "account").unwrap_or_default(),
                label: row_string(row, &top_produits_indexes, "label").unwrap_or_default(),
                amount: row_decimal(row, &top_produits_indexes, "amount"),
            })
            .collect(),
        evolution_mensuelle: evolution
            .rows
            .iter()
            .map(|row| MonthlyEvolutionPoint {
                month: row_string(row, &evolution_indexes, "month").unwrap_or_default(),
                ventes: row_decimal(row, &evolution_indexes, "ventes"),
                achats: row_decimal(row, &evolution_indexes, "achats"),
            })
            .collect(),
    };

    Ok(DashboardResponse {
        data: kpis,
        warning: None,
    })
}

#[tauri::command]
pub async fn get_grand_livre(
    state: State<'_, AppState>,
    id: String,
    date_from: String,
    date_to: String,
    account_prefix: Option<String>,
) -> Result<DashboardResponse<Vec<GrandLivreRow>>, String> {
    let config = load_connection_config(&state, &id)?;
    let hint_schema = state.get_connection_schema(&id);
    let timeout_secs = dashboard_timeout_secs(state.inner());
    let mut client = get_active_client(state.inner(), &id).await?;
    if should_use_sage1000_analytics(&mut client, hint_schema.as_ref(), "get_grand_livre").await? {
        return get_grand_livre_sage1000(
            &mut client,
            &date_from,
            &date_to,
            account_prefix.as_deref(),
            timeout_secs,
        )
        .await;
    }
    let (entry_context, mut warnings) =
        resolve_entry_context_smart(&mut client, hint_schema.as_ref()).await?;

    let Some(entry_context) = entry_context else {
        return Ok(empty_vec_response(warnings));
    };

    let account_lookup = if let Some(ref schema) = hint_schema {
        if !schema.edition.is_generic() {
            schema_lookup_context(
                &mut client,
                &schema.table_comptes,
                &schema.col_compte_num,
                &schema.col_compte_lib,
                &schema.col_compte_type,
            )
            .await?
            .or(resolve_lookup_context(
                &mut client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?)
        } else {
            resolve_lookup_context(
                &mut client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?
        }
    } else {
        resolve_lookup_context(
            &mut client,
            ACCOUNT_TABLE_CANDIDATES,
            &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
            &[
                "CG_Intitule",
                "CT_Intitule",
                "LIBELLE_COMPTE",
                "ACCOUNT_LABEL",
                "ACCOUNT_NAME",
            ],
            &[],
        )
        .await?
    };

    if account_lookup.is_none() && entry_context.account_label_col.is_none() {
        warnings.push(
            "Account labels table not detected; labels will be blank when not present on entries"
                .to_string(),
        );
    }

    let parsed_date_expr = format!(
        "TRY_CAST({} AS DATE)",
        qualify("e", &entry_context.date_col)
    );
    let journal_expr = entry_context
        .journal_col
        .as_ref()
        .map(|column| text_or_empty(&qualify("e", column), 64))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(64))".to_string());
    let account_no_expr = text_or_empty(&qualify("e", &entry_context.account_col), 64);
    let ref_piece_expr = entry_context
        .ref_piece_col
        .as_ref()
        .map(|column| text_or_empty(&qualify("e", column), 128))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(128))".to_string());
    let lettrage_expr = entry_context
        .lettrage_col
        .as_ref()
        .map(|column| text_or_empty(&qualify("e", column), 64))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(64))".to_string());
    let account_label_expr = match (&account_lookup, &entry_context.account_label_col) {
        (Some(lookup), Some(entry_label)) => format!(
            "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
            qualify("a", &lookup.label_col),
            qualify("e", entry_label)
        ),
        (Some(lookup), None) => text_or_empty(&qualify("a", &lookup.label_col), 255),
        (None, Some(entry_label)) => text_or_empty(&qualify("e", entry_label), 255),
        (None, None) => "CAST('' AS NVARCHAR(255))".to_string(),
    };
    let account_join = account_lookup
        .as_ref()
        .map(|lookup| {
            format!(
                "LEFT JOIN {} a ON {} = {}",
                lookup.table.sql_name(),
                qualify("a", &lookup.code_col),
                qualify("e", &entry_context.account_col)
            )
        })
        .unwrap_or_default();
    let account_filter_sql = account_prefix
        .as_ref()
        .filter(|prefix| !prefix.trim().is_empty())
        .map(|prefix| {
            format!(
                " AND {} LIKE {}",
                account_no_expr,
                sql_literal(&format!("{}%", prefix.trim()))
            )
        })
        .unwrap_or_default();

    let sql = format!(
        r#"
        WITH base AS (
            SELECT
                parsed_date = {parsed_date_expr},
                journal = {journal_expr},
                account_no = {account_no_expr},
                account_label = {account_label_expr},
                ref_piece = {ref_piece_expr},
                lettrage = {lettrage_expr},
                debit = {debit_expr},
                credit = {credit_expr}
            FROM {entries_table} e
            {account_join}
            WHERE {parsed_date_expr} BETWEEN {date_from} AND {date_to}
              {account_filter_sql}
        ),
        ranked AS (
            SELECT
                CONVERT(char(10), parsed_date, 103) AS [date],
                journal,
                account_no,
                account_label,
                ref_piece,
                lettrage,
                debit,
                credit,
                SUM(debit - credit) OVER (
                    PARTITION BY account_no
                    ORDER BY parsed_date, journal, ref_piece, lettrage, account_label
                    ROWS UNBOUNDED PRECEDING
                ) AS solde_cumule,
                ROW_NUMBER() OVER (
                    PARTITION BY account_no
                    ORDER BY parsed_date, journal, ref_piece, lettrage, account_label
                ) AS sort_row
            FROM base
            WHERE parsed_date IS NOT NULL
              AND account_no <> ''
        ),
        subtotal AS (
            SELECT
                CAST(NULL AS NVARCHAR(10)) AS [date],
                CAST(NULL AS NVARCHAR(64)) AS journal,
                account_no,
                MAX(account_label) AS account_label,
                CAST(NULL AS NVARCHAR(128)) AS ref_piece,
                CAST(NULL AS NVARCHAR(64)) AS lettrage,
                CAST(0 AS DECIMAL(38, 6)) AS debit,
                CAST(0 AS DECIMAL(38, 6)) AS credit,
                SUM(debit - credit) AS solde_cumule,
                CAST(1 AS BIT) AS is_subtotal_row,
                SUM(debit) AS total_debit,
                SUM(credit) AS total_credit,
                SUM(debit - credit) AS solde,
                2147483647 AS sort_row
            FROM base
            WHERE parsed_date IS NOT NULL
              AND account_no <> ''
            GROUP BY account_no
        )
        SELECT
            [date],
            journal,
            account_no,
            account_label,
            ref_piece,
            lettrage,
            debit,
            credit,
            solde_cumule,
            is_subtotal_row,
            total_debit,
            total_credit,
            solde
        FROM (
            SELECT
                [date],
                journal,
                account_no,
                account_label,
                ref_piece,
                lettrage,
                debit,
                credit,
                solde_cumule,
                CAST(0 AS BIT) AS is_subtotal_row,
                CAST(NULL AS DECIMAL(38, 6)) AS total_debit,
                CAST(NULL AS DECIMAL(38, 6)) AS total_credit,
                CAST(NULL AS DECIMAL(38, 6)) AS solde,
                sort_row
            FROM ranked
            UNION ALL
            SELECT
                [date],
                journal,
                account_no,
                account_label,
                ref_piece,
                lettrage,
                debit,
                credit,
                solde_cumule,
                is_subtotal_row,
                total_debit,
                total_credit,
                solde,
                sort_row
            FROM subtotal
        ) ledger
        ORDER BY account_no, sort_row
        "#,
        parsed_date_expr = parsed_date_expr,
        journal_expr = journal_expr,
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        ref_piece_expr = ref_piece_expr,
        lettrage_expr = lettrage_expr,
        debit_expr = entry_context.debit_expr,
        credit_expr = entry_context.credit_expr,
        entries_table = entry_context.table.sql_name(),
        account_join = account_join,
        date_from = sql_literal(&date_from),
        date_to = sql_literal(&date_to),
        account_filter_sql = account_filter_sql
    );

    let data = db::execute_logged_query(
        &mut client,
        &sql,
        db::QueryExecutionContext::new("get_grand_livre", "dashboard_grand_livre")
            .with_connection_id(id.clone())
            .with_database(config.database.clone())
            .with_table(entry_context.table.sql_name())
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let indexes = column_index_map(&data);
    let rows = data
        .rows
        .iter()
        .map(|row| {
            let is_subtotal = row_bool(row, &indexes, "is_subtotal_row");
            GrandLivreRow {
                row_type: if is_subtotal {
                    "subtotal".to_string()
                } else {
                    "entry".to_string()
                },
                date: row_string(row, &indexes, "date"),
                journal: row_string(row, &indexes, "journal"),
                account_no: row_string(row, &indexes, "account_no").unwrap_or_default(),
                account_label: row_string(row, &indexes, "account_label").unwrap_or_default(),
                ref_piece: row_string(row, &indexes, "ref_piece"),
                lettrage: row_string(row, &indexes, "lettrage"),
                debit: row_decimal(row, &indexes, "debit"),
                credit: row_decimal(row, &indexes, "credit"),
                solde_cumule: row_decimal(row, &indexes, "solde_cumule"),
                total_debit: indexes
                    .contains_key("total_debit")
                    .then(|| row_decimal(row, &indexes, "total_debit"))
                    .filter(|value| is_subtotal || *value != Decimal::ZERO),
                total_credit: indexes
                    .contains_key("total_credit")
                    .then(|| row_decimal(row, &indexes, "total_credit"))
                    .filter(|value| is_subtotal || *value != Decimal::ZERO),
                solde: indexes
                    .contains_key("solde")
                    .then(|| row_decimal(row, &indexes, "solde"))
                    .filter(|value| is_subtotal || *value != Decimal::ZERO),
            }
        })
        .collect();

    Ok(DashboardResponse {
        data: rows,
        warning: join_warnings(warnings),
    })
}

#[tauri::command]
pub async fn get_balance(
    state: State<'_, AppState>,
    id: String,
    date_from: String,
    date_to: String,
    account_prefix: Option<String>,
) -> Result<DashboardResponse<Vec<BalanceRow>>, String> {
    let period = resolve_balance_period(&date_from, &date_to)?;
    let config = load_connection_config(&state, &id)?;
    let hint_schema = state.get_connection_schema(&id);
    let timeout_secs = dashboard_timeout_secs(state.inner());
    let mut client = get_active_client(state.inner(), &id).await?;
    if should_use_sage1000_analytics(&mut client, hint_schema.as_ref(), "get_balance").await? {
        return get_balance_sage1000(
            &mut client,
            &period,
            account_prefix.as_deref(),
            timeout_secs,
        )
        .await;
    }
    let (entry_context, mut warnings) =
        resolve_entry_context_smart(&mut client, hint_schema.as_ref()).await?;

    let Some(entry_context) = entry_context else {
        return Ok(empty_vec_response(warnings));
    };

    let account_lookup = if let Some(ref schema) = hint_schema {
        if !schema.edition.is_generic() {
            schema_lookup_context(
                &mut client,
                &schema.table_comptes,
                &schema.col_compte_num,
                &schema.col_compte_lib,
                &schema.col_compte_type,
            )
            .await?
            .or(resolve_lookup_context(
                &mut client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?)
        } else {
            resolve_lookup_context(
                &mut client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?
        }
    } else {
        resolve_lookup_context(
            &mut client,
            ACCOUNT_TABLE_CANDIDATES,
            &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
            &[
                "CG_Intitule",
                "CT_Intitule",
                "LIBELLE_COMPTE",
                "ACCOUNT_LABEL",
                "ACCOUNT_NAME",
            ],
            &[],
        )
        .await?
    };

    if entry_context.journal_col.is_none() {
        warnings.push("Journal column not detected; opening balance falls back to entries before the start date only".to_string());
    }

    let parsed_date_expr = format!(
        "TRY_CAST({} AS DATE)",
        qualify("e", &entry_context.date_col)
    );
    let journal_expr = entry_context
        .journal_col
        .as_ref()
        .map(|column| text_or_empty(&qualify("e", column), 64))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(64))".to_string());
    let account_no_expr = text_or_empty(&qualify("e", &entry_context.account_col), 64);
    let account_label_expr = match (&account_lookup, &entry_context.account_label_col) {
        (Some(lookup), Some(entry_label)) => format!(
            "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
            qualify("a", &lookup.label_col),
            qualify("e", entry_label)
        ),
        (Some(lookup), None) => text_or_empty(&qualify("a", &lookup.label_col), 255),
        (None, Some(entry_label)) => text_or_empty(&qualify("e", entry_label), 255),
        (None, None) => "CAST('' AS NVARCHAR(255))".to_string(),
    };
    let account_join = account_lookup
        .as_ref()
        .map(|lookup| {
            format!(
                "LEFT JOIN {} a ON {} = {}",
                lookup.table.sql_name(),
                qualify("a", &lookup.code_col),
                qualify("e", &entry_context.account_col)
            )
        })
        .unwrap_or_default();
    let account_filter_sql = account_prefix
        .as_ref()
        .filter(|prefix| !prefix.trim().is_empty())
        .map(|prefix| {
            format!(
                " AND {} LIKE {}",
                account_no_expr,
                sql_literal(&format!("{}%", prefix.trim()))
            )
        })
        .unwrap_or_default();
    let base_opening_condition = if entry_context.journal_col.is_some() {
        format!(
            "(journal IN ('RAN', 'AN', 'OUV') OR parsed_date < {})",
            sql_literal(&period.date_from)
        )
    } else {
        format!("parsed_date < {}", sql_literal(&period.date_from))
    };
    let opening_condition = format!(
        "NOT {} AND {}",
        annual_account_condition("account_no"),
        base_opening_condition
    );
    let movement_condition = balance_movement_condition(
        "account_no",
        "parsed_date",
        "journal",
        entry_context.journal_col.is_some(),
        &period,
    );
    let cutoff_condition = if entry_context.journal_col.is_some() {
        format!(
            "(parsed_date <= {} OR journal IN ('RAN', 'AN', 'OUV'))",
            sql_literal(&period.date_to)
        )
    } else {
        format!("parsed_date <= {}", sql_literal(&period.date_to))
    };

    let sql = format!(
        r#"
        WITH base AS (
            SELECT
                parsed_date = {parsed_date_expr},
                journal = {journal_expr},
                account_no = {account_no_expr},
                account_label = {account_label_expr},
                debit = {debit_expr},
                credit = {credit_expr}
            FROM {entries_table} e
            {account_join}
            WHERE {account_filter_sql_base}
        ),
        aggregated AS (
            SELECT
                account_no,
                MAX(account_label) AS account_label,
                SUM(CASE WHEN {opening_condition} THEN debit ELSE CAST(0 AS DECIMAL(38, 6)) END) AS open_debit_raw,
                SUM(CASE WHEN {opening_condition} THEN credit ELSE CAST(0 AS DECIMAL(38, 6)) END) AS open_credit_raw,
                SUM(CASE WHEN {movement_condition} THEN debit ELSE CAST(0 AS DECIMAL(38, 6)) END) AS mvt_debit,
                SUM(CASE WHEN {movement_condition} THEN credit ELSE CAST(0 AS DECIMAL(38, 6)) END) AS mvt_credit
            FROM base
            WHERE account_no <> ''
              AND parsed_date IS NOT NULL
              AND {cutoff_condition}
            GROUP BY account_no
        )
        SELECT
            account_no,
            account_label,
            CASE WHEN (open_debit_raw - open_credit_raw) >= 0 THEN (open_debit_raw - open_credit_raw) ELSE CAST(0 AS DECIMAL(38, 6)) END AS ouverture_debit,
            CASE WHEN (open_debit_raw - open_credit_raw) < 0 THEN ABS(open_debit_raw - open_credit_raw) ELSE CAST(0 AS DECIMAL(38, 6)) END AS ouverture_credit,
            mvt_debit,
            mvt_credit,
            CASE
                WHEN ((open_debit_raw - open_credit_raw) + (mvt_debit - mvt_credit)) >= 0
                    THEN ((open_debit_raw - open_credit_raw) + (mvt_debit - mvt_credit))
                ELSE CAST(0 AS DECIMAL(38, 6))
            END AS cloture_debit,
            CASE
                WHEN ((open_debit_raw - open_credit_raw) + (mvt_debit - mvt_credit)) < 0
                    THEN ABS((open_debit_raw - open_credit_raw) + (mvt_debit - mvt_credit))
                ELSE CAST(0 AS DECIMAL(38, 6))
            END AS cloture_credit
        FROM aggregated
        ORDER BY account_no
        "#,
        parsed_date_expr = parsed_date_expr,
        journal_expr = journal_expr,
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        debit_expr = entry_context.debit_expr,
        credit_expr = entry_context.credit_expr,
        entries_table = entry_context.table.sql_name(),
        account_join = account_join,
        account_filter_sql_base = format!("1 = 1{}", account_filter_sql),
        opening_condition = opening_condition,
        movement_condition = movement_condition,
        cutoff_condition = cutoff_condition
    );

    let data = db::execute_logged_query(
        &mut client,
        &sql,
        db::QueryExecutionContext::new("get_balance", "dashboard_balance")
            .with_connection_id(id.clone())
            .with_database(config.database.clone())
            .with_table(entry_context.table.sql_name())
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let indexes = column_index_map(&data);
    let rows = data
        .rows
        .iter()
        .map(|row| BalanceRow {
            account_no: row_string(row, &indexes, "account_no").unwrap_or_default(),
            account_label: row_string(row, &indexes, "account_label").unwrap_or_default(),
            ouverture_debit: row_decimal(row, &indexes, "ouverture_debit"),
            ouverture_credit: row_decimal(row, &indexes, "ouverture_credit"),
            mvt_debit: row_decimal(row, &indexes, "mvt_debit"),
            mvt_credit: row_decimal(row, &indexes, "mvt_credit"),
            cloture_debit: row_decimal(row, &indexes, "cloture_debit"),
            cloture_credit: row_decimal(row, &indexes, "cloture_credit"),
        })
        .collect();

    Ok(DashboardResponse {
        data: rows,
        warning: join_warnings(warnings),
    })
}

/// Aggregate Sage movements once per account and calendar month for the operating
/// statement. The Sage database remains strictly read-only; classification and
/// statement formulas are applied in the application layer using local mappings.
#[tauri::command]
pub async fn get_compte_exploitation_accounts(
    state: State<'_, AppState>,
    id: String,
    date_from: String,
    date_to: String,
) -> Result<DashboardResponse<Vec<ExploitationAccountMonthRow>>, String> {
    let config = load_connection_config(&state, &id)?;
    let hint_schema = state.get_connection_schema(&id);
    let timeout_secs = dashboard_timeout_secs(state.inner());
    let mut client = get_active_client(state.inner(), &id).await?;

    let (sql, table_name, warnings) = if should_use_sage1000_analytics(
        &mut client,
        hint_schema.as_ref(),
        "get_compte_exploitation_accounts",
    )
    .await?
    {
        let Some(context) = resolve_sage1000_context(&mut client).await? else {
            return Ok(empty_vec_response(vec![
                "Sage 1000 accounting tables were not detected".to_string(),
            ]));
        };
        let table_name = context.entries.sql_name();
        let sql = format!(
            r#"
            WITH base AS (
                SELECT
                    parsed_date = TRY_CAST({date_col} AS DATE),
                    account_no = {account_no},
                    account_label = {account_label},
                    debit = {debit},
                    credit = {credit}
                FROM {entries} e
                LEFT JOIN {accounts} cg ON {entry_account_fk} = {account_pk}
                WHERE TRY_CAST({date_col} AS DATE) BETWEEN {date_from} AND {date_to}
            )
            SELECT
                account_no,
                MAX(account_label) AS account_label,
                MONTH(parsed_date) AS [month],
                SUM(debit) AS debit,
                SUM(credit) AS credit
            FROM base
            WHERE parsed_date IS NOT NULL AND account_no <> ''
            GROUP BY account_no, MONTH(parsed_date)
            ORDER BY account_no, [month]
            "#,
            date_col = qualify("e", "eDate"),
            account_no = text_or_empty(&qualify("cg", "codeCompte"), 64),
            account_label = text_or_empty(&qualify("cg", "Caption"), 255),
            debit = decimal_or_zero(&qualify("e", "debit")),
            credit = decimal_or_zero(&qualify("e", "credit")),
            entries = context.entries.sql_name(),
            accounts = context.accounts.sql_name(),
            entry_account_fk = qualify("e", "oidcompteGeneral"),
            account_pk = qualify("cg", "oid"),
            date_from = sql_literal(&date_from),
            date_to = sql_literal(&date_to),
        );
        (sql, table_name, Vec::new())
    } else {
        let (entry_context, warnings) =
            resolve_entry_context_smart(&mut client, hint_schema.as_ref()).await?;
        let Some(entry_context) = entry_context else {
            return Ok(empty_vec_response(warnings));
        };
        let account_lookup = if let Some(ref schema) = hint_schema {
            if !schema.edition.is_generic() {
                schema_lookup_context(
                    &mut client,
                    &schema.table_comptes,
                    &schema.col_compte_num,
                    &schema.col_compte_lib,
                    &schema.col_compte_type,
                )
                .await?
                .or(resolve_lookup_context(
                    &mut client,
                    ACCOUNT_TABLE_CANDIDATES,
                    &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                    &[
                        "CG_Intitule",
                        "CT_Intitule",
                        "LIBELLE_COMPTE",
                        "ACCOUNT_LABEL",
                        "ACCOUNT_NAME",
                    ],
                    &[],
                )
                .await?)
            } else {
                resolve_lookup_context(
                    &mut client,
                    ACCOUNT_TABLE_CANDIDATES,
                    &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                    &[
                        "CG_Intitule",
                        "CT_Intitule",
                        "LIBELLE_COMPTE",
                        "ACCOUNT_LABEL",
                        "ACCOUNT_NAME",
                    ],
                    &[],
                )
                .await?
            }
        } else {
            resolve_lookup_context(
                &mut client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?
        };
        let parsed_date = format!(
            "TRY_CAST({} AS DATE)",
            qualify("e", &entry_context.date_col)
        );
        let account_no = text_or_empty(&qualify("e", &entry_context.account_col), 64);
        let account_label = match (&account_lookup, &entry_context.account_label_col) {
            (Some(lookup), Some(entry_label)) => format!(
                "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
                qualify("a", &lookup.label_col), qualify("e", entry_label)
            ),
            (Some(lookup), None) => text_or_empty(&qualify("a", &lookup.label_col), 255),
            (None, Some(entry_label)) => text_or_empty(&qualify("e", entry_label), 255),
            (None, None) => "CAST('' AS NVARCHAR(255))".to_string(),
        };
        let account_join = account_lookup
            .as_ref()
            .map(|lookup| {
                format!(
                    "LEFT JOIN {} a ON {} = {}",
                    lookup.table.sql_name(),
                    qualify("a", &lookup.code_col),
                    qualify("e", &entry_context.account_col)
                )
            })
            .unwrap_or_default();
        let table_name = entry_context.table.sql_name();
        let sql = format!(
            r#"
            WITH base AS (
                SELECT
                    parsed_date = {parsed_date},
                    account_no = {account_no},
                    account_label = {account_label},
                    debit = {debit},
                    credit = {credit}
                FROM {entries} e
                {account_join}
                WHERE {parsed_date} BETWEEN {date_from} AND {date_to}
            )
            SELECT
                account_no,
                MAX(account_label) AS account_label,
                MONTH(parsed_date) AS [month],
                SUM(debit) AS debit,
                SUM(credit) AS credit
            FROM base
            WHERE parsed_date IS NOT NULL AND account_no <> ''
            GROUP BY account_no, MONTH(parsed_date)
            ORDER BY account_no, [month]
            "#,
            parsed_date = parsed_date,
            account_no = account_no,
            account_label = account_label,
            debit = entry_context.debit_expr,
            credit = entry_context.credit_expr,
            entries = entry_context.table.sql_name(),
            account_join = account_join,
            date_from = sql_literal(&date_from),
            date_to = sql_literal(&date_to),
        );
        (sql, table_name, warnings)
    };

    let data = db::execute_logged_query(
        &mut client,
        &sql,
        db::QueryExecutionContext::new(
            "get_compte_exploitation_accounts",
            "compte_exploitation_monthly_accounts",
        )
        .with_connection_id(id)
        .with_database(config.database)
        .with_table(table_name)
        .with_timeout_secs(timeout_secs),
    )
    .await?;
    let indexes = column_index_map(&data);
    let rows = data
        .rows
        .iter()
        .map(|row| ExploitationAccountMonthRow {
            account_no: row_string(row, &indexes, "account_no").unwrap_or_default(),
            account_label: row_string(row, &indexes, "account_label").unwrap_or_default(),
            month: row_i64(row, &indexes, "month").clamp(1, 12) as u32,
            debit: row_decimal(row, &indexes, "debit"),
            credit: row_decimal(row, &indexes, "credit"),
        })
        .collect();

    Ok(DashboardResponse {
        data: rows,
        warning: join_warnings(warnings),
    })
}

#[tauri::command]
pub async fn get_grand_livre_auxiliaire(
    state: State<'_, AppState>,
    id: String,
    date_from: String,
    date_to: String,
    tiers_type: String,
    account_prefix: Option<String>,
) -> Result<DashboardResponse<Vec<AuxiliaireRow>>, String> {
    let normalized_type = tiers_type.trim().to_ascii_lowercase();
    let is_fournisseurs = normalized_type == "fournisseurs";
    let is_clients = normalized_type == "clients";

    if !is_fournisseurs && !is_clients {
        return Err("tiers_type must be 'fournisseurs' or 'clients'".to_string());
    }

    let config = load_connection_config(&state, &id)?;
    let hint_schema = state.get_connection_schema(&id);
    let timeout_secs = dashboard_timeout_secs(state.inner());
    let mut client = get_active_client(state.inner(), &id).await?;
    if should_use_sage1000_analytics(
        &mut client,
        hint_schema.as_ref(),
        "get_grand_livre_auxiliaire",
    )
    .await?
    {
        return get_grand_livre_auxiliaire_sage1000(
            &mut client,
            &date_from,
            &date_to,
            &normalized_type,
            account_prefix.as_deref(),
            timeout_secs,
        )
        .await;
    }
    let (entry_context, mut warnings) =
        resolve_entry_context_smart(&mut client, hint_schema.as_ref()).await?;

    let Some(entry_context) = entry_context else {
        return Ok(empty_vec_response(warnings));
    };

    let Some(tiers_code_col) = entry_context.tiers_code_col.clone() else {
        warnings.push("No tiers code column detected on accounting entries".to_string());
        return Ok(empty_vec_response(warnings));
    };

    let account_lookup = if let Some(ref schema) = hint_schema {
        if !schema.edition.is_generic() {
            schema_lookup_context(
                &mut client,
                &schema.table_comptes,
                &schema.col_compte_num,
                &schema.col_compte_lib,
                &schema.col_compte_type,
            )
            .await?
            .or(resolve_lookup_context(
                &mut client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?)
        } else {
            resolve_lookup_context(
                &mut client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?
        }
    } else {
        resolve_lookup_context(
            &mut client,
            ACCOUNT_TABLE_CANDIDATES,
            &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
            &[
                "CG_Intitule",
                "CT_Intitule",
                "LIBELLE_COMPTE",
                "ACCOUNT_LABEL",
                "ACCOUNT_NAME",
            ],
            &[],
        )
        .await?
    };
    let tiers_lookup = if let Some(ref schema) = hint_schema {
        if !schema.edition.is_generic() {
            schema_lookup_context(
                &mut client,
                &schema.table_tiers,
                &schema.col_tiers_code,
                &schema.col_tiers_nom,
                &schema.col_tiers_type,
            )
            .await?
            .or(resolve_lookup_context(
                &mut client,
                TIERS_TABLE_CANDIDATES,
                &[
                    "CT_Num",
                    "TIERS_CODE",
                    "CODE_TIERS",
                    "ACCOUNT_NO",
                    "CUSTOMER_NO",
                    "SUPPLIER_NO",
                ],
                &[
                    "CT_Intitule",
                    "TIERS_NAME",
                    "LIBELLE",
                    "NAME",
                    "ACCOUNT_NAME",
                    "CUSTOMER_NAME",
                    "SUPPLIER_NAME",
                ],
                &["CT_Type", "TIERS_TYPE", "ACCOUNT_TYPE", "TYPE_TIERS"],
            )
            .await?)
        } else {
            resolve_lookup_context(
                &mut client,
                TIERS_TABLE_CANDIDATES,
                &[
                    "CT_Num",
                    "TIERS_CODE",
                    "CODE_TIERS",
                    "ACCOUNT_NO",
                    "CUSTOMER_NO",
                    "SUPPLIER_NO",
                ],
                &[
                    "CT_Intitule",
                    "TIERS_NAME",
                    "LIBELLE",
                    "NAME",
                    "ACCOUNT_NAME",
                    "CUSTOMER_NAME",
                    "SUPPLIER_NAME",
                ],
                &["CT_Type", "TIERS_TYPE", "ACCOUNT_TYPE", "TYPE_TIERS"],
            )
            .await?
        }
    } else {
        resolve_lookup_context(
            &mut client,
            TIERS_TABLE_CANDIDATES,
            &[
                "CT_Num",
                "TIERS_CODE",
                "CODE_TIERS",
                "ACCOUNT_NO",
                "CUSTOMER_NO",
                "SUPPLIER_NO",
            ],
            &[
                "CT_Intitule",
                "TIERS_NAME",
                "LIBELLE",
                "NAME",
                "ACCOUNT_NAME",
                "CUSTOMER_NAME",
                "SUPPLIER_NAME",
            ],
            &["CT_Type", "TIERS_TYPE", "ACCOUNT_TYPE", "TYPE_TIERS"],
        )
        .await?
    };

    if tiers_lookup.is_none() {
        warnings.push("No tiers lookup table detected; tiers names will be blank".to_string());
    }

    let parsed_date_expr = format!(
        "TRY_CAST({} AS DATE)",
        qualify("e", &entry_context.date_col)
    );
    let journal_expr = entry_context
        .journal_col
        .as_ref()
        .map(|column| text_or_empty(&qualify("e", column), 64))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(64))".to_string());
    let account_no_expr = text_or_empty(&qualify("e", &entry_context.account_col), 64);
    let ref_piece_expr = entry_context
        .ref_piece_col
        .as_ref()
        .map(|column| text_or_empty(&qualify("e", column), 128))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(128))".to_string());
    let lettrage_expr = entry_context
        .lettrage_col
        .as_ref()
        .map(|column| text_or_empty(&qualify("e", column), 64))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(64))".to_string());
    let description_expr = entry_context
        .description_col
        .as_ref()
        .map(|column| text_or_empty(&qualify("e", column), 255))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(255))".to_string());
    let account_label_expr = match (&account_lookup, &entry_context.account_label_col) {
        (Some(lookup), Some(entry_label)) => format!(
            "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
            qualify("a", &lookup.label_col),
            qualify("e", entry_label)
        ),
        (Some(lookup), None) => text_or_empty(&qualify("a", &lookup.label_col), 255),
        (None, Some(entry_label)) => text_or_empty(&qualify("e", entry_label), 255),
        (None, None) => "CAST('' AS NVARCHAR(255))".to_string(),
    };
    let tiers_code_expr = text_or_empty(&qualify("e", &tiers_code_col), 64);
    let tiers_name_expr = tiers_lookup
        .as_ref()
        .map(|lookup| text_or_empty(&qualify("t", &lookup.label_col), 255))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(255))".to_string());
    let account_join = account_lookup
        .as_ref()
        .map(|lookup| {
            format!(
                "LEFT JOIN {} a ON {} = {}",
                lookup.table.sql_name(),
                qualify("a", &lookup.code_col),
                qualify("e", &entry_context.account_col)
            )
        })
        .unwrap_or_default();
    let tiers_join = tiers_lookup
        .as_ref()
        .map(|lookup| {
            format!(
                "LEFT JOIN {} t ON {} = {}",
                lookup.table.sql_name(),
                qualify("t", &lookup.code_col),
                qualify("e", &tiers_code_col)
            )
        })
        .unwrap_or_default();
    let default_account_filter = if is_fournisseurs {
        format!(
            "({account_no_expr} LIKE '40%' OR {account_no_expr} LIKE '401%')",
            account_no_expr = account_no_expr
        )
    } else {
        format!(
            "({account_no_expr} LIKE '41%' OR {account_no_expr} LIKE '411%')",
            account_no_expr = account_no_expr
        )
    };
    let type_filter = tiers_lookup
        .as_ref()
        .and_then(|lookup| lookup.type_col.as_ref())
        .map(|type_col| {
            let type_value = if is_fournisseurs { 1 } else { 0 };
            format!(
                "COALESCE(TRY_CAST({} AS INT), -1) = {}",
                qualify("t", type_col),
                type_value
            )
        });
    let base_kind_filter = type_filter
        .map(|type_filter| format!("({} OR {})", type_filter, default_account_filter))
        .unwrap_or(default_account_filter);
    let scoped_prefix_filter = account_prefix
        .as_ref()
        .filter(|prefix| !prefix.trim().is_empty())
        .map(|prefix| {
            format!(
                " AND {} LIKE {}",
                account_no_expr,
                sql_literal(&format!("{}%", prefix.trim()))
            )
        })
        .unwrap_or_default();

    let sql = format!(
        r#"
        WITH base AS (
            SELECT
                parsed_date = {parsed_date_expr},
                account_no = {account_no_expr},
                account_label = {account_label_expr},
                tiers_code = {tiers_code_expr},
                tiers_name = {tiers_name_expr},
                journal = {journal_expr},
                description = {description_expr},
                ref_piece = {ref_piece_expr},
                lettrage = {lettrage_expr},
                debit = {debit_expr},
                credit = {credit_expr}
            FROM {entries_table} e
            {account_join}
            {tiers_join}
            WHERE {parsed_date_expr} BETWEEN {date_from} AND {date_to}
              AND {base_kind_filter}
              AND {tiers_code_expr} <> ''
              {scoped_prefix_filter}
        ),
        ranked AS (
            SELECT
                account_no,
                account_label,
                tiers_code,
                tiers_name,
                CONVERT(char(10), parsed_date, 103) AS [date],
                journal,
                description,
                ref_piece,
                lettrage,
                debit,
                credit,
                SUM(debit - credit) OVER (
                    PARTITION BY tiers_code
                    ORDER BY account_no, parsed_date, journal, ref_piece, description
                    ROWS UNBOUNDED PRECEDING
                ) AS running_balance,
                ROW_NUMBER() OVER (
                    PARTITION BY tiers_code
                    ORDER BY account_no, parsed_date, journal, ref_piece, description
                ) AS sort_row
            FROM base
            WHERE parsed_date IS NOT NULL
        ),
        totals AS (
            SELECT
                MAX(account_no) AS account_no,
                MAX(account_label) AS account_label,
                tiers_code,
                MAX(tiers_name) AS tiers_name,
                CAST(NULL AS NVARCHAR(10)) AS [date],
                CAST(NULL AS NVARCHAR(64)) AS journal,
                CAST(NULL AS NVARCHAR(255)) AS description,
                CAST(NULL AS NVARCHAR(128)) AS ref_piece,
                CAST(NULL AS NVARCHAR(64)) AS lettrage,
                SUM(debit) AS debit,
                SUM(credit) AS credit,
                SUM(debit - credit) AS running_balance,
                CAST(1 AS BIT) AS is_total_row,
                2147483647 AS sort_row
            FROM base
            WHERE parsed_date IS NOT NULL
            GROUP BY tiers_code
        )
        SELECT
            account_no,
            account_label,
            tiers_code,
            tiers_name,
            [date],
            journal,
            description,
            ref_piece,
            lettrage,
            debit,
            credit,
            running_balance,
            is_total_row
        FROM (
            SELECT
                account_no,
                account_label,
                tiers_code,
                tiers_name,
                [date],
                journal,
                description,
                ref_piece,
                lettrage,
                debit,
                credit,
                running_balance,
                CAST(0 AS BIT) AS is_total_row,
                sort_row
            FROM ranked
            UNION ALL
            SELECT
                account_no,
                account_label,
                tiers_code,
                tiers_name,
                [date],
                journal,
                description,
                ref_piece,
                lettrage,
                debit,
                credit,
                running_balance,
                is_total_row,
                sort_row
            FROM totals
        ) gl_aux
        ORDER BY account_no, tiers_code, sort_row
        "#,
        parsed_date_expr = parsed_date_expr,
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        tiers_code_expr = tiers_code_expr,
        tiers_name_expr = tiers_name_expr,
        journal_expr = journal_expr,
        description_expr = description_expr,
        ref_piece_expr = ref_piece_expr,
        lettrage_expr = lettrage_expr,
        debit_expr = entry_context.debit_expr,
        credit_expr = entry_context.credit_expr,
        entries_table = entry_context.table.sql_name(),
        account_join = account_join,
        tiers_join = tiers_join,
        date_from = sql_literal(&date_from),
        date_to = sql_literal(&date_to),
        base_kind_filter = base_kind_filter,
        scoped_prefix_filter = scoped_prefix_filter
    );

    let data = db::execute_logged_query(
        &mut client,
        &sql,
        db::QueryExecutionContext::new(
            "get_grand_livre_auxiliaire",
            "dashboard_grand_livre_auxiliaire",
        )
        .with_connection_id(id.clone())
        .with_database(config.database.clone())
        .with_table(entry_context.table.sql_name())
        .with_timeout_secs(timeout_secs),
    )
    .await?;
    let indexes = column_index_map(&data);
    let rows = data
        .rows
        .iter()
        .map(|row| AuxiliaireRow {
            account_no: row_string(row, &indexes, "account_no").unwrap_or_default(),
            account_label: row_string(row, &indexes, "account_label").unwrap_or_default(),
            tiers_code: row_string(row, &indexes, "tiers_code").unwrap_or_default(),
            tiers_name: row_string(row, &indexes, "tiers_name").unwrap_or_default(),
            date: row_string(row, &indexes, "date"),
            journal: row_string(row, &indexes, "journal"),
            description: row_string(row, &indexes, "description"),
            ref_piece: row_string(row, &indexes, "ref_piece"),
            lettrage: row_string(row, &indexes, "lettrage"),
            debit: row_decimal(row, &indexes, "debit"),
            credit: row_decimal(row, &indexes, "credit"),
            running_balance: row_decimal(row, &indexes, "running_balance"),
            is_total_row: row_bool(row, &indexes, "is_total_row"),
        })
        .collect();

    Ok(DashboardResponse {
        data: rows,
        warning: join_warnings(warnings),
    })
}

// ─── Streaming Analytics Commands ────────────────────────────────────────────

#[derive(Clone, Serialize)]
struct StreamTotalPayload {
    total: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

#[derive(Clone, Serialize)]
struct StreamGlChunkPayload {
    rows: Vec<GrandLivreRow>,
}

#[derive(Clone, Serialize)]
struct StreamBalanceChunkPayload {
    rows: Vec<BalanceRow>,
}

#[derive(Clone, Serialize)]
struct StreamAuxChunkPayload {
    rows: Vec<AuxiliaireRow>,
}

#[derive(Clone, Serialize)]
struct StreamCompletePayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct PagedDashboardResponse<T> {
    data: Vec<T>,
    total: i64,
    offset: i64,
    limit: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

const STREAM_CHUNK_SIZE: i64 = 2000;
const EXCEL_MAX_ROWS: u32 = 1_048_576;
const XLSX_MAX_PACKAGE_BYTES: u64 = 3_800_000_000;
const XLSX_TARGET_PART_BYTES: u64 = 450_000_000;
const XLSX_ESTIMATED_BYTES_PER_ROW: u64 = 500;
const XLSX_ESTIMATED_FIXED_BYTES: u64 = 2_000_000;
const XLSX_WRITE_BUFFER_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Serialize)]
struct ExportGrandLivreProgressPayload {
    loaded_rows: i64,
    total_rows: i64,
    percent: u8,
    current_sheet: u32,
    current_part: u32,
    total_parts: u32,
    phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_path: Option<String>,
}

struct ZipEntryMeta {
    name: String,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
}

struct ZipStream<W: Write + Seek> {
    writer: W,
    entries: Vec<ZipEntryMeta>,
    current: Option<ZipEntryMeta>,
}

impl<W: Write + Seek> ZipStream<W> {
    fn new(writer: W) -> Self {
        Self {
            writer,
            entries: Vec::new(),
            current: None,
        }
    }

    fn start_entry(&mut self, name: &str) -> Result<(), String> {
        if self.current.is_some() {
            return Err("A ZIP entry is already open".to_string());
        }
        let offset = self.position()?;
        self.write_u32(0x0403_4b50)?;
        self.write_u16(20)?;
        self.write_u16(0x0008)?;
        self.write_u16(0)?;
        self.write_u16(0)?;
        self.write_u16(0)?;
        self.write_u32(0)?;
        self.write_u32(0)?;
        self.write_u32(0)?;
        self.write_u16(name.as_bytes().len() as u16)?;
        self.write_u16(0)?;
        self.writer
            .write_all(name.as_bytes())
            .map_err(|e| format!("Failed to write ZIP entry name: {}", e))?;
        self.current = Some(ZipEntryMeta {
            name: name.to_string(),
            crc32: 0xffff_ffff,
            compressed_size: 0,
            uncompressed_size: 0,
            local_header_offset: offset,
        });
        Ok(())
    }

    fn write_entry_data(&mut self, data: &[u8]) -> Result<(), String> {
        let Some(current) = self.current.as_mut() else {
            return Err("No ZIP entry is open".to_string());
        };
        self.writer
            .write_all(data)
            .map_err(|e| format!("Failed to write ZIP entry data: {}", e))?;
        current.crc32 = update_crc32(current.crc32, data);
        current.compressed_size += data.len() as u64;
        current.uncompressed_size += data.len() as u64;
        Ok(())
    }

    fn finish_entry(&mut self) -> Result<(), String> {
        let Some(mut current) = self.current.take() else {
            return Err("No ZIP entry is open".to_string());
        };
        current.crc32 ^= 0xffff_ffff;
        self.write_u32(0x0807_4b50)?;
        self.write_u32(current.crc32)?;
        self.write_u32(as_zip_u32(current.compressed_size)?)?;
        self.write_u32(as_zip_u32(current.uncompressed_size)?)?;
        self.entries.push(current);
        Ok(())
    }

    fn add_entry(&mut self, name: &str, data: &str) -> Result<(), String> {
        self.start_entry(name)?;
        self.write_entry_data(data.as_bytes())?;
        self.finish_entry()
    }

    fn finish(mut self) -> Result<(), String> {
        if self.current.is_some() {
            return Err("Cannot finish ZIP while an entry is open".to_string());
        }
        let central_offset = self.position()?;
        let entries = std::mem::take(&mut self.entries);
        for entry in &entries {
            self.write_u32(0x0201_4b50)?;
            self.write_u16(20)?;
            self.write_u16(20)?;
            self.write_u16(0x0008)?;
            self.write_u16(0)?;
            self.write_u16(0)?;
            self.write_u16(0)?;
            self.write_u32(entry.crc32)?;
            self.write_u32(as_zip_u32(entry.compressed_size)?)?;
            self.write_u32(as_zip_u32(entry.uncompressed_size)?)?;
            self.write_u16(entry.name.as_bytes().len() as u16)?;
            self.write_u16(0)?;
            self.write_u16(0)?;
            self.write_u16(0)?;
            self.write_u16(0)?;
            self.write_u32(0)?;
            self.write_u32(as_zip_u32(entry.local_header_offset)?)?;
            self.writer
                .write_all(entry.name.as_bytes())
                .map_err(|e| format!("Failed to write ZIP central directory: {}", e))?;
        }
        let central_size = self.position()? - central_offset;
        self.write_u32(0x0605_4b50)?;
        self.write_u16(0)?;
        self.write_u16(0)?;
        self.write_u16(entries.len() as u16)?;
        self.write_u16(entries.len() as u16)?;
        self.write_u32(as_zip_u32(central_size)?)?;
        self.write_u32(as_zip_u32(central_offset)?)?;
        self.write_u16(0)?;
        self.writer
            .flush()
            .map_err(|e| format!("Failed to flush XLSX file: {}", e))
    }

    fn flush(&mut self) -> Result<(), String> {
        self.writer
            .flush()
            .map_err(|e| format!("Failed to flush XLSX file: {}", e))
    }

    fn position(&mut self) -> Result<u64, String> {
        self.writer
            .stream_position()
            .map_err(|e| format!("Failed to read ZIP position: {}", e))
    }

    fn write_u16(&mut self, value: u16) -> Result<(), String> {
        self.writer
            .write_all(&value.to_le_bytes())
            .map_err(|e| format!("Failed to write ZIP data: {}", e))
    }

    fn write_u32(&mut self, value: u32) -> Result<(), String> {
        self.writer
            .write_all(&value.to_le_bytes())
            .map_err(|e| format!("Failed to write ZIP data: {}", e))
    }
}

fn as_zip_u32(value: u64) -> Result<u32, String> {
    u32::try_from(value).map_err(|_| "XLSX package is larger than ZIP32 supports".to_string())
}

fn update_crc32(mut crc: u32, data: &[u8]) -> u32 {
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    crc
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn sheet_name(index: u32) -> String {
    if index == 1 {
        "Grand Livre".to_string()
    } else {
        format!("Grand Livre {}", index)
    }
}

fn xlsx_col_name(mut index: usize) -> String {
    let mut name = String::new();
    loop {
        let remainder = index % 26;
        name.insert(0, (b'A' + remainder as u8) as char);
        if index < 26 {
            break;
        }
        index = (index / 26) - 1;
    }
    name
}

fn write_xlsx_cell<W: Write + Seek>(
    zip: &mut ZipStream<W>,
    row_index: u32,
    col_index: usize,
    value: &str,
    numeric: bool,
) -> Result<(), String> {
    let cell_ref = format!("{}{}", xlsx_col_name(col_index), row_index);
    if numeric {
        zip.write_entry_data(format!(r#"<c r="{cell_ref}"><v>{}</v></c>"#, value).as_bytes())
    } else {
        zip.write_entry_data(
            format!(
                r#"<c r="{cell_ref}" t="inlineStr"><is><t>{}</t></is></c>"#,
                xml_escape(value)
            )
            .as_bytes(),
        )
    }
}

fn write_xlsx_row<W: Write + Seek>(
    zip: &mut ZipStream<W>,
    row_index: u32,
    cells: &[(&str, bool)],
) -> Result<(), String> {
    zip.write_entry_data(format!(r#"<row r="{row_index}">"#).as_bytes())?;
    for (index, (value, numeric)) in cells.iter().enumerate() {
        write_xlsx_cell(zip, row_index, index, value, *numeric)?;
    }
    zip.write_entry_data(b"</row>")
}

pub(crate) struct TableXlsxWriter<W: Write + Seek> {
    zip: ZipStream<W>,
    sheet_index: u32,
    current_row: u32,
    headers: Vec<String>,
}

impl<W: Write + Seek> TableXlsxWriter<W> {
    pub(crate) fn new(writer: W, headers: Vec<String>) -> Result<Self, String> {
        let mut export = Self {
            zip: ZipStream::new(writer),
            sheet_index: 0,
            current_row: 0,
            headers,
        };
        export.start_sheet()?;
        Ok(export)
    }

    fn start_sheet(&mut self) -> Result<(), String> {
        if self.sheet_index > 0 {
            self.zip.write_entry_data(b"</sheetData></worksheet>")?;
            self.zip.finish_entry()?;
        }
        self.sheet_index += 1;
        self.current_row = 0;
        self.zip
            .start_entry(&format!("xl/worksheets/sheet{}.xml", self.sheet_index))?;
        self.zip.write_entry_data(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
        )?;
        let headers = self.headers.clone();
        self.write_values(&headers, &vec![false; headers.len()])
    }

    fn write_values(&mut self, values: &[String], numeric: &[bool]) -> Result<(), String> {
        self.current_row += 1;
        let cells = values
            .iter()
            .enumerate()
            .map(|(index, value)| (value.as_str(), numeric.get(index).copied().unwrap_or(false)))
            .collect::<Vec<_>>();
        write_xlsx_row(&mut self.zip, self.current_row, &cells)
    }

    pub(crate) fn write_row(&mut self, values: &[String], numeric: &[bool]) -> Result<(), String> {
        if self.current_row >= EXCEL_MAX_ROWS {
            self.start_sheet()?;
        }
        self.write_values(values, numeric)
    }

    pub(crate) fn flush(&mut self) -> Result<(), String> {
        self.zip.flush()
    }

    pub(crate) fn finish(mut self) -> Result<u32, String> {
        self.zip.write_entry_data(b"</sheetData></worksheet>")?;
        self.zip.finish_entry()?;
        let count = self.sheet_index;
        self.add_workbook_parts()?;
        self.zip.finish()?;
        Ok(count)
    }

    fn add_workbook_parts(&mut self) -> Result<(), String> {
        let mut content_types = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
        );
        let mut sheets = String::new();
        let mut rels = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        );
        for index in 1..=self.sheet_index {
            content_types.push_str(&format!(
                r#"<Override PartName="/xl/worksheets/sheet{index}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
            ));
            sheets.push_str(&format!(
                r#"<sheet name="Export {index}" sheetId="{index}" r:id="rId{index}"/>"#
            ));
            rels.push_str(&format!(
                r#"<Relationship Id="rId{index}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{index}.xml"/>"#
            ));
        }
        content_types.push_str("</Types>");
        rels.push_str("</Relationships>");
        self.zip.add_entry("[Content_Types].xml", &content_types)?;
        self.zip.add_entry(
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        )?;
        self.zip.add_entry(
            "xl/workbook.xml",
            &format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{sheets}</sheets></workbook>"#
            ),
        )?;
        self.zip.add_entry("xl/_rels/workbook.xml.rels", &rels)
    }
}

struct GrandLivreTotals {
    account_no: String,
    account_label: String,
    total_debit: Decimal,
    total_credit: Decimal,
    solde: Decimal,
}

struct GrandLivreXlsxWriter<W: Write + Seek> {
    zip: ZipStream<W>,
    sheet_index: u32,
    current_row: u32,
    current_account: Option<GrandLivreTotals>,
}

impl<W: Write + Seek> GrandLivreXlsxWriter<W> {
    fn new(writer: W) -> Result<Self, String> {
        let mut export = Self {
            zip: ZipStream::new(writer),
            sheet_index: 0,
            current_row: 0,
            current_account: None,
        };
        export.start_sheet(None)?;
        Ok(export)
    }

    fn sheet_count(&self) -> u32 {
        self.sheet_index
    }

    fn flush(&mut self) -> Result<(), String> {
        self.zip.flush()
    }

    fn start_sheet(&mut self, repeat_account: Option<(String, String)>) -> Result<(), String> {
        if self.sheet_index > 0 {
            self.zip.write_entry_data(b"</sheetData></worksheet>")?;
            self.zip.finish_entry()?;
        }
        self.sheet_index += 1;
        self.current_row = 0;
        self.zip
            .start_entry(&format!("xl/worksheets/sheet{}.xml", self.sheet_index))?;
        self.zip.write_entry_data(
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
        )?;
        self.write_row(&[
            ("Date", false),
            ("Journal", false),
            ("Compte", false),
            ("Libelle", false),
            ("Piece", false),
            ("Lettrage", false),
            ("Debit", false),
            ("Credit", false),
            ("Solde cumule", false),
        ])?;
        if let Some((account_no, account_label)) = repeat_account {
            self.write_account_header(&account_no, &account_label)?;
        }
        Ok(())
    }

    fn ensure_space(&mut self, needed_rows: u32) -> Result<(), String> {
        if self.current_row + needed_rows <= EXCEL_MAX_ROWS {
            return Ok(());
        }
        let repeat = self
            .current_account
            .as_ref()
            .map(|account| (account.account_no.clone(), account.account_label.clone()));
        self.start_sheet(repeat)
    }

    fn write_row(&mut self, cells: &[(&str, bool)]) -> Result<(), String> {
        self.current_row += 1;
        write_xlsx_row(&mut self.zip, self.current_row, cells)
    }

    fn write_account_header(
        &mut self,
        account_no: &str,
        account_label: &str,
    ) -> Result<(), String> {
        self.ensure_space(1)?;
        let label = format!("{} - {}", account_no, account_label);
        self.write_row(&[
            (&label, false),
            ("", false),
            ("", false),
            ("", false),
            ("", false),
            ("", false),
            ("", false),
            ("", false),
            ("", false),
        ])
    }

    fn flush_subtotal(&mut self) -> Result<(), String> {
        let Some(current) = self.current_account.as_ref() else {
            return Ok(());
        };
        let repeat = (current.account_no.clone(), current.account_label.clone());
        self.ensure_space(1)?;
        if self.current_row == 1 {
            self.write_account_header(&repeat.0, &repeat.1)?;
        }
        let Some(totals) = self.current_account.take() else {
            return Ok(());
        };
        let debit = totals.total_debit.to_string();
        let credit = totals.total_credit.to_string();
        let solde = totals.solde.to_string();
        self.write_row(&[
            ("Total", false),
            ("", false),
            (&totals.account_no, false),
            (&totals.account_label, false),
            ("", false),
            ("", false),
            (&debit, true),
            (&credit, true),
            (&solde, true),
        ])
    }

    fn write_entry(&mut self, row: &GrandLivreRow) -> Result<(), String> {
        let account_changed = self
            .current_account
            .as_ref()
            .map(|account| {
                account.account_no != row.account_no || account.account_label != row.account_label
            })
            .unwrap_or(true);

        if account_changed {
            self.flush_subtotal()?;
            self.ensure_space(2)?;
            self.current_account = Some(GrandLivreTotals {
                account_no: row.account_no.clone(),
                account_label: row.account_label.clone(),
                total_debit: Decimal::ZERO,
                total_credit: Decimal::ZERO,
                solde: Decimal::ZERO,
            });
            self.write_account_header(&row.account_no, &row.account_label)?;
        }

        self.ensure_space(1)?;
        if let Some(account) = self.current_account.as_mut() {
            account.total_debit += row.debit;
            account.total_credit += row.credit;
            account.solde += row.debit - row.credit;
        }
        let debit = row.debit.to_string();
        let credit = row.credit.to_string();
        let balance = row.solde_cumule.to_string();
        self.write_row(&[
            (row.date.as_deref().unwrap_or(""), false),
            (row.journal.as_deref().unwrap_or(""), false),
            (&row.account_no, false),
            (&row.account_label, false),
            (row.ref_piece.as_deref().unwrap_or(""), false),
            (row.lettrage.as_deref().unwrap_or(""), false),
            (&debit, true),
            (&credit, true),
            (&balance, true),
        ])
    }

    fn finish(mut self) -> Result<u32, String> {
        self.flush_subtotal()?;
        self.zip.write_entry_data(b"</sheetData></worksheet>")?;
        self.zip.finish_entry()?;
        let sheet_count = self.sheet_index;
        self.add_workbook_parts(sheet_count)?;
        self.zip.finish()?;
        Ok(sheet_count)
    }

    fn add_workbook_parts(&mut self, sheet_count: u32) -> Result<(), String> {
        let mut content_types = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
        );
        let mut sheets = String::new();
        let mut rels = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        );
        for index in 1..=sheet_count {
            content_types.push_str(&format!(
                r#"<Override PartName="/xl/worksheets/sheet{index}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
            ));
            sheets.push_str(&format!(
                r#"<sheet name="{}" sheetId="{index}" r:id="rId{index}"/>"#,
                xml_escape(&sheet_name(index))
            ));
            rels.push_str(&format!(
                r#"<Relationship Id="rId{index}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{index}.xml"/>"#
            ));
        }
        content_types.push_str("</Types>");
        rels.push_str("</Relationships>");
        let workbook = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{sheets}</sheets></workbook>"#
        );
        self.zip.add_entry("[Content_Types].xml", &content_types)?;
        self.zip.add_entry(
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        )?;
        self.zip.add_entry("xl/workbook.xml", &workbook)?;
        self.zip.add_entry("xl/_rels/workbook.xml.rels", &rels)?;
        Ok(())
    }
}

// ─── stream_grand_livre ───────────────────────────────────────────────────────

#[tauri::command]
pub async fn stream_grand_livre(
    window: tauri::Window,
    state: State<'_, AppState>,
    id: String,
    date_from: String,
    date_to: String,
    account_prefix: Option<String>,
    preview_limit: Option<i64>,
) -> Result<(), String> {
    let result = do_stream_grand_livre(
        &window,
        &state,
        &id,
        &date_from,
        &date_to,
        account_prefix,
        preview_limit,
    )
    .await;
    if let Err(ref e) = result {
        let _ = window.emit("gl_error", e.clone());
    }
    result
}

async fn do_stream_grand_livre(
    window: &tauri::Window,
    state: &AppState,
    id: &str,
    date_from: &str,
    date_to: &str,
    account_prefix: Option<String>,
    preview_limit: Option<i64>,
) -> Result<(), String> {
    let _config = state.resolve_connection_config(id)?;
    let hint_schema = state.get_connection_schema(id);
    let timeout_secs = dashboard_timeout_secs(state);
    let mut client = get_active_client(state, id).await?;
    if should_use_sage1000_analytics(&mut client, hint_schema.as_ref(), "stream_grand_livre")
        .await?
    {
        stream_gl_sage1000(
            window,
            &mut client,
            date_from,
            date_to,
            account_prefix.as_deref(),
            timeout_secs,
            preview_limit,
        )
        .await
    } else {
        stream_gl_generic(
            window,
            &mut client,
            hint_schema.as_ref(),
            date_from,
            date_to,
            account_prefix.as_deref(),
            timeout_secs,
            preview_limit,
        )
        .await
    }
}

async fn stream_gl_with_base_cte(
    window: &tauri::Window,
    client: &mut db::DbClient,
    base_cte: &str,
    order_by: &str,
    warning: Option<String>,
    timeout_secs: u64,
    preview_limit: Option<i64>,
) -> Result<(), String> {
    let count_sql = format!(
        "{} SELECT COUNT(*) AS total FROM base WHERE parsed_date IS NOT NULL",
        base_cte
    );
    let count_data = db::execute_logged_query(
        client,
        &count_sql,
        db::QueryExecutionContext::new("stream_grand_livre", "stream_grand_livre_count")
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let idx = column_index_map(&count_data);
    let total = count_data
        .rows
        .first()
        .map(|r| row_i64(r, &idx, "total"))
        .unwrap_or(0);

    let _ = window.emit(
        "gl_total",
        StreamTotalPayload {
            total,
            warning: warning.clone(),
        },
    );

    let mut running_balance: HashMap<String, Decimal> = HashMap::new();
    let mut offset = 0i64;

    let max_rows = preview_limit.filter(|limit| *limit > 0);

    loop {
        if max_rows.map(|limit| offset >= limit).unwrap_or(false) {
            break;
        }
        let fetch_size = max_rows
            .map(|limit| (limit - offset).min(STREAM_CHUNK_SIZE))
            .unwrap_or(STREAM_CHUNK_SIZE);
        let chunk_sql = format!(
            "{}\nSELECT CONVERT(char(10), parsed_date, 103) AS [date], journal, account_no, account_label, ref_piece, lettrage, debit, credit\nFROM base\nWHERE parsed_date IS NOT NULL\nORDER BY {}\nOFFSET {} ROWS FETCH NEXT {} ROWS ONLY",
            base_cte, order_by, offset, fetch_size
        );
        let data = db::execute_logged_query(
            client,
            &chunk_sql,
            db::QueryExecutionContext::new("stream_grand_livre", "stream_grand_livre_chunk")
                .with_timeout_secs(timeout_secs),
        )
        .await?;
        let ix = column_index_map(&data);
        let fetched = data.rows.len() as i64;

        let rows: Vec<GrandLivreRow> = data
            .rows
            .iter()
            .map(|row| {
                let account_no = row_string(row, &ix, "account_no").unwrap_or_default();
                let debit = row_decimal(row, &ix, "debit");
                let credit = row_decimal(row, &ix, "credit");
                let bal = running_balance
                    .entry(account_no.clone())
                    .or_insert(Decimal::ZERO);
                *bal += debit - credit;
                GrandLivreRow {
                    row_type: "entry".to_string(),
                    date: row_string(row, &ix, "date"),
                    journal: row_string(row, &ix, "journal"),
                    account_no,
                    account_label: row_string(row, &ix, "account_label").unwrap_or_default(),
                    ref_piece: row_string(row, &ix, "ref_piece"),
                    lettrage: row_string(row, &ix, "lettrage"),
                    debit,
                    credit,
                    solde_cumule: *bal,
                    total_debit: None,
                    total_credit: None,
                    solde: None,
                }
            })
            .collect();

        let _ = window.emit("gl_chunk", StreamGlChunkPayload { rows });

        if fetched < fetch_size {
            break;
        }
        offset += fetched;
    }

    let _ = window.emit("gl_complete", StreamCompletePayload { warning });
    Ok(())
}

#[tauri::command]
pub async fn get_grand_livre_page(
    state: State<'_, AppState>,
    id: String,
    date_from: String,
    date_to: String,
    account_prefix: Option<String>,
    offset: Option<i64>,
    limit: Option<i64>,
    search: Option<String>,
) -> Result<PagedDashboardResponse<GrandLivreRow>, String> {
    let _config = state.resolve_connection_config(&id)?;
    let hint_schema = state.get_connection_schema(&id);
    let timeout_secs = dashboard_timeout_secs(state.inner());
    let mut client = get_active_client(state.inner(), &id).await?;
    let (base_cte, order_by, warning) =
        if should_use_sage1000_analytics(&mut client, hint_schema.as_ref(), "get_grand_livre_page")
            .await?
        {
            build_gl_sage1000_sql(&mut client, &date_from, &date_to, account_prefix.as_deref())
                .await?
        } else {
            build_gl_generic_sql(
                &mut client,
                hint_schema.as_ref(),
                &date_from,
                &date_to,
                account_prefix.as_deref(),
            )
            .await?
        };

    get_gl_page_with_base_cte(
        &mut client,
        &base_cte,
        &order_by,
        warning,
        timeout_secs,
        offset.unwrap_or(0),
        limit.unwrap_or(STREAM_CHUNK_SIZE),
        search.as_deref(),
    )
    .await
}

async fn get_gl_page_with_base_cte(
    client: &mut db::DbClient,
    base_cte: &str,
    order_by: &str,
    warning: Option<String>,
    timeout_secs: u64,
    offset: i64,
    limit: i64,
    search: Option<&str>,
) -> Result<PagedDashboardResponse<GrandLivreRow>, String> {
    let safe_offset = offset.max(0);
    let safe_limit = limit.clamp(1, 2_000);
    let search_filter = search
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            let pattern = sql_literal(&format!("%{}%", value.replace('[', "[[]")));
            format!(
                "WHERE account_no LIKE {pattern} OR account_label LIKE {pattern} OR journal LIKE {pattern} OR ref_piece LIKE {pattern} OR lettrage LIKE {pattern}"
            )
        })
        .unwrap_or_default();
    let page_cte = format!(
        "{base_cte}, calculated AS (
            SELECT *,
                   SUM(debit - credit) OVER (PARTITION BY account_no ORDER BY {order_by} ROWS UNBOUNDED PRECEDING) AS solde_cumule
            FROM base
            WHERE parsed_date IS NOT NULL
        ), filtered AS (
            SELECT * FROM calculated {search_filter}
        )",
    );
    let count_sql = format!("{page_cte} SELECT COUNT(*) AS total FROM filtered");
    let count_data = db::execute_logged_query(
        client,
        &count_sql,
        db::QueryExecutionContext::new("get_grand_livre_page", "get_grand_livre_page_count")
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let count_idx = column_index_map(&count_data);
    let total = count_data
        .rows
        .first()
        .map(|row| row_i64(row, &count_idx, "total"))
        .unwrap_or(0);

    let page_sql = format!(
        "{page_cte}
        SELECT CONVERT(char(10), parsed_date, 103) AS [date], journal, account_no, account_label, ref_piece, lettrage, debit, credit, solde_cumule
        FROM filtered
        ORDER BY {order_by}
        OFFSET {safe_offset} ROWS FETCH NEXT {safe_limit} ROWS ONLY"
    );
    let data = db::execute_logged_query(
        client,
        &page_sql,
        db::QueryExecutionContext::new("get_grand_livre_page", "get_grand_livre_page_rows")
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let ix = column_index_map(&data);
    let rows = data
        .rows
        .iter()
        .map(|row| GrandLivreRow {
            row_type: "entry".to_string(),
            date: row_string(row, &ix, "date"),
            journal: row_string(row, &ix, "journal"),
            account_no: row_string(row, &ix, "account_no").unwrap_or_default(),
            account_label: row_string(row, &ix, "account_label").unwrap_or_default(),
            ref_piece: row_string(row, &ix, "ref_piece"),
            lettrage: row_string(row, &ix, "lettrage"),
            debit: row_decimal(row, &ix, "debit"),
            credit: row_decimal(row, &ix, "credit"),
            solde_cumule: row_decimal(row, &ix, "solde_cumule"),
            total_debit: None,
            total_credit: None,
            solde: None,
        })
        .collect();

    Ok(PagedDashboardResponse {
        data: rows,
        total,
        offset: safe_offset,
        limit: safe_limit,
        warning,
    })
}

#[tauri::command]
pub async fn export_grand_livre_xlsx(
    window: tauri::Window,
    state: State<'_, AppState>,
    id: String,
    date_from: String,
    date_to: String,
    account_prefix: Option<String>,
    file_path: String,
) -> Result<String, String> {
    cleanup_grand_livre_temp_files(Path::new(&file_path));
    let result = do_export_grand_livre_xlsx(
        &window,
        &state,
        &id,
        &date_from,
        &date_to,
        account_prefix,
        &file_path,
    )
    .await;
    if let Err(ref e) = result {
        cleanup_grand_livre_temp_files(Path::new(&file_path));
        let _ = window.emit("gl_export_error", e.clone());
    }
    result.map(|_| file_path)
}

fn cleanup_grand_livre_temp_files(base_path: &Path) {
    let Some(parent) = base_path.parent() else {
        return;
    };
    let Some(stem) = base_path.file_stem().and_then(|value| value.to_str()) else {
        return;
    };
    let prefix = format!(".{}", stem);
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let matches = path
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value.starts_with(&prefix) && value.ends_with(".tmp"))
            .unwrap_or(false);
        if matches {
            let _ = fs::remove_file(path);
        }
    }
}

async fn do_export_grand_livre_xlsx(
    window: &tauri::Window,
    state: &AppState,
    id: &str,
    date_from: &str,
    date_to: &str,
    account_prefix: Option<String>,
    file_path: &str,
) -> Result<(), String> {
    let _config = state.resolve_connection_config(id)?;
    let hint_schema = state.get_connection_schema(id);
    let timeout_secs = dashboard_timeout_secs(state).max(300);
    let mut client = get_active_client(state, id).await?;
    let (base_cte, order_by, warning) = if should_use_sage1000_analytics(
        &mut client,
        hint_schema.as_ref(),
        "export_grand_livre_xlsx",
    )
    .await?
    {
        build_gl_sage1000_sql(&mut client, date_from, date_to, account_prefix.as_deref()).await?
    } else {
        build_gl_generic_sql(
            &mut client,
            hint_schema.as_ref(),
            date_from,
            date_to,
            account_prefix.as_deref(),
        )
        .await?
    };

    export_gl_with_base_cte(
        window,
        &mut client,
        file_path,
        &base_cte,
        &order_by,
        warning,
        timeout_secs,
    )
    .await
}

fn xlsx_rows_per_part() -> i64 {
    let budget = XLSX_TARGET_PART_BYTES
        .min(XLSX_MAX_PACKAGE_BYTES)
        .saturating_sub(XLSX_ESTIMATED_FIXED_BYTES);
    ((budget / XLSX_ESTIMATED_BYTES_PER_ROW).max(1)) as i64
}

fn export_part_path(
    base_path: &Path,
    part_index: u32,
    total_parts: u32,
) -> Result<PathBuf, String> {
    if total_parts <= 1 {
        return Ok(base_path.to_path_buf());
    }

    let parent = base_path.parent().unwrap_or_else(|| Path::new(""));
    let stem = base_path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Invalid Excel export file name".to_string())?;
    let extension = base_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("xlsx");
    Ok(parent.join(format!("{}.part-{:03}.{}", stem, part_index, extension)))
}

fn export_temp_path(final_path: &Path) -> Result<PathBuf, String> {
    let file_name = final_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Invalid Excel export file path".to_string())?;
    Ok(final_path.with_file_name(format!(".{}.tmp", file_name)))
}

fn create_grand_livre_writer(
    file_path: &Path,
) -> Result<GrandLivreXlsxWriter<BufWriter<File>>, String> {
    let file = File::create(file_path)
        .map_err(|e| format!("Failed to create Excel export file: {}", e))?;
    let buffered_file = BufWriter::with_capacity(XLSX_WRITE_BUFFER_BYTES, file);
    GrandLivreXlsxWriter::new(buffered_file)
}

fn replace_export_file(tmp_path: &Path, final_path: &Path) -> Result<(), String> {
    let backup_path = final_path.with_file_name(format!(
        ".{}.backup",
        final_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| "Invalid Excel export file path".to_string())?
    ));
    let _ = fs::remove_file(&backup_path);
    let mut moved_existing = false;
    if final_path.exists() {
        fs::rename(final_path, &backup_path).map_err(|e| {
            format!(
                "Excel export completed, but preserving the existing file failed: {}",
                e
            )
        })?;
        moved_existing = true;
    }
    if let Err(error) = fs::rename(tmp_path, final_path) {
        if moved_existing {
            let _ = fs::rename(&backup_path, final_path);
        }
        return Err(format!(
            "Excel export completed, but moving the temporary file failed: {}",
            error
        ));
    }
    if moved_existing {
        let _ = fs::remove_file(backup_path);
    }
    Ok(())
}

async fn export_gl_with_base_cte(
    window: &tauri::Window,
    client: &mut db::DbClient,
    file_path: &str,
    base_cte: &str,
    order_by: &str,
    warning: Option<String>,
    timeout_secs: u64,
) -> Result<(), String> {
    let base_export_path = Path::new(file_path);
    let _ = window.emit(
        "gl_export_progress",
        ExportGrandLivreProgressPayload {
            loaded_rows: 0,
            total_rows: 0,
            percent: 0,
            current_sheet: 1,
            current_part: 1,
            total_parts: 1,
            phase: "counting".to_string(),
            output_path: None,
        },
    );

    let count_sql = format!(
        "{} SELECT COUNT(*) AS total FROM base WHERE parsed_date IS NOT NULL",
        base_cte
    );
    let count_data = db::execute_logged_query(
        client,
        &count_sql,
        db::QueryExecutionContext::new("export_grand_livre_xlsx", "export_grand_livre_count")
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let idx = column_index_map(&count_data);
    let total = count_data
        .rows
        .first()
        .map(|r| row_i64(r, &idx, "total"))
        .unwrap_or(0);
    let rows_per_part = xlsx_rows_per_part();
    let total_parts = if total <= 0 {
        1
    } else {
        u32::try_from(((total + rows_per_part - 1) / rows_per_part).max(1))
            .map_err(|_| "Grand Livre export would create too many Excel part files".to_string())?
    };
    let mut current_part = 1u32;
    let mut current_part_rows = 0i64;
    let mut current_final_path = export_part_path(base_export_path, current_part, total_parts)?;
    let mut current_tmp_path = export_temp_path(&current_final_path)?;
    let _ = fs::remove_file(&current_tmp_path);
    let _ = window.emit(
        "gl_export_progress",
        ExportGrandLivreProgressPayload {
            loaded_rows: 0,
            total_rows: total,
            percent: 0,
            current_sheet: 1,
            current_part,
            total_parts,
            phase: "writing".to_string(),
            output_path: Some(current_final_path.display().to_string()),
        },
    );

    let mut writer = create_grand_livre_writer(&current_tmp_path)?;
    let mut running_balance: HashMap<String, Decimal> = HashMap::new();
    let mut loaded = 0i64;
    let mut offset = 0i64;
    let mut last_sheet_count = 1u32;

    loop {
        let chunk_sql = format!(
            "{}\nSELECT CONVERT(char(10), parsed_date, 103) AS [date], journal, account_no, account_label, ref_piece, lettrage, debit, credit\nFROM base\nWHERE parsed_date IS NOT NULL\nORDER BY {}\nOFFSET {} ROWS FETCH NEXT {} ROWS ONLY",
            base_cte, order_by, offset, STREAM_CHUNK_SIZE
        );
        let data = db::execute_logged_query(
            client,
            &chunk_sql,
            db::QueryExecutionContext::new("export_grand_livre_xlsx", "export_grand_livre_chunk")
                .with_timeout_secs(timeout_secs),
        )
        .await?;
        let ix = column_index_map(&data);
        let fetched = data.rows.len() as i64;

        for row in &data.rows {
            if current_part_rows >= rows_per_part && current_part < total_parts {
                let sheet_count = writer.finish()?;
                replace_export_file(&current_tmp_path, &current_final_path)?;
                last_sheet_count = sheet_count;
                let percent = if total <= 0 {
                    100
                } else {
                    ((loaded.min(total) * 100) / total).clamp(0, 99) as u8
                };
                let _ = window.emit(
                    "gl_export_progress",
                    ExportGrandLivreProgressPayload {
                        loaded_rows: loaded,
                        total_rows: total,
                        percent,
                        current_sheet: sheet_count,
                        current_part,
                        total_parts,
                        phase: "part complete".to_string(),
                        output_path: Some(current_final_path.display().to_string()),
                    },
                );

                current_part += 1;
                current_part_rows = 0;
                current_final_path = export_part_path(base_export_path, current_part, total_parts)?;
                current_tmp_path = export_temp_path(&current_final_path)?;
                let _ = fs::remove_file(&current_tmp_path);
                writer = create_grand_livre_writer(&current_tmp_path)?;
            }

            let account_no = row_string(row, &ix, "account_no").unwrap_or_default();
            let debit = row_decimal(row, &ix, "debit");
            let credit = row_decimal(row, &ix, "credit");
            let bal = running_balance
                .entry(account_no.clone())
                .or_insert(Decimal::ZERO);
            *bal += debit - credit;
            writer.write_entry(&GrandLivreRow {
                row_type: "entry".to_string(),
                date: row_string(row, &ix, "date"),
                journal: row_string(row, &ix, "journal"),
                account_no,
                account_label: row_string(row, &ix, "account_label").unwrap_or_default(),
                ref_piece: row_string(row, &ix, "ref_piece"),
                lettrage: row_string(row, &ix, "lettrage"),
                debit,
                credit,
                solde_cumule: *bal,
                total_debit: None,
                total_credit: None,
                solde: None,
            })?;
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
            "gl_export_progress",
            ExportGrandLivreProgressPayload {
                loaded_rows: loaded,
                total_rows: total,
                percent,
                current_sheet: writer.sheet_count(),
                current_part,
                total_parts,
                phase: "writing".to_string(),
                output_path: Some(current_final_path.display().to_string()),
            },
        );

        if fetched < STREAM_CHUNK_SIZE {
            break;
        }
        offset += fetched;
    }

    let sheet_count = writer.finish()?;
    replace_export_file(&current_tmp_path, &current_final_path)?;
    last_sheet_count = sheet_count;
    let payload = ExportGrandLivreProgressPayload {
        loaded_rows: loaded,
        total_rows: total,
        percent: 100,
        current_sheet: last_sheet_count,
        current_part,
        total_parts,
        phase: warning.unwrap_or_else(|| "complete".to_string()),
        output_path: Some(current_final_path.display().to_string()),
    };
    let _ = window.emit("gl_export_progress", payload.clone());
    let _ = window.emit("gl_export_complete", payload);
    Ok(())
}

async fn build_gl_sage1000_sql(
    client: &mut db::DbClient,
    date_from: &str,
    date_to: &str,
    account_prefix: Option<&str>,
) -> Result<(String, String, Option<String>), String> {
    let Some(context) = resolve_sage1000_context(client).await? else {
        return Ok((
            "WITH base AS (SELECT parsed_date = CAST(NULL AS DATE), journal = CAST('' AS NVARCHAR(64)), account_no = CAST('' AS NVARCHAR(64)), account_label = CAST('' AS NVARCHAR(255)), ref_piece = CAST('' AS NVARCHAR(128)), lettrage = CAST('' AS NVARCHAR(64)), debit = CAST(0 AS DECIMAL(38,6)), credit = CAST(0 AS DECIMAL(38,6)) WHERE 1=0)".to_string(),
            "account_no".to_string(),
            Some("Sage 1000 tables TECRITURE / TCOMPTEGENERAL non détectées".to_string()),
        ));
    };

    let document_sql = sage1000_document_sql(&context);
    let account_no_expr = text_or_empty(&qualify("cg", "codeCompte"), 64);
    let account_label_expr = format!(
        "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
        qualify("cg", "Caption"),
        qualify("e", "reference")
    );
    let lettrage_expr = text_or_empty(&qualify("e", "CodeLettrageExterne"), 64);
    let debit_expr = decimal_or_zero(&qualify("e", "debit"));
    let credit_expr = decimal_or_zero(&qualify("e", "credit"));
    let account_filter_sql = account_prefix
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            format!(
                " AND {} LIKE {}",
                account_no_expr,
                sql_literal(&format!("{}%", p.trim()))
            )
        })
        .unwrap_or_default();

    let base_cte = format!(
        "WITH base AS (SELECT parsed_date = TRY_CAST({date_col} AS DATE), journal = {journal_expr}, account_no = {account_no_expr}, account_label = {account_label_expr}, ref_piece = {ref_piece_expr}, lettrage = {lettrage_expr}, debit = {debit_expr}, credit = {credit_expr}, entry_number = COALESCE(TRY_CAST({entry_number_col} AS BIGINT), 0) FROM {entries_table} e LEFT JOIN {accounts_table} cg ON {entry_account_fk} = {account_pk} {document_join} WHERE TRY_CAST({date_col} AS DATE) BETWEEN {date_from} AND {date_to} AND {account_no_filter} <> '' {account_filter_sql})",
        date_col = qualify("e", "eDate"),
        journal_expr = document_sql.journal_expr,
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        ref_piece_expr = document_sql.ref_piece_expr,
        lettrage_expr = lettrage_expr,
        debit_expr = debit_expr,
        credit_expr = credit_expr,
        entry_number_col = qualify("e", "numero"),
        entries_table = context.entries.sql_name(),
        accounts_table = context.accounts.sql_name(),
        entry_account_fk = qualify("e", "oidcompteGeneral"),
        account_pk = qualify("cg", "oid"),
        document_join = document_sql.join_sql,
        date_from = sql_literal(date_from),
        date_to = sql_literal(date_to),
        account_no_filter = account_no_expr,
        account_filter_sql = account_filter_sql
    );
    Ok((
        base_cte,
        "account_no, parsed_date, entry_number".to_string(),
        None,
    ))
}

async fn build_gl_generic_sql(
    client: &mut db::DbClient,
    hint_schema: Option<&crate::sage_compat::SageSchema>,
    date_from: &str,
    date_to: &str,
    account_prefix: Option<&str>,
) -> Result<(String, String, Option<String>), String> {
    let (entry_context, mut warnings) = resolve_entry_context_smart(client, hint_schema).await?;
    let Some(entry_context) = entry_context else {
        return Ok((
            "WITH base AS (SELECT parsed_date = CAST(NULL AS DATE), journal = CAST('' AS NVARCHAR(64)), account_no = CAST('' AS NVARCHAR(64)), account_label = CAST('' AS NVARCHAR(255)), ref_piece = CAST('' AS NVARCHAR(128)), lettrage = CAST('' AS NVARCHAR(64)), debit = CAST(0 AS DECIMAL(38,6)), credit = CAST(0 AS DECIMAL(38,6)) WHERE 1=0)".to_string(),
            "account_no".to_string(),
            Some(join_warnings(warnings).unwrap_or_default()),
        ));
    };

    let account_lookup = if let Some(schema) = hint_schema {
        if !schema.edition.is_generic() {
            schema_lookup_context(
                client,
                &schema.table_comptes,
                &schema.col_compte_num,
                &schema.col_compte_lib,
                &schema.col_compte_type,
            )
            .await?
            .or(resolve_lookup_context(
                client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?)
        } else {
            resolve_lookup_context(
                client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?
        }
    } else {
        resolve_lookup_context(
            client,
            ACCOUNT_TABLE_CANDIDATES,
            &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
            &[
                "CG_Intitule",
                "CT_Intitule",
                "LIBELLE_COMPTE",
                "ACCOUNT_LABEL",
                "ACCOUNT_NAME",
            ],
            &[],
        )
        .await?
    };

    if account_lookup.is_none() && entry_context.account_label_col.is_none() {
        warnings.push("Libellés de compte non détectés".to_string());
    }

    let parsed_date_expr = format!(
        "TRY_CAST({} AS DATE)",
        qualify("e", &entry_context.date_col)
    );
    let journal_expr = entry_context
        .journal_col
        .as_ref()
        .map(|c| text_or_empty(&qualify("e", c), 64))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(64))".to_string());
    let account_no_expr = text_or_empty(&qualify("e", &entry_context.account_col), 64);
    let ref_piece_expr = entry_context
        .ref_piece_col
        .as_ref()
        .map(|c| text_or_empty(&qualify("e", c), 128))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(128))".to_string());
    let lettrage_expr = entry_context
        .lettrage_col
        .as_ref()
        .map(|c| text_or_empty(&qualify("e", c), 64))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(64))".to_string());
    let account_label_expr = match (&account_lookup, &entry_context.account_label_col) {
        (Some(lu), Some(el)) => format!(
            "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
            qualify("a", &lu.label_col), qualify("e", el)
        ),
        (Some(lu), None) => text_or_empty(&qualify("a", &lu.label_col), 255),
        (None, Some(el)) => text_or_empty(&qualify("e", el), 255),
        (None, None) => "CAST('' AS NVARCHAR(255))".to_string(),
    };
    let account_join = account_lookup
        .as_ref()
        .map(|lu| {
            format!(
                "LEFT JOIN {} a ON {} = {}",
                lu.table.sql_name(),
                qualify("a", &lu.code_col),
                qualify("e", &entry_context.account_col)
            )
        })
        .unwrap_or_default();
    let account_filter_sql = account_prefix
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            format!(
                " AND {} LIKE {}",
                account_no_expr,
                sql_literal(&format!("{}%", p.trim()))
            )
        })
        .unwrap_or_default();

    let base_cte = format!(
        "WITH base AS (SELECT parsed_date = {parsed_date_expr}, journal = {journal_expr}, account_no = {account_no_expr}, account_label = {account_label_expr}, ref_piece = {ref_piece_expr}, lettrage = {lettrage_expr}, debit = {debit_expr}, credit = {credit_expr} FROM {entries_table} e {account_join} WHERE {parsed_date_expr} BETWEEN {date_from} AND {date_to} AND {account_no_filter} <> '' {account_filter_sql})",
        parsed_date_expr = parsed_date_expr,
        journal_expr = journal_expr,
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        ref_piece_expr = ref_piece_expr,
        lettrage_expr = lettrage_expr,
        debit_expr = entry_context.debit_expr,
        credit_expr = entry_context.credit_expr,
        entries_table = entry_context.table.sql_name(),
        account_join = account_join,
        date_from = sql_literal(date_from),
        date_to = sql_literal(date_to),
        account_no_filter = account_no_expr,
        account_filter_sql = account_filter_sql
    );

    Ok((
        base_cte,
        "account_no, parsed_date, journal, ref_piece, lettrage".to_string(),
        join_warnings(warnings),
    ))
}

async fn stream_gl_sage1000(
    window: &tauri::Window,
    client: &mut db::DbClient,
    date_from: &str,
    date_to: &str,
    account_prefix: Option<&str>,
    timeout_secs: u64,
    preview_limit: Option<i64>,
) -> Result<(), String> {
    let Some(context) = resolve_sage1000_context(client).await? else {
        let _ = window.emit(
            "gl_total",
            StreamTotalPayload {
                total: 0,
                warning: Some(
                    "Sage 1000 tables TECRITURE / TCOMPTEGENERAL non détectées".to_string(),
                ),
            },
        );
        let _ = window.emit("gl_complete", StreamCompletePayload { warning: None });
        return Ok(());
    };

    let document_sql = sage1000_document_sql(&context);
    let account_no_expr = text_or_empty(&qualify("cg", "codeCompte"), 64);
    let account_label_expr = format!(
        "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
        qualify("cg", "Caption"),
        qualify("e", "reference")
    );
    let lettrage_expr = text_or_empty(&qualify("e", "CodeLettrageExterne"), 64);
    let debit_expr = decimal_or_zero(&qualify("e", "debit"));
    let credit_expr = decimal_or_zero(&qualify("e", "credit"));
    let account_filter_sql = account_prefix
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            format!(
                " AND {} LIKE {}",
                account_no_expr,
                sql_literal(&format!("{}%", p.trim()))
            )
        })
        .unwrap_or_default();

    let base_cte = format!(
        "WITH base AS (SELECT parsed_date = TRY_CAST({date_col} AS DATE), journal = {journal_expr}, account_no = {account_no_expr}, account_label = {account_label_expr}, ref_piece = {ref_piece_expr}, lettrage = {lettrage_expr}, debit = {debit_expr}, credit = {credit_expr}, entry_number = COALESCE(TRY_CAST({entry_number_col} AS BIGINT), 0) FROM {entries_table} e LEFT JOIN {accounts_table} cg ON {entry_account_fk} = {account_pk} {document_join} WHERE TRY_CAST({date_col} AS DATE) BETWEEN {date_from} AND {date_to} AND {account_no_filter} <> '' {account_filter_sql})",
        date_col = qualify("e", "eDate"),
        journal_expr = document_sql.journal_expr,
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        ref_piece_expr = document_sql.ref_piece_expr,
        lettrage_expr = lettrage_expr,
        debit_expr = debit_expr,
        credit_expr = credit_expr,
        entry_number_col = qualify("e", "numero"),
        entries_table = context.entries.sql_name(),
        accounts_table = context.accounts.sql_name(),
        entry_account_fk = qualify("e", "oidcompteGeneral"),
        account_pk = qualify("cg", "oid"),
        document_join = document_sql.join_sql,
        date_from = sql_literal(date_from),
        date_to = sql_literal(date_to),
        account_no_filter = account_no_expr,
        account_filter_sql = account_filter_sql
    );

    stream_gl_with_base_cte(
        window,
        client,
        &base_cte,
        "account_no, parsed_date, entry_number",
        None,
        timeout_secs,
        preview_limit,
    )
    .await
}

async fn stream_gl_generic(
    window: &tauri::Window,
    client: &mut db::DbClient,
    hint_schema: Option<&crate::sage_compat::SageSchema>,
    date_from: &str,
    date_to: &str,
    account_prefix: Option<&str>,
    timeout_secs: u64,
    preview_limit: Option<i64>,
) -> Result<(), String> {
    let (entry_context, mut warnings) = resolve_entry_context_smart(client, hint_schema).await?;

    let Some(entry_context) = entry_context else {
        let _ = window.emit(
            "gl_total",
            StreamTotalPayload {
                total: 0,
                warning: Some(join_warnings(warnings).unwrap_or_default()),
            },
        );
        let _ = window.emit("gl_complete", StreamCompletePayload { warning: None });
        return Ok(());
    };

    let account_lookup = if let Some(schema) = hint_schema {
        if !schema.edition.is_generic() {
            schema_lookup_context(
                client,
                &schema.table_comptes,
                &schema.col_compte_num,
                &schema.col_compte_lib,
                &schema.col_compte_type,
            )
            .await?
            .or(resolve_lookup_context(
                client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?)
        } else {
            resolve_lookup_context(
                client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?
        }
    } else {
        resolve_lookup_context(
            client,
            ACCOUNT_TABLE_CANDIDATES,
            &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
            &[
                "CG_Intitule",
                "CT_Intitule",
                "LIBELLE_COMPTE",
                "ACCOUNT_LABEL",
                "ACCOUNT_NAME",
            ],
            &[],
        )
        .await?
    };

    if account_lookup.is_none() && entry_context.account_label_col.is_none() {
        warnings.push("Libellés de compte non détectés".to_string());
    }

    let parsed_date_expr = format!(
        "TRY_CAST({} AS DATE)",
        qualify("e", &entry_context.date_col)
    );
    let journal_expr = entry_context
        .journal_col
        .as_ref()
        .map(|c| text_or_empty(&qualify("e", c), 64))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(64))".to_string());
    let account_no_expr = text_or_empty(&qualify("e", &entry_context.account_col), 64);
    let ref_piece_expr = entry_context
        .ref_piece_col
        .as_ref()
        .map(|c| text_or_empty(&qualify("e", c), 128))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(128))".to_string());
    let lettrage_expr = entry_context
        .lettrage_col
        .as_ref()
        .map(|c| text_or_empty(&qualify("e", c), 64))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(64))".to_string());
    let account_label_expr = match (&account_lookup, &entry_context.account_label_col) {
        (Some(lu), Some(el)) => format!(
            "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
            qualify("a", &lu.label_col), qualify("e", el)
        ),
        (Some(lu), None) => text_or_empty(&qualify("a", &lu.label_col), 255),
        (None, Some(el)) => text_or_empty(&qualify("e", el), 255),
        (None, None) => "CAST('' AS NVARCHAR(255))".to_string(),
    };
    let account_join = account_lookup
        .as_ref()
        .map(|lu| {
            format!(
                "LEFT JOIN {} a ON {} = {}",
                lu.table.sql_name(),
                qualify("a", &lu.code_col),
                qualify("e", &entry_context.account_col)
            )
        })
        .unwrap_or_default();
    let account_filter_sql = account_prefix
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            format!(
                " AND {} LIKE {}",
                account_no_expr,
                sql_literal(&format!("{}%", p.trim()))
            )
        })
        .unwrap_or_default();

    let base_cte = format!(
        "WITH base AS (SELECT parsed_date = {parsed_date_expr}, journal = {journal_expr}, account_no = {account_no_expr}, account_label = {account_label_expr}, ref_piece = {ref_piece_expr}, lettrage = {lettrage_expr}, debit = {debit_expr}, credit = {credit_expr} FROM {entries_table} e {account_join} WHERE {parsed_date_expr} BETWEEN {date_from} AND {date_to} AND {account_no_filter} <> '' {account_filter_sql})",
        parsed_date_expr = parsed_date_expr,
        journal_expr = journal_expr,
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        ref_piece_expr = ref_piece_expr,
        lettrage_expr = lettrage_expr,
        debit_expr = entry_context.debit_expr,
        credit_expr = entry_context.credit_expr,
        entries_table = entry_context.table.sql_name(),
        account_join = account_join,
        date_from = sql_literal(date_from),
        date_to = sql_literal(date_to),
        account_no_filter = account_no_expr,
        account_filter_sql = account_filter_sql
    );

    stream_gl_with_base_cte(
        window,
        client,
        &base_cte,
        "account_no, parsed_date, journal, ref_piece, lettrage",
        join_warnings(warnings),
        timeout_secs,
        preview_limit,
    )
    .await
}

// ─── stream_balance ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn stream_balance(
    window: tauri::Window,
    state: State<'_, AppState>,
    id: String,
    date_from: String,
    date_to: String,
    account_prefix: Option<String>,
) -> Result<(), String> {
    let result =
        do_stream_balance(&window, &state, &id, &date_from, &date_to, account_prefix).await;
    if let Err(ref e) = result {
        let _ = window.emit("balance_error", e.clone());
    }
    result
}

async fn do_stream_balance(
    window: &tauri::Window,
    state: &AppState,
    id: &str,
    date_from: &str,
    date_to: &str,
    account_prefix: Option<String>,
) -> Result<(), String> {
    let period = resolve_balance_period(date_from, date_to)?;
    let _config = state.resolve_connection_config(id)?;
    let hint_schema = state.get_connection_schema(id);
    let timeout_secs = dashboard_timeout_secs(state);
    let mut client = get_active_client(state, id).await?;

    // Balance is an aggregated query (one row per account) — run it fully, then chunk-emit
    let result = if should_use_sage1000_analytics(
        &mut client,
        hint_schema.as_ref(),
        "stream_balance",
    )
    .await?
    {
        get_balance_sage1000(
            &mut client,
            &period,
            account_prefix.as_deref(),
            timeout_secs,
        )
        .await?
    } else {
        // Build inline via the existing get_balance logic reused here
        let (entry_context, mut warnings) =
            resolve_entry_context_smart(&mut client, hint_schema.as_ref()).await?;
        let Some(entry_context) = entry_context else {
            let _ = window.emit(
                "balance_total",
                StreamTotalPayload {
                    total: 0,
                    warning: join_warnings(warnings),
                },
            );
            let _ = window.emit("balance_complete", StreamCompletePayload { warning: None });
            return Ok(());
        };

        let account_lookup = if let Some(ref schema) = hint_schema {
            if !schema.edition.is_generic() {
                schema_lookup_context(
                    &mut client,
                    &schema.table_comptes,
                    &schema.col_compte_num,
                    &schema.col_compte_lib,
                    &schema.col_compte_type,
                )
                .await?
                .or(resolve_lookup_context(
                    &mut client,
                    ACCOUNT_TABLE_CANDIDATES,
                    &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                    &[
                        "CG_Intitule",
                        "CT_Intitule",
                        "LIBELLE_COMPTE",
                        "ACCOUNT_LABEL",
                        "ACCOUNT_NAME",
                    ],
                    &[],
                )
                .await?)
            } else {
                resolve_lookup_context(
                    &mut client,
                    ACCOUNT_TABLE_CANDIDATES,
                    &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                    &[
                        "CG_Intitule",
                        "CT_Intitule",
                        "LIBELLE_COMPTE",
                        "ACCOUNT_LABEL",
                        "ACCOUNT_NAME",
                    ],
                    &[],
                )
                .await?
            }
        } else {
            resolve_lookup_context(
                &mut client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?
        };

        if entry_context.journal_col.is_none() {
            warnings.push("Journal non détecté; solde d'ouverture calculé sur les dates antérieures uniquement".to_string());
        }

        let parsed_date_expr = format!(
            "TRY_CAST({} AS DATE)",
            qualify("e", &entry_context.date_col)
        );
        let journal_expr = entry_context
            .journal_col
            .as_ref()
            .map(|c| text_or_empty(&qualify("e", c), 64))
            .unwrap_or_else(|| "CAST('' AS NVARCHAR(64))".to_string());
        let account_no_expr = text_or_empty(&qualify("e", &entry_context.account_col), 64);
        let account_label_expr = match (&account_lookup, &entry_context.account_label_col) {
            (Some(lu), Some(el)) => format!(
                "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
                qualify("a", &lu.label_col), qualify("e", el)
            ),
            (Some(lu), None) => text_or_empty(&qualify("a", &lu.label_col), 255),
            (None, Some(el)) => text_or_empty(&qualify("e", el), 255),
            (None, None) => "CAST('' AS NVARCHAR(255))".to_string(),
        };
        let account_join = account_lookup
            .as_ref()
            .map(|lu| {
                format!(
                    "LEFT JOIN {} a ON {} = {}",
                    lu.table.sql_name(),
                    qualify("a", &lu.code_col),
                    qualify("e", &entry_context.account_col)
                )
            })
            .unwrap_or_default();
        let account_filter_sql = account_prefix
            .as_ref()
            .filter(|p| !p.trim().is_empty())
            .map(|p| {
                format!(
                    " AND {} LIKE {}",
                    account_no_expr,
                    sql_literal(&format!("{}%", p.trim()))
                )
            })
            .unwrap_or_default();
        let base_opening_condition = if entry_context.journal_col.is_some() {
            format!(
                "(journal IN ('RAN', 'AN', 'OUV') OR parsed_date < {})",
                sql_literal(&period.date_from)
            )
        } else {
            format!("parsed_date < {}", sql_literal(&period.date_from))
        };
        let opening_condition = format!(
            "NOT {} AND {}",
            annual_account_condition("account_no"),
            base_opening_condition
        );
        let movement_condition = balance_movement_condition(
            "account_no",
            "parsed_date",
            "journal",
            entry_context.journal_col.is_some(),
            &period,
        );
        let cutoff_condition = if entry_context.journal_col.is_some() {
            format!(
                "(parsed_date <= {} OR journal IN ('RAN', 'AN', 'OUV'))",
                sql_literal(&period.date_to)
            )
        } else {
            format!("parsed_date <= {}", sql_literal(&period.date_to))
        };

        let sql = format!(
            r#"WITH base AS (SELECT parsed_date = {parsed_date_expr}, journal = {journal_expr}, account_no = {account_no_expr}, account_label = {account_label_expr}, debit = {debit_expr}, credit = {credit_expr} FROM {entries_table} e {account_join} WHERE 1=1 {account_filter_sql_base}),
aggregated AS (SELECT account_no, MAX(account_label) AS account_label, SUM(CASE WHEN {opening_condition} THEN debit ELSE CAST(0 AS DECIMAL(38,6)) END) AS open_debit_raw, SUM(CASE WHEN {opening_condition} THEN credit ELSE CAST(0 AS DECIMAL(38,6)) END) AS open_credit_raw, SUM(CASE WHEN {movement_condition} THEN debit ELSE CAST(0 AS DECIMAL(38,6)) END) AS mvt_debit, SUM(CASE WHEN {movement_condition} THEN credit ELSE CAST(0 AS DECIMAL(38,6)) END) AS mvt_credit FROM base WHERE account_no <> '' AND parsed_date IS NOT NULL AND {cutoff_condition} GROUP BY account_no)
SELECT account_no, account_label, CASE WHEN (open_debit_raw-open_credit_raw)>=0 THEN (open_debit_raw-open_credit_raw) ELSE CAST(0 AS DECIMAL(38,6)) END AS ouverture_debit, CASE WHEN (open_debit_raw-open_credit_raw)<0 THEN ABS(open_debit_raw-open_credit_raw) ELSE CAST(0 AS DECIMAL(38,6)) END AS ouverture_credit, mvt_debit, mvt_credit, CASE WHEN ((open_debit_raw-open_credit_raw)+(mvt_debit-mvt_credit))>=0 THEN ((open_debit_raw-open_credit_raw)+(mvt_debit-mvt_credit)) ELSE CAST(0 AS DECIMAL(38,6)) END AS cloture_debit, CASE WHEN ((open_debit_raw-open_credit_raw)+(mvt_debit-mvt_credit))<0 THEN ABS((open_debit_raw-open_credit_raw)+(mvt_debit-mvt_credit)) ELSE CAST(0 AS DECIMAL(38,6)) END AS cloture_credit FROM aggregated ORDER BY account_no"#,
            parsed_date_expr = parsed_date_expr,
            journal_expr = journal_expr,
            account_no_expr = account_no_expr,
            account_label_expr = account_label_expr,
            debit_expr = entry_context.debit_expr,
            credit_expr = entry_context.credit_expr,
            entries_table = entry_context.table.sql_name(),
            account_join = account_join,
            account_filter_sql_base = format!("{}", account_filter_sql),
            opening_condition = opening_condition,
            movement_condition = movement_condition,
            cutoff_condition = cutoff_condition
        );

        let data = db::execute_logged_query(
            &mut client,
            &sql,
            db::QueryExecutionContext::new("stream_balance", "stream_balance")
                .with_table(entry_context.table.sql_name())
                .with_timeout_secs(timeout_secs),
        )
        .await?;
        let ix = column_index_map(&data);
        let rows = data
            .rows
            .iter()
            .map(|row| BalanceRow {
                account_no: row_string(row, &ix, "account_no").unwrap_or_default(),
                account_label: row_string(row, &ix, "account_label").unwrap_or_default(),
                ouverture_debit: row_decimal(row, &ix, "ouverture_debit"),
                ouverture_credit: row_decimal(row, &ix, "ouverture_credit"),
                mvt_debit: row_decimal(row, &ix, "mvt_debit"),
                mvt_credit: row_decimal(row, &ix, "mvt_credit"),
                cloture_debit: row_decimal(row, &ix, "cloture_debit"),
                cloture_credit: row_decimal(row, &ix, "cloture_credit"),
            })
            .collect();
        DashboardResponse {
            data: rows,
            warning: join_warnings(warnings),
        }
    };

    let all_rows = result.data;
    let total = all_rows.len() as i64;
    let _ = window.emit(
        "balance_total",
        StreamTotalPayload {
            total,
            warning: result.warning.clone(),
        },
    );

    let mut offset = 0usize;
    while offset < all_rows.len() {
        let end = (offset + STREAM_CHUNK_SIZE as usize).min(all_rows.len());
        let chunk = all_rows[offset..end].to_vec();
        let _ = window.emit("balance_chunk", StreamBalanceChunkPayload { rows: chunk });
        offset = end;
    }

    let _ = window.emit(
        "balance_complete",
        StreamCompletePayload {
            warning: result.warning,
        },
    );
    Ok(())
}

// ─── stream_grand_livre_auxiliaire ───────────────────────────────────────────

#[tauri::command]
pub async fn stream_grand_livre_auxiliaire(
    window: tauri::Window,
    state: State<'_, AppState>,
    id: String,
    date_from: String,
    date_to: String,
    tiers_type: String,
    account_prefix: Option<String>,
    preview_limit: Option<i64>,
) -> Result<(), String> {
    let result = do_stream_aux(
        &window,
        &state,
        &id,
        &date_from,
        &date_to,
        &tiers_type,
        account_prefix,
        preview_limit,
    )
    .await;
    if let Err(ref e) = result {
        let _ = window.emit("aux_error", e.clone());
    }
    result
}

async fn do_stream_aux(
    window: &tauri::Window,
    state: &AppState,
    id: &str,
    date_from: &str,
    date_to: &str,
    tiers_type: &str,
    account_prefix: Option<String>,
    preview_limit: Option<i64>,
) -> Result<(), String> {
    let normalized_type = tiers_type.trim().to_ascii_lowercase();
    let is_fournisseurs = normalized_type == "fournisseurs";
    if !is_fournisseurs && normalized_type != "clients" {
        return Err("tiers_type must be 'fournisseurs' or 'clients'".to_string());
    }

    let _config = state.resolve_connection_config(id)?;
    let hint_schema = state.get_connection_schema(id);
    let timeout_secs = dashboard_timeout_secs(state);
    let mut client = get_active_client(state, id).await?;

    if should_use_sage1000_analytics(&mut client, hint_schema.as_ref(), "stream_auxiliaire").await?
    {
        stream_aux_sage1000(
            window,
            &mut client,
            date_from,
            date_to,
            &normalized_type,
            account_prefix.as_deref(),
            timeout_secs,
            preview_limit,
        )
        .await
    } else {
        stream_aux_generic(
            window,
            &mut client,
            hint_schema.as_ref(),
            date_from,
            date_to,
            is_fournisseurs,
            account_prefix.as_deref(),
            timeout_secs,
            preview_limit,
        )
        .await
    }
}

async fn stream_aux_with_base_cte(
    window: &tauri::Window,
    client: &mut db::DbClient,
    base_cte: &str,
    order_by: &str,
    warning: Option<String>,
    timeout_secs: u64,
    preview_limit: Option<i64>,
) -> Result<(), String> {
    let count_sql = format!(
        "{} SELECT COUNT(*) AS total FROM base WHERE parsed_date IS NOT NULL",
        base_cte
    );
    let count_data = db::execute_logged_query(
        client,
        &count_sql,
        db::QueryExecutionContext::new(
            "stream_grand_livre_auxiliaire",
            "stream_grand_livre_auxiliaire_count",
        )
        .with_timeout_secs(timeout_secs),
    )
    .await?;
    let idx = column_index_map(&count_data);
    let total = count_data
        .rows
        .first()
        .map(|r| row_i64(r, &idx, "total"))
        .unwrap_or(0);

    let _ = window.emit(
        "aux_total",
        StreamTotalPayload {
            total,
            warning: warning.clone(),
        },
    );

    let mut running_balance: HashMap<String, Decimal> = HashMap::new();
    let mut offset = 0i64;

    let max_rows = preview_limit.filter(|limit| *limit > 0);

    loop {
        if max_rows.map(|limit| offset >= limit).unwrap_or(false) {
            break;
        }
        let fetch_size = max_rows
            .map(|limit| (limit - offset).min(STREAM_CHUNK_SIZE))
            .unwrap_or(STREAM_CHUNK_SIZE);
        let chunk_sql = format!(
            "{}\nSELECT account_no, account_label, tiers_code, tiers_name, CONVERT(char(10), parsed_date, 103) AS [date], journal, description, ref_piece, lettrage, debit, credit\nFROM base\nWHERE parsed_date IS NOT NULL\nORDER BY {}\nOFFSET {} ROWS FETCH NEXT {} ROWS ONLY",
            base_cte, order_by, offset, fetch_size
        );
        let data = db::execute_logged_query(
            client,
            &chunk_sql,
            db::QueryExecutionContext::new(
                "stream_grand_livre_auxiliaire",
                "stream_grand_livre_auxiliaire_chunk",
            )
            .with_timeout_secs(timeout_secs),
        )
        .await?;
        let ix = column_index_map(&data);
        let fetched = data.rows.len() as i64;

        let rows: Vec<AuxiliaireRow> = data
            .rows
            .iter()
            .map(|row| {
                let tiers_code = row_string(row, &ix, "tiers_code").unwrap_or_default();
                let debit = row_decimal(row, &ix, "debit");
                let credit = row_decimal(row, &ix, "credit");
                let bal = running_balance
                    .entry(tiers_code.clone())
                    .or_insert(Decimal::ZERO);
                *bal += debit - credit;
                AuxiliaireRow {
                    account_no: row_string(row, &ix, "account_no").unwrap_or_default(),
                    account_label: row_string(row, &ix, "account_label").unwrap_or_default(),
                    tiers_code,
                    tiers_name: row_string(row, &ix, "tiers_name").unwrap_or_default(),
                    date: row_string(row, &ix, "date"),
                    journal: row_string(row, &ix, "journal"),
                    description: row_string(row, &ix, "description"),
                    ref_piece: row_string(row, &ix, "ref_piece"),
                    lettrage: row_string(row, &ix, "lettrage"),
                    debit,
                    credit,
                    running_balance: *bal,
                    is_total_row: false,
                }
            })
            .collect();

        let _ = window.emit("aux_chunk", StreamAuxChunkPayload { rows });

        if fetched < fetch_size {
            break;
        }
        offset += fetched;
    }

    let _ = window.emit("aux_complete", StreamCompletePayload { warning });
    Ok(())
}

async fn stream_aux_sage1000(
    window: &tauri::Window,
    client: &mut db::DbClient,
    date_from: &str,
    date_to: &str,
    normalized_type: &str,
    account_prefix: Option<&str>,
    timeout_secs: u64,
    preview_limit: Option<i64>,
) -> Result<(), String> {
    let is_fournisseurs = normalized_type == "fournisseurs";
    let Some(context) = resolve_sage1000_context(client).await? else {
        let _ = window.emit(
            "aux_total",
            StreamTotalPayload {
                total: 0,
                warning: Some("Sage 1000 tables non détectées".to_string()),
            },
        );
        let _ = window.emit("aux_complete", StreamCompletePayload { warning: None });
        return Ok(());
    };

    let document_sql = sage1000_document_sql(&context);
    let tiers_join = match (context.role_tiers.as_ref(), context.tiers.as_ref()) {
        (Some(role_tiers), Some(tiers)) => {
            let tier_oid_expr = if context.pieces.is_some() {
                format!(
                    "COALESCE({}, {})",
                    qualify("rt", "oidTiers"),
                    qualify("p", "oidTiers")
                )
            } else {
                qualify("rt", "oidTiers")
            };
            format!(
                "LEFT JOIN {} rt ON {} = {}\n            LEFT JOIN {} t ON {} = {}",
                role_tiers.sql_name(),
                qualify("e", "oidroleTiers"),
                qualify("rt", "oid"),
                tiers.sql_name(),
                tier_oid_expr,
                qualify("t", "oid")
            )
        }
        (None, Some(tiers)) if context.pieces.is_some() => format!(
            "LEFT JOIN {} t ON {} = {}",
            tiers.sql_name(),
            qualify("p", "oidTiers"),
            qualify("t", "oid")
        ),
        _ => String::new(),
    };
    let role_account_join = context
        .role_tiers
        .as_ref()
        .filter(|_| context.tiers.is_some())
        .map(|_| {
            format!(
                "LEFT JOIN {} ap ON {} = {}",
                context.accounts.sql_name(),
                qualify("rt", "oidcomptePrivilegie"),
                qualify("ap", "oid")
            )
        })
        .unwrap_or_default();
    let account_no_expr = text_or_empty(&qualify("cg", "codeCompte"), 64);
    let account_label_expr = format!(
        "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
        qualify("cg", "Caption"),
        qualify("e", "reference")
    );
    let tiers_code_expr = context
        .tiers
        .as_ref()
        .map(|_| text_or_empty(&qualify("t", "code"), 64))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(64))".to_string());
    let tiers_name_expr = context
        .tiers
        .as_ref()
        .map(|_| {
            format!(
                "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
                qualify("t", "raisonSociale"),
                qualify("t", "nom"),
                qualify("t", "code")
            )
        })
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(255))".to_string());
    let lettrage_expr = text_or_empty(&qualify("e", "CodeLettrageExterne"), 64);
    let debit_expr = decimal_or_zero(&qualify("e", "debit"));
    let credit_expr = decimal_or_zero(&qualify("e", "credit"));
    let role_account_expr = text_or_empty(&qualify("ap", "codeCompte"), 64);
    let account_filter = if context.role_tiers.is_some() && context.tiers.is_some() {
        let role_prefix = if is_fournisseurs { "40" } else { "41" };
        format!(
            "(LEFT({role_account_expr}, 2) = {role_prefix} OR LEFT({account_no_expr}, 2) = {role_prefix})",
            role_account_expr = role_account_expr,
            account_no_expr = account_no_expr,
            role_prefix = sql_literal(role_prefix)
        )
    } else if is_fournisseurs {
        format!(
            "({} LIKE '40%' OR {} LIKE '401%')",
            account_no_expr, account_no_expr
        )
    } else {
        format!(
            "({} LIKE '41%' OR {} LIKE '411%')",
            account_no_expr, account_no_expr
        )
    };
    let prefix_filter = account_prefix
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            format!(
                " AND {} LIKE {}",
                account_no_expr,
                sql_literal(&format!("{}%", p.trim()))
            )
        })
        .unwrap_or_default();

    let base_cte = format!(
        "WITH base AS (SELECT parsed_date = TRY_CAST({date_col} AS DATE), account_no = {account_no_expr}, account_label = {account_label_expr}, tiers_code = {tiers_code_expr}, tiers_name = {tiers_name_expr}, journal = {journal_expr}, description = CAST('' AS NVARCHAR(255)), ref_piece = {ref_piece_expr}, lettrage = {lettrage_expr}, debit = {debit_expr}, credit = {credit_expr}, entry_number = COALESCE(TRY_CAST({entry_number_col} AS BIGINT), 0) FROM {entries_table} e LEFT JOIN {accounts_table} cg ON {entry_account_fk} = {account_pk} {document_join} {tiers_join} {role_account_join} WHERE TRY_CAST({date_col} AS DATE) BETWEEN {date_from} AND {date_to} AND {account_filter} AND {tiers_code_filter} <> '' {prefix_filter})",
        date_col = qualify("e", "eDate"),
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        tiers_code_expr = tiers_code_expr,
        tiers_name_expr = tiers_name_expr,
        journal_expr = document_sql.journal_expr,
        ref_piece_expr = document_sql.ref_piece_expr,
        lettrage_expr = lettrage_expr,
        debit_expr = debit_expr,
        credit_expr = credit_expr,
        entry_number_col = qualify("e", "numero"),
        entries_table = context.entries.sql_name(),
        accounts_table = context.accounts.sql_name(),
        entry_account_fk = qualify("e", "oidcompteGeneral"),
        account_pk = qualify("cg", "oid"),
        document_join = document_sql.join_sql,
        tiers_join = tiers_join,
        role_account_join = role_account_join,
        date_from = sql_literal(date_from),
        date_to = sql_literal(date_to),
        account_filter = account_filter,
        tiers_code_filter = tiers_code_expr,
        prefix_filter = prefix_filter
    );

    stream_aux_with_base_cte(
        window,
        client,
        &base_cte,
        "account_no, tiers_code, parsed_date, entry_number",
        None,
        timeout_secs,
        preview_limit,
    )
    .await
}

async fn stream_aux_generic(
    window: &tauri::Window,
    client: &mut db::DbClient,
    hint_schema: Option<&crate::sage_compat::SageSchema>,
    date_from: &str,
    date_to: &str,
    is_fournisseurs: bool,
    account_prefix: Option<&str>,
    timeout_secs: u64,
    preview_limit: Option<i64>,
) -> Result<(), String> {
    let (entry_context, mut warnings) = resolve_entry_context_smart(client, hint_schema).await?;

    let Some(entry_context) = entry_context else {
        let _ = window.emit(
            "aux_total",
            StreamTotalPayload {
                total: 0,
                warning: join_warnings(warnings),
            },
        );
        let _ = window.emit("aux_complete", StreamCompletePayload { warning: None });
        return Ok(());
    };

    let Some(tiers_code_col) = entry_context.tiers_code_col.clone() else {
        warnings.push("Colonne code tiers non détectée".to_string());
        let _ = window.emit(
            "aux_total",
            StreamTotalPayload {
                total: 0,
                warning: join_warnings(warnings),
            },
        );
        let _ = window.emit("aux_complete", StreamCompletePayload { warning: None });
        return Ok(());
    };

    let account_lookup = if let Some(schema) = hint_schema {
        if !schema.edition.is_generic() {
            schema_lookup_context(
                client,
                &schema.table_comptes,
                &schema.col_compte_num,
                &schema.col_compte_lib,
                &schema.col_compte_type,
            )
            .await?
            .or(resolve_lookup_context(
                client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?)
        } else {
            resolve_lookup_context(
                client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?
        }
    } else {
        resolve_lookup_context(
            client,
            ACCOUNT_TABLE_CANDIDATES,
            &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
            &[
                "CG_Intitule",
                "CT_Intitule",
                "LIBELLE_COMPTE",
                "ACCOUNT_LABEL",
                "ACCOUNT_NAME",
            ],
            &[],
        )
        .await?
    };
    let tiers_lookup = if let Some(schema) = hint_schema {
        if !schema.edition.is_generic() {
            schema_lookup_context(
                client,
                &schema.table_tiers,
                &schema.col_tiers_code,
                &schema.col_tiers_nom,
                &schema.col_tiers_type,
            )
            .await?
            .or(resolve_lookup_context(
                client,
                TIERS_TABLE_CANDIDATES,
                &[
                    "CT_Num",
                    "TIERS_CODE",
                    "CODE_TIERS",
                    "ACCOUNT_NO",
                    "CUSTOMER_NO",
                    "SUPPLIER_NO",
                ],
                &[
                    "CT_Intitule",
                    "TIERS_NAME",
                    "LIBELLE",
                    "NAME",
                    "ACCOUNT_NAME",
                    "CUSTOMER_NAME",
                    "SUPPLIER_NAME",
                ],
                &["CT_Type", "TIERS_TYPE", "ACCOUNT_TYPE", "TYPE_TIERS"],
            )
            .await?)
        } else {
            resolve_lookup_context(
                client,
                TIERS_TABLE_CANDIDATES,
                &[
                    "CT_Num",
                    "TIERS_CODE",
                    "CODE_TIERS",
                    "ACCOUNT_NO",
                    "CUSTOMER_NO",
                    "SUPPLIER_NO",
                ],
                &[
                    "CT_Intitule",
                    "TIERS_NAME",
                    "LIBELLE",
                    "NAME",
                    "ACCOUNT_NAME",
                    "CUSTOMER_NAME",
                    "SUPPLIER_NAME",
                ],
                &["CT_Type", "TIERS_TYPE", "ACCOUNT_TYPE", "TYPE_TIERS"],
            )
            .await?
        }
    } else {
        resolve_lookup_context(
            client,
            TIERS_TABLE_CANDIDATES,
            &[
                "CT_Num",
                "TIERS_CODE",
                "CODE_TIERS",
                "ACCOUNT_NO",
                "CUSTOMER_NO",
                "SUPPLIER_NO",
            ],
            &[
                "CT_Intitule",
                "TIERS_NAME",
                "LIBELLE",
                "NAME",
                "ACCOUNT_NAME",
                "CUSTOMER_NAME",
                "SUPPLIER_NAME",
            ],
            &["CT_Type", "TIERS_TYPE", "ACCOUNT_TYPE", "TYPE_TIERS"],
        )
        .await?
    };

    if tiers_lookup.is_none() {
        warnings.push("Table tiers non détectée; noms tiers seront vides".to_string());
    }

    let parsed_date_expr = format!(
        "TRY_CAST({} AS DATE)",
        qualify("e", &entry_context.date_col)
    );
    let journal_expr = entry_context
        .journal_col
        .as_ref()
        .map(|c| text_or_empty(&qualify("e", c), 64))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(64))".to_string());
    let account_no_expr = text_or_empty(&qualify("e", &entry_context.account_col), 64);
    let ref_piece_expr = entry_context
        .ref_piece_col
        .as_ref()
        .map(|c| text_or_empty(&qualify("e", c), 128))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(128))".to_string());
    let lettrage_expr = entry_context
        .lettrage_col
        .as_ref()
        .map(|c| text_or_empty(&qualify("e", c), 64))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(64))".to_string());
    let description_expr = entry_context
        .description_col
        .as_ref()
        .map(|c| text_or_empty(&qualify("e", c), 255))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(255))".to_string());
    let account_label_expr = match (&account_lookup, &entry_context.account_label_col) {
        (Some(lu), Some(el)) => format!(
            "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
            qualify("a", &lu.label_col), qualify("e", el)
        ),
        (Some(lu), None) => text_or_empty(&qualify("a", &lu.label_col), 255),
        (None, Some(el)) => text_or_empty(&qualify("e", el), 255),
        (None, None) => "CAST('' AS NVARCHAR(255))".to_string(),
    };
    let tiers_code_expr = text_or_empty(&qualify("e", &tiers_code_col), 64);
    let tiers_name_expr = tiers_lookup
        .as_ref()
        .map(|lu| text_or_empty(&qualify("t", &lu.label_col), 255))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(255))".to_string());
    let account_join = account_lookup
        .as_ref()
        .map(|lu| {
            format!(
                "LEFT JOIN {} a ON {} = {}",
                lu.table.sql_name(),
                qualify("a", &lu.code_col),
                qualify("e", &entry_context.account_col)
            )
        })
        .unwrap_or_default();
    let tiers_join = tiers_lookup
        .as_ref()
        .map(|lu| {
            format!(
                "LEFT JOIN {} t ON {} = {}",
                lu.table.sql_name(),
                qualify("t", &lu.code_col),
                qualify("e", &tiers_code_col)
            )
        })
        .unwrap_or_default();
    let default_account_filter = if is_fournisseurs {
        format!(
            "({} LIKE '40%' OR {} LIKE '401%')",
            account_no_expr, account_no_expr
        )
    } else {
        format!(
            "({} LIKE '41%' OR {} LIKE '411%')",
            account_no_expr, account_no_expr
        )
    };
    let type_filter = tiers_lookup
        .as_ref()
        .and_then(|lu| lu.type_col.as_ref())
        .map(|tc| {
            let tv = if is_fournisseurs { 1 } else { 0 };
            format!(
                "COALESCE(TRY_CAST({} AS INT), -1) = {}",
                qualify("t", tc),
                tv
            )
        });
    let kind_filter = type_filter
        .map(|tf| format!("({} OR {})", tf, default_account_filter))
        .unwrap_or(default_account_filter);
    let prefix_filter = account_prefix
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            format!(
                " AND {} LIKE {}",
                account_no_expr,
                sql_literal(&format!("{}%", p.trim()))
            )
        })
        .unwrap_or_default();

    let base_cte = format!(
        "WITH base AS (SELECT parsed_date = {parsed_date_expr}, account_no = {account_no_expr}, account_label = {account_label_expr}, tiers_code = {tiers_code_expr}, tiers_name = {tiers_name_expr}, journal = {journal_expr}, description = {description_expr}, ref_piece = {ref_piece_expr}, lettrage = {lettrage_expr}, debit = {debit_expr}, credit = {credit_expr} FROM {entries_table} e {account_join} {tiers_join} WHERE {parsed_date_expr} BETWEEN {date_from} AND {date_to} AND {kind_filter} AND {tiers_code_filter} <> '' {prefix_filter})",
        parsed_date_expr = parsed_date_expr,
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        tiers_code_expr = tiers_code_expr,
        tiers_name_expr = tiers_name_expr,
        journal_expr = journal_expr,
        description_expr = description_expr,
        ref_piece_expr = ref_piece_expr,
        lettrage_expr = lettrage_expr,
        debit_expr = entry_context.debit_expr,
        credit_expr = entry_context.credit_expr,
        entries_table = entry_context.table.sql_name(),
        account_join = account_join,
        tiers_join = tiers_join,
        date_from = sql_literal(date_from),
        date_to = sql_literal(date_to),
        kind_filter = kind_filter,
        tiers_code_filter = tiers_code_expr,
        prefix_filter = prefix_filter
    );

    stream_aux_with_base_cte(
        window,
        client,
        &base_cte,
        "account_no, tiers_code, parsed_date, journal, ref_piece, description",
        join_warnings(warnings),
        timeout_secs,
        preview_limit,
    )
    .await
}

#[tauri::command]
pub async fn get_dashboard_kpis(
    state: State<'_, AppState>,
    id: String,
    date_from: String,
    date_to: String,
    account_prefix: Option<String>,
) -> Result<DashboardResponse<DashboardKpis>, String> {
    let config = load_connection_config(&state, &id)?;
    let hint_schema = state.get_connection_schema(&id);
    let timeout_secs = dashboard_timeout_secs(state.inner());
    let mut client = get_active_client(state.inner(), &id).await?;
    if should_use_sage1000_analytics(&mut client, hint_schema.as_ref(), "get_dashboard_kpis")
        .await?
    {
        return get_dashboard_kpis_sage1000(
            &mut client,
            &date_from,
            &date_to,
            account_prefix.as_deref(),
            timeout_secs,
        )
        .await;
    }
    let (entry_context, mut warnings) =
        resolve_entry_context_smart(&mut client, hint_schema.as_ref()).await?;

    let Some(entry_context) = entry_context else {
        return Ok(empty_kpi_response(warnings));
    };

    let account_lookup = if let Some(ref schema) = hint_schema {
        if !schema.edition.is_generic() {
            schema_lookup_context(
                &mut client,
                &schema.table_comptes,
                &schema.col_compte_num,
                &schema.col_compte_lib,
                &schema.col_compte_type,
            )
            .await?
            .or(resolve_lookup_context(
                &mut client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?)
        } else {
            resolve_lookup_context(
                &mut client,
                ACCOUNT_TABLE_CANDIDATES,
                &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
                &[
                    "CG_Intitule",
                    "CT_Intitule",
                    "LIBELLE_COMPTE",
                    "ACCOUNT_LABEL",
                    "ACCOUNT_NAME",
                ],
                &[],
            )
            .await?
        }
    } else {
        resolve_lookup_context(
            &mut client,
            ACCOUNT_TABLE_CANDIDATES,
            &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"],
            &[
                "CG_Intitule",
                "CT_Intitule",
                "LIBELLE_COMPTE",
                "ACCOUNT_LABEL",
                "ACCOUNT_NAME",
            ],
            &[],
        )
        .await?
    };

    let parsed_date_expr = format!(
        "TRY_CAST({} AS DATE)",
        qualify("e", &entry_context.date_col)
    );
    let account_no_expr = text_or_empty(&qualify("e", &entry_context.account_col), 64);
    let account_label_expr = match (&account_lookup, &entry_context.account_label_col) {
        (Some(lookup), Some(entry_label)) => format!(
            "COALESCE(CAST({} AS NVARCHAR(255)), CAST({} AS NVARCHAR(255)), CAST('' AS NVARCHAR(255)))",
            qualify("a", &lookup.label_col),
            qualify("e", entry_label)
        ),
        (Some(lookup), None) => text_or_empty(&qualify("a", &lookup.label_col), 255),
        (None, Some(entry_label)) => text_or_empty(&qualify("e", entry_label), 255),
        (None, None) => "CAST('' AS NVARCHAR(255))".to_string(),
    };
    let tiers_code_expr = entry_context
        .tiers_code_col
        .as_ref()
        .map(|column| text_or_empty(&qualify("e", column), 64))
        .unwrap_or_else(|| "CAST('' AS NVARCHAR(64))".to_string());
    let account_join = account_lookup
        .as_ref()
        .map(|lookup| {
            format!(
                "LEFT JOIN {} a ON {} = {}",
                lookup.table.sql_name(),
                qualify("a", &lookup.code_col),
                qualify("e", &entry_context.account_col)
            )
        })
        .unwrap_or_default();
    let account_filter_sql = account_prefix
        .as_ref()
        .filter(|prefix| !prefix.trim().is_empty())
        .map(|prefix| {
            format!(
                " AND account_no LIKE {}",
                sql_literal(&format!("{}%", prefix.trim()))
            )
        })
        .unwrap_or_default();
    let base_cte = format!(
        r#"
        WITH base AS (
            SELECT
                parsed_date = {parsed_date_expr},
                account_no = {account_no_expr},
                account_label = {account_label_expr},
                tiers_code = {tiers_code_expr},
                debit = {debit_expr},
                credit = {credit_expr}
            FROM {entries_table} e
            {account_join}
        )
        "#,
        parsed_date_expr = parsed_date_expr,
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        tiers_code_expr = tiers_code_expr,
        debit_expr = entry_context.debit_expr,
        credit_expr = entry_context.credit_expr,
        entries_table = entry_context.table.sql_name(),
        account_join = account_join
    );

    let summary_sql = format!(
        r#"
        {base_cte}
        SELECT
            SUM(CASE WHEN parsed_date BETWEEN {date_from} AND {date_to} AND account_no LIKE '7%' THEN credit ELSE CAST(0 AS DECIMAL(38, 6)) END) AS total_ventes,
            SUM(CASE WHEN parsed_date BETWEEN {date_from} AND {date_to} AND account_no LIKE '6%' THEN debit ELSE CAST(0 AS DECIMAL(38, 6)) END) AS total_achats,
            SUM(CASE WHEN parsed_date <= {date_to} AND account_no LIKE '41%' THEN (debit - credit) ELSE CAST(0 AS DECIMAL(38, 6)) END) AS solde_clients,
            SUM(CASE WHEN parsed_date <= {date_to} AND account_no LIKE '40%' THEN (credit - debit) ELSE CAST(0 AS DECIMAL(38, 6)) END) AS solde_fournisseurs,
            SUM(CASE WHEN parsed_date BETWEEN {date_from} AND {date_to} THEN 1 ELSE 0 END) AS nb_ecritures,
            COUNT(DISTINCT CASE WHEN parsed_date BETWEEN {date_from} AND {date_to} THEN NULLIF(tiers_code, '') END) AS nb_tiers_actifs
        FROM base
        WHERE parsed_date IS NOT NULL
          AND account_no <> ''
          {account_filter_sql}
        "#,
        base_cte = base_cte,
        date_from = sql_literal(&date_from),
        date_to = sql_literal(&date_to),
        account_filter_sql = account_filter_sql
    );

    let top_charges_sql = format!(
        r#"
        {base_cte}
        SELECT TOP 5
            account_no AS account,
            MAX(account_label) AS label,
            SUM(debit) AS amount
        FROM base
        WHERE parsed_date BETWEEN {date_from} AND {date_to}
          AND account_no LIKE '6%'
          {account_filter_sql}
        GROUP BY account_no
        ORDER BY amount DESC, account_no
        "#,
        base_cte = base_cte,
        date_from = sql_literal(&date_from),
        date_to = sql_literal(&date_to),
        account_filter_sql = account_filter_sql
    );

    let top_produits_sql = format!(
        r#"
        {base_cte}
        SELECT TOP 5
            account_no AS account,
            MAX(account_label) AS label,
            SUM(credit) AS amount
        FROM base
        WHERE parsed_date BETWEEN {date_from} AND {date_to}
          AND account_no LIKE '7%'
          {account_filter_sql}
        GROUP BY account_no
        ORDER BY amount DESC, account_no
        "#,
        base_cte = base_cte,
        date_from = sql_literal(&date_from),
        date_to = sql_literal(&date_to),
        account_filter_sql = account_filter_sql
    );

    let evolution_sql = format!(
        r#"
        {base_cte}
        SELECT
            CONVERT(char(7), parsed_date, 120) AS [month],
            SUM(CASE WHEN account_no LIKE '7%' THEN credit ELSE CAST(0 AS DECIMAL(38, 6)) END) AS ventes,
            SUM(CASE WHEN account_no LIKE '6%' THEN debit ELSE CAST(0 AS DECIMAL(38, 6)) END) AS achats
        FROM base
        WHERE parsed_date BETWEEN {date_from} AND {date_to}
          AND account_no <> ''
          {account_filter_sql}
        GROUP BY CONVERT(char(7), parsed_date, 120)
        ORDER BY [month]
        "#,
        base_cte = base_cte,
        date_from = sql_literal(&date_from),
        date_to = sql_literal(&date_to),
        account_filter_sql = account_filter_sql
    );

    let summary = db::execute_logged_query(
        &mut client,
        &summary_sql,
        db::QueryExecutionContext::new("get_dashboard_kpis", "dashboard_kpi_summary")
            .with_connection_id(id.clone())
            .with_database(config.database.clone())
            .with_table(entry_context.table.sql_name())
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let top_charges = db::execute_logged_query(
        &mut client,
        &top_charges_sql,
        db::QueryExecutionContext::new("get_dashboard_kpis", "dashboard_kpi_top_charges")
            .with_connection_id(id.clone())
            .with_database(config.database.clone())
            .with_table(entry_context.table.sql_name())
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let top_produits = db::execute_logged_query(
        &mut client,
        &top_produits_sql,
        db::QueryExecutionContext::new("get_dashboard_kpis", "dashboard_kpi_top_products")
            .with_connection_id(id.clone())
            .with_database(config.database.clone())
            .with_table(entry_context.table.sql_name())
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let evolution = db::execute_logged_query(
        &mut client,
        &evolution_sql,
        db::QueryExecutionContext::new("get_dashboard_kpis", "dashboard_kpi_evolution")
            .with_connection_id(id.clone())
            .with_database(config.database.clone())
            .with_table(entry_context.table.sql_name())
            .with_timeout_secs(timeout_secs),
    )
    .await?;

    let summary_indexes = column_index_map(&summary);
    let top_charges_indexes = column_index_map(&top_charges);
    let top_produits_indexes = column_index_map(&top_produits);
    let evolution_indexes = column_index_map(&evolution);

    let summary_row = summary.rows.first();
    let kpis = DashboardKpis {
        total_ventes: summary_row
            .map(|row| row_decimal(row, &summary_indexes, "total_ventes"))
            .unwrap_or(Decimal::ZERO),
        total_achats: summary_row
            .map(|row| row_decimal(row, &summary_indexes, "total_achats"))
            .unwrap_or(Decimal::ZERO),
        solde_clients: summary_row
            .map(|row| row_decimal(row, &summary_indexes, "solde_clients"))
            .unwrap_or(Decimal::ZERO),
        solde_fournisseurs: summary_row
            .map(|row| row_decimal(row, &summary_indexes, "solde_fournisseurs"))
            .unwrap_or(Decimal::ZERO),
        nb_ecritures: summary_row
            .map(|row| row_i64(row, &summary_indexes, "nb_ecritures"))
            .unwrap_or(0),
        nb_tiers_actifs: summary_row
            .map(|row| row_i64(row, &summary_indexes, "nb_tiers_actifs"))
            .unwrap_or(0),
        top_charges: top_charges
            .rows
            .iter()
            .map(|row| DashboardTopItem {
                account: row_string(row, &top_charges_indexes, "account").unwrap_or_default(),
                label: row_string(row, &top_charges_indexes, "label").unwrap_or_default(),
                amount: row_decimal(row, &top_charges_indexes, "amount"),
            })
            .collect(),
        top_produits: top_produits
            .rows
            .iter()
            .map(|row| DashboardTopItem {
                account: row_string(row, &top_produits_indexes, "account").unwrap_or_default(),
                label: row_string(row, &top_produits_indexes, "label").unwrap_or_default(),
                amount: row_decimal(row, &top_produits_indexes, "amount"),
            })
            .collect(),
        evolution_mensuelle: evolution
            .rows
            .iter()
            .map(|row| MonthlyEvolutionPoint {
                month: row_string(row, &evolution_indexes, "month").unwrap_or_default(),
                ventes: row_decimal(row, &evolution_indexes, "ventes"),
                achats: row_decimal(row, &evolution_indexes, "achats"),
            })
            .collect(),
    };

    if entry_context.tiers_code_col.is_none() {
        warnings.push(
            "No tiers code column detected; active tiers count is approximate and may be zero"
                .to_string(),
        );
    }

    Ok(DashboardResponse {
        data: kpis,
        warning: join_warnings(warnings),
    })
}

#[tauri::command]
pub async fn get_dashboard_tiers(
    state: State<'_, AppState>,
    id: String,
    tiers_type: Option<String>,
    search: Option<String>,
) -> Result<DashboardResponse<Vec<DashboardTiersRow>>, String> {
    let config = load_connection_config(&state, &id)?;
    let timeout_secs = dashboard_timeout_secs(state.inner());
    let mut client = get_active_client(state.inner(), &id).await?;
    let context =
        sage_entity_service::resolve_entity_search_context(&state, &id, &mut client).await?;
    let Some(mapping) = context.tiers else {
        return Ok(empty_vec_response(vec![
            "No supported Sage tiers table was detected".to_string(),
        ]));
    };

    let code_expr = text_or_empty(&sage_entity_service::qualify("t", &mapping.col_code), 100);
    let name_expr = text_or_empty(&sage_entity_service::qualify("t", &mapping.col_nom), 255);
    let normalized_type = tiers_type
        .unwrap_or_else(|| "all".to_string())
        .trim()
        .to_ascii_lowercase();
    let type_filter = match normalized_type.as_str() {
        "clients" => " AND tiers_type IN (N'client', N'les_deux')",
        "fournisseurs" => " AND tiers_type IN (N'fournisseur', N'les_deux')",
        _ => "",
    };
    let search_filter = search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            let pattern = sql_literal(&format!("%{}%", value));
            format!(" AND (code LIKE {pattern} OR name LIKE {pattern})")
        })
        .unwrap_or_default();

    let use_sage1000 = should_use_sage1000_analytics(
        &mut client,
        state.get_connection_schema(&id).as_ref(),
        "get_dashboard_tiers",
    )
    .await?;
    let mut warnings = Vec::new();

    let source_type_expr = if use_sage1000 {
        // Sage 1000's typePersonne describes the legal-person kind, not whether
        // the tier is a customer or supplier. TROLETIERS/account prefixes are
        // authoritative for that distinction.
        "N'unknown'".to_string()
    } else {
        mapping
            .col_type
            .as_ref()
            .map(|column| {
                let value = sage_entity_service::qualify("t", column);
                let numeric_cases = if column.eq_ignore_ascii_case("CT_Type") {
                    format!(
                        "WHEN TRY_CAST({value} AS INT) = 0 THEN N'client' \
                         WHEN TRY_CAST({value} AS INT) = 1 THEN N'fournisseur' \
                         WHEN TRY_CAST({value} AS INT) = 2 THEN N'les_deux'"
                    )
                } else {
                    String::new()
                };
                format!(
                    "CASE {numeric_cases} \
                     WHEN UPPER(LTRIM(RTRIM(CAST({value} AS NVARCHAR(64))))) IN (N'CLIENT', N'CUSTOMER', N'CLIENTS', N'C') THEN N'client' \
                     WHEN UPPER(LTRIM(RTRIM(CAST({value} AS NVARCHAR(64))))) IN (N'FOURNISSEUR', N'SUPPLIER', N'VENDOR', N'FOURNISSEURS', N'F') THEN N'fournisseur' \
                     WHEN UPPER(LTRIM(RTRIM(CAST({value} AS NVARCHAR(64))))) IN (N'LES_DEUX', N'BOTH', N'MIXTE') THEN N'les_deux' \
                     ELSE N'unknown' END"
                )
            })
            .unwrap_or_else(|| "N'unknown'".to_string())
    };

    let finance_cte = if use_sage1000 {
        let Some(sage) = resolve_sage1000_context(&mut client).await? else {
            return Ok(empty_vec_response(vec![
                "Sage 1000 accounting tables were not detected".to_string(),
            ]));
        };
        match sage.role_tiers.as_ref() {
            Some(roles) => format!(
                r#"
                tier_finance AS (
                    SELECT
                        tier_id = {role_tier_fk},
                        client_account = MAX(CASE WHEN LEFT({effective_account}, 2) = '41' THEN {effective_account} END),
                        supplier_account = MAX(CASE WHEN LEFT({effective_account}, 2) = '40' THEN {effective_account} END),
                        has_client = MAX(CASE WHEN LEFT({effective_account}, 2) = '41' THEN 1 ELSE 0 END),
                        has_supplier = MAX(CASE WHEN LEFT({effective_account}, 2) = '40' THEN 1 ELSE 0 END),
                        debit = SUM(CASE WHEN LEFT({effective_account}, 2) IN ('40', '41') THEN {debit} ELSE CAST(0 AS DECIMAL(38, 6)) END),
                        credit = SUM(CASE WHEN LEFT({effective_account}, 2) IN ('40', '41') THEN {credit} ELSE CAST(0 AS DECIMAL(38, 6)) END),
                        last_movement_date = MAX(CASE WHEN LEFT({effective_account}, 2) IN ('40', '41') THEN TRY_CAST({entry_date} AS DATE) END)
                    FROM {roles_table} rt
                    LEFT JOIN {accounts_table} ap ON {role_account_fk} = {account_pk_ap}
                    LEFT JOIN {entries_table} e ON {entry_role_fk} = {role_pk}
                    LEFT JOIN {accounts_table} cg ON {entry_account_fk} = {account_pk_cg}
                    GROUP BY {role_tier_fk}
                )
                "#,
                role_tier_fk = qualify("rt", "oidTiers"),
                effective_account = format!(
                    "COALESCE(NULLIF({}, ''), {})",
                    text_or_empty(&qualify("ap", "codeCompte"), 64),
                    text_or_empty(&qualify("cg", "codeCompte"), 64),
                ),
                debit = decimal_or_zero(&qualify("e", "debit")),
                credit = decimal_or_zero(&qualify("e", "credit")),
                entry_date = qualify("e", "eDate"),
                roles_table = roles.sql_name(),
                accounts_table = sage.accounts.sql_name(),
                entries_table = sage.entries.sql_name(),
                role_account_fk = qualify("rt", "oidcomptePrivilegie"),
                account_pk_ap = qualify("ap", "oid"),
                entry_role_fk = qualify("e", "oidroleTiers"),
                role_pk = qualify("rt", "oid"),
                entry_account_fk = qualify("e", "oidcompteGeneral"),
                account_pk_cg = qualify("cg", "oid"),
            ),
            None => {
                warnings.push("TROLETIERS was not detected; customer/supplier roles and balances are unavailable".to_string());
                r#"
                tier_finance AS (
                    SELECT
                        tier_id = CAST(NULL AS BIGINT),
                        client_account = CAST(NULL AS NVARCHAR(64)),
                        supplier_account = CAST(NULL AS NVARCHAR(64)),
                        has_client = 0, has_supplier = 0,
                        debit = CAST(NULL AS DECIMAL(38, 6)),
                        credit = CAST(NULL AS DECIMAL(38, 6)),
                        last_movement_date = CAST(NULL AS DATE)
                    WHERE 1 = 0
                )
                "#
                .to_string()
            }
        }
    } else {
        let (auto_context, mut detection_warnings) = resolve_entry_context(&mut client).await?;
        warnings.append(&mut detection_warnings);
        let entry_context = if auto_context.is_some() {
            auto_context
        } else {
            let (fallback, mut fallback_warnings) =
                resolve_entry_context_smart(&mut client, state.get_connection_schema(&id).as_ref())
                    .await?;
            warnings.append(&mut fallback_warnings);
            fallback
        };
        match entry_context {
            Some(entries) if entries.tiers_code_col.is_some() => {
                let tiers_column = entries.tiers_code_col.as_ref().expect("checked above");
                let account = text_or_empty(&qualify("e", &entries.account_col), 64);
                let tiers_code = text_or_empty(&qualify("e", tiers_column), 100);
                format!(
                    r#"
                    movement_base AS (
                        SELECT
                            tiers_code = {tiers_code},
                            account_no = {account},
                            parsed_date = TRY_CAST({entry_date} AS DATE),
                            debit = {debit},
                            credit = {credit}
                        FROM {entries_table} e
                        WHERE LEFT({account}, 2) IN ('40', '41')
                    ),
                    tier_finance AS (
                        SELECT
                            tier_id = tiers_code,
                            client_account = MAX(CASE WHEN LEFT(account_no, 2) = '41' THEN account_no END),
                            supplier_account = MAX(CASE WHEN LEFT(account_no, 2) = '40' THEN account_no END),
                            has_client = MAX(CASE WHEN LEFT(account_no, 2) = '41' THEN 1 ELSE 0 END),
                            has_supplier = MAX(CASE WHEN LEFT(account_no, 2) = '40' THEN 1 ELSE 0 END),
                            debit = SUM(debit),
                            credit = SUM(credit),
                            last_movement_date = MAX(parsed_date)
                        FROM movement_base
                        WHERE tiers_code <> ''
                        GROUP BY tiers_code
                    )
                    "#,
                    tiers_code = tiers_code,
                    account = account,
                    entry_date = qualify("e", &entries.date_col),
                    debit = entries.debit_expr,
                    credit = entries.credit_expr,
                    entries_table = entries.table.sql_name(),
                )
            }
            _ => {
                warnings.push("No compatible tier code was detected on accounting entries; movement balances are unavailable".to_string());
                r#"
                tier_finance AS (
                    SELECT
                        tier_id = CAST(NULL AS NVARCHAR(100)),
                        client_account = CAST(NULL AS NVARCHAR(64)),
                        supplier_account = CAST(NULL AS NVARCHAR(64)),
                        has_client = 0, has_supplier = 0,
                        debit = CAST(NULL AS DECIMAL(38, 6)),
                        credit = CAST(NULL AS DECIMAL(38, 6)),
                        last_movement_date = CAST(NULL AS DATE)
                    WHERE 1 = 0
                )
                "#
                .to_string()
            }
        }
    };

    let tier_join =
        "TRY_CAST(tf.tier_id AS NVARCHAR(100)) = TRY_CAST(ts.source_id AS NVARCHAR(100))";
    let sql = format!(
        r#"
        WITH tiers_source AS (
            SELECT
                source_id = {source_id},
                code = {code_expr},
                name = {name_expr},
                source_type = {source_type_expr}
            FROM {table} t
        ),
        {finance_cte},
        enriched AS (
            SELECT
                ts.code,
                ts.name,
                tiers_type = CASE
                    WHEN COALESCE(tf.has_client, 0) = 1 AND COALESCE(tf.has_supplier, 0) = 1 THEN N'les_deux'
                    WHEN ts.source_type <> N'unknown' THEN ts.source_type
                    WHEN COALESCE(tf.has_client, 0) = 1 THEN N'client'
                    WHEN COALESCE(tf.has_supplier, 0) = 1 THEN N'fournisseur'
                    ELSE N'unknown'
                END,
                account_no = CASE
                    WHEN tf.client_account IS NOT NULL AND tf.supplier_account IS NOT NULL
                        THEN CONCAT(tf.client_account, N' / ', tf.supplier_account)
                    ELSE COALESCE(tf.client_account, tf.supplier_account)
                END,
                tf.debit,
                tf.credit,
                balance = CASE WHEN tf.debit IS NULL AND tf.credit IS NULL THEN NULL ELSE COALESCE(tf.debit, 0) - COALESCE(tf.credit, 0) END,
                tf.last_movement_date
            FROM tiers_source ts
            LEFT JOIN tier_finance tf ON {tier_join}
        )
        SELECT TOP (100)
            code,
            name,
            tiers_type,
            account_no,
            debit,
            credit,
            balance,
            CONVERT(char(10), last_movement_date, 23) AS last_movement_date
        FROM enriched
        WHERE code <> ''
          {type_filter}
          {search_filter}
        ORDER BY name, code
        "#,
        source_id = sage_entity_service::qualify("t", &mapping.col_id),
        table = mapping.table.quoted_name(),
        source_type_expr = source_type_expr,
        finance_cte = finance_cte,
        tier_join = tier_join,
    );
    let data = db::execute_logged_query(
        &mut client,
        &sql,
        db::QueryExecutionContext::new("get_dashboard_tiers", "dashboard_tiers_read_only")
            .with_connection_id(id)
            .with_database(config.database)
            .with_table(mapping.table.quoted_name())
            .with_timeout_secs(timeout_secs),
    )
    .await?;
    let indexes = column_index_map(&data);
    let rows = data
        .rows
        .iter()
        .map(|row| DashboardTiersRow {
            code: row_string(row, &indexes, "code").unwrap_or_default(),
            name: row_string(row, &indexes, "name").unwrap_or_default(),
            tiers_type: row_string(row, &indexes, "tiers_type")
                .unwrap_or_else(|| "unknown".to_string()),
            account_no: row_string(row, &indexes, "account_no"),
            debit: row_optional_decimal(row, &indexes, "debit"),
            credit: row_optional_decimal(row, &indexes, "credit"),
            balance: row_optional_decimal(row, &indexes, "balance"),
            last_movement_date: row_string(row, &indexes, "last_movement_date"),
        })
        .collect();

    Ok(DashboardResponse {
        data: rows,
        warning: join_warnings(warnings),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sage1000_route_uses_schema_hint_first() {
        let schema = SageSchema::for_edition(&SageEdition::Sage1000);

        assert_eq!(
            sage1000_route_source(Some(&schema), false),
            Some("schema_hint")
        );
    }

    #[test]
    fn sage1000_route_uses_database_probe_without_hint() {
        assert_eq!(sage1000_route_source(None, true), Some("database_probe"));
    }

    #[test]
    fn sage1000_route_uses_database_probe_for_generic_hint() {
        let schema = SageSchema::for_edition(&SageEdition::Generic);

        assert_eq!(
            sage1000_route_source(Some(&schema), true),
            Some("database_probe")
        );
    }

    #[test]
    fn sage1000_route_falls_back_when_no_hint_or_tables() {
        assert_eq!(sage1000_route_source(None, false), None);
    }
}
