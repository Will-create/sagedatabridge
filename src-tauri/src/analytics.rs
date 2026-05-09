use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use tauri::State;

use crate::db;
use crate::sage_compat::{SageEdition, SageSchema};
use crate::state::{AppState, ConnectionConfig, TableData};

const ENTRY_TABLE_CANDIDATES: &[&str] = &["F_ECRITUREC", "ECRITUREC", "GL_ENTRIES", "JOURNAL_ENTRIES"];
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

fn load_connection_config(state: &State<'_, AppState>, id: &str) -> Result<ConnectionConfig, String> {
    state.resolve_connection_config(id)
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
        Some(Value::Number(number)) => Decimal::from_str(&number.to_string()).unwrap_or(Decimal::ZERO),
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

fn row_i64(row: &[Value], indexes: &HashMap<String, usize>, name: &str) -> i64 {
    indexes
        .get(&name.to_ascii_lowercase())
        .map(|index| value_as_i64(row.get(*index)))
        .unwrap_or(0)
}

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
            let schema = row_string(row, &indexes, "schema_name").unwrap_or_else(|| "dbo".to_string());
            let name = row_string(row, &indexes, "table_name").unwrap_or_else(|| candidate.to_string());
            return Some(DetectedTable { schema, name });
        }
    }

    None
}

async fn detect_columns(client: &mut db::DbClient, table: &DetectedTable) -> Result<ColumnMap, String> {
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

async fn resolve_sage1000_context(client: &mut db::DbClient) -> Result<Option<Sage1000Context>, String> {
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
    }))
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
    let date_col = pick_column(&columns, &["EC_Date", "DATE_ECR", "TRS_DATE", "ENTRY_DATE", "JM_Date", "DATE"]);
    let account_col = pick_column(&columns, &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"]);
    let journal_col = pick_column(&columns, &["JO_Num", "CODE_JOURNAL", "JRN_CODE", "JOURNAL_CODE"]);
    let account_label_col =
        pick_column(&columns, &["CG_Intitule", "CT_Intitule", "LIBELLE_COMPTE", "ACCOUNT_LABEL", "ACCOUNT_NAME"]);
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
        &["EC_Intitule", "EC_Libelle", "LIBELLE", "DESCRIPTION", "ENTRY_LABEL", "NARRATION"],
    );
    let ref_piece_col =
        pick_column(&columns, &["EC_RefPiece", "EC_Piece", "REF_PIECE", "PIECE_NO", "REFERENCE"]);
    let lettrage_col = pick_column(&columns, &["EC_Lettrage", "LETTRAGE", "MATCHING_CODE"]);
    let amount_exprs = resolve_amount_exprs("e", &columns);

    if date_col.is_none() {
        warnings.push(format!("{} is missing a supported date column", table.sql_name()));
    }
    if account_col.is_none() {
        warnings.push(format!("{} is missing a supported account column", table.sql_name()));
    }
    if amount_exprs.is_none() {
        warnings.push(format!("{} is missing supported debit/credit columns", table.sql_name()));
    }

    let context = match (date_col, account_col, amount_exprs) {
        (Some(date_col), Some(account_col), Some((debit_expr, credit_expr))) => Some(EntryContext {
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
        }),
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
    if schema.table_ecritures.is_empty() || schema.col_date.is_empty() || schema.col_compte.is_empty() {
        return Ok(None);
    }

    let Some(table) = detect_table(client, &[schema.table_ecritures.as_str()]).await else {
        return Ok(None);
    };

    let debit_expr = if schema.col_debit.trim_start().to_uppercase().starts_with("CASE") {
        decimal_or_zero(&schema.col_debit)
    } else {
        decimal_or_zero(&qualify("e", &schema.col_debit))
    };
    let credit_expr = if schema.col_credit.trim_start().to_uppercase().starts_with("CASE") {
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
        type_col: if type_col.is_empty() { None } else { Some(type_col.to_string()) },
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
) -> Result<DashboardResponse<Vec<GrandLivreRow>>, String> {
    let Some(context) = resolve_sage1000_context(client).await? else {
        return Ok(empty_vec_response(vec![
            "Sage 1000 tables TECRITURE and TCOMPTEGENERAL were not detected".to_string(),
        ]));
    };

    let piece_join = context
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
        .unwrap_or_default();
    let journal_expr = if context.pieces.is_some() {
        text_or_empty(&qualify("p", "numero"), 64)
    } else {
        "CAST('' AS NVARCHAR(64))".to_string()
    };
    let ref_piece_expr = if context.pieces.is_some() {
        text_or_empty(&qualify("p", "numero"), 128)
    } else {
        "CAST('' AS NVARCHAR(128))".to_string()
    };
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
        .map(|prefix| format!(" AND {} LIKE {}", account_no_expr, sql_literal(&format!("{}%", prefix.trim()))))
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
            {piece_join}
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
        journal_expr = journal_expr,
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        ref_piece_expr = ref_piece_expr,
        lettrage_expr = lettrage_expr,
        debit_expr = debit_expr,
        credit_expr = credit_expr,
        entry_number_col = qualify("e", "numero"),
        entries_table = context.entries.sql_name(),
        accounts_table = context.accounts.sql_name(),
        entry_account_fk = qualify("e", "oidcompteGeneral"),
        account_pk = qualify("cg", "oid"),
        piece_join = piece_join,
        date_from = sql_literal(date_from),
        date_to = sql_literal(date_to),
        account_filter_sql = account_filter_sql
    );

    let data = db::execute_raw_query(client, &sql).await?;
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

async fn get_balance_sage1000(
    client: &mut db::DbClient,
    date_from: &str,
    date_to: &str,
    account_prefix: Option<&str>,
) -> Result<DashboardResponse<Vec<BalanceRow>>, String> {
    let Some(context) = resolve_sage1000_context(client).await? else {
        return Ok(empty_vec_response(vec![
            "Sage 1000 tables TECRITURE and TCOMPTEGENERAL were not detected".to_string(),
        ]));
    };

    let account_no_expr = text_or_empty(&qualify("cg", "codeCompte"), 64);
    let account_label_expr = text_or_empty(&qualify("cg", "Caption"), 255);
    let opening_debit_expr = decimal_or_zero(&qualify("cg", "soldeDO"));
    let opening_credit_expr = decimal_or_zero(&qualify("cg", "soldeDV"));
    let movement_debit_expr = format!(
        "COALESCE(SUM(CASE WHEN TRY_CAST({} AS DATE) BETWEEN {} AND {} THEN {} ELSE CAST(0 AS DECIMAL(38, 6)) END), CAST(0 AS DECIMAL(38, 6)))",
        qualify("e", "eDate"),
        sql_literal(date_from),
        sql_literal(date_to),
        decimal_or_zero(&qualify("e", "debit"))
    );
    let movement_credit_expr = format!(
        "COALESCE(SUM(CASE WHEN TRY_CAST({} AS DATE) BETWEEN {} AND {} THEN {} ELSE CAST(0 AS DECIMAL(38, 6)) END), CAST(0 AS DECIMAL(38, 6)))",
        qualify("e", "eDate"),
        sql_literal(date_from),
        sql_literal(date_to),
        decimal_or_zero(&qualify("e", "credit"))
    );
    let account_filter_sql = account_prefix
        .filter(|prefix| !prefix.trim().is_empty())
        .map(|prefix| format!(" AND {} LIKE {}", account_no_expr, sql_literal(&format!("{}%", prefix.trim()))))
        .unwrap_or_default();

    let sql = format!(
        r#"
        SELECT
            account_no = {account_no_expr},
            account_label = {account_label_expr},
            ouverture_debit = {opening_debit_expr},
            ouverture_credit = {opening_credit_expr},
            mvt_debit = {movement_debit_expr},
            mvt_credit = {movement_credit_expr},
            cloture_debit = {opening_debit_expr} + {movement_debit_expr},
            cloture_credit = {opening_credit_expr} + {movement_credit_expr}
        FROM {accounts_table} cg
        LEFT JOIN {entries_table} e ON {entry_account_fk} = {account_pk}
        WHERE COALESCE(TRY_CAST({is_active_col} AS INT), 1) = 1
          AND {account_no_expr} <> ''
          {account_filter_sql}
        GROUP BY
            {account_pk},
            {account_no_col},
            {account_label_col},
            {opening_debit_col},
            {opening_credit_col}
        ORDER BY account_no
        "#,
        account_no_expr = account_no_expr,
        account_label_expr = account_label_expr,
        opening_debit_expr = opening_debit_expr,
        opening_credit_expr = opening_credit_expr,
        movement_debit_expr = movement_debit_expr,
        movement_credit_expr = movement_credit_expr,
        accounts_table = context.accounts.sql_name(),
        entries_table = context.entries.sql_name(),
        entry_account_fk = qualify("e", "oidcompteGeneral"),
        account_pk = qualify("cg", "oid"),
        is_active_col = qualify("cg", "enActivite"),
        account_filter_sql = account_filter_sql,
        account_no_col = qualify("cg", "codeCompte"),
        account_label_col = qualify("cg", "Caption"),
        opening_debit_col = qualify("cg", "soldeDO"),
        opening_credit_col = qualify("cg", "soldeDV")
    );

    let data = db::execute_raw_query(client, &sql).await?;
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
    let piece_join = context
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
        .unwrap_or_default();
    let tier_oid_expr = if context.pieces.is_some() {
        format!("COALESCE({}, {})", qualify("rt", "oidTiers"), qualify("p", "oidTiers"))
    } else {
        qualify("rt", "oidTiers")
    };
    let journal_expr = if context.pieces.is_some() {
        text_or_empty(&qualify("p", "numero"), 64)
    } else {
        "CAST('' AS NVARCHAR(64))".to_string()
    };
    let ref_piece_expr = if context.pieces.is_some() {
        text_or_empty(&qualify("p", "numero"), 128)
    } else {
        "CAST('' AS NVARCHAR(128))".to_string()
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
        .map(|prefix| format!(" AND {} LIKE {}", account_no_expr, sql_literal(&format!("{}%", prefix.trim()))))
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
            {piece_join}
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
        journal_expr = journal_expr,
        description_expr = description_expr,
        ref_piece_expr = ref_piece_expr,
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
        piece_join = piece_join,
        date_from = sql_literal(date_from),
        date_to = sql_literal(date_to),
        tier_kind_filter = tier_kind_filter,
        account_filter_sql = account_filter_sql
    );

    let data = db::execute_raw_query(client, &sql).await?;
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
) -> Result<DashboardResponse<DashboardKpis>, String> {
    let Some(context) = resolve_sage1000_context(client).await? else {
        return Ok(empty_kpi_response(vec![
            "Sage 1000 tables TECRITURE and TCOMPTEGENERAL were not detected".to_string(),
        ]));
    };

    let piece_join = context
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
        .unwrap_or_default();
    let tiers_join = match (context.role_tiers.as_ref(), context.tiers.as_ref()) {
        (Some(role_tiers), Some(tiers)) => {
            let tier_oid_expr = if context.pieces.is_some() {
                format!("COALESCE({}, {})", qualify("rt", "oidTiers"), qualify("p", "oidTiers"))
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
        .map(|prefix| format!(" AND account_no LIKE {}", sql_literal(&format!("{}%", prefix.trim()))))
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

    let summary = db::execute_raw_query(client, &summary_sql).await?;
    let top_charges = db::execute_raw_query(client, &top_charges_sql).await?;
    let top_produits = db::execute_raw_query(client, &top_produits_sql).await?;
    let evolution = db::execute_raw_query(client, &evolution_sql).await?;

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
    let mut client = db::connect(&config).await?;
    if is_sage1000_schema(hint_schema.as_ref()) {
        return get_grand_livre_sage1000(&mut client, &date_from, &date_to, account_prefix.as_deref()).await;
    }
    let (entry_context, mut warnings) = resolve_entry_context_smart(&mut client, hint_schema.as_ref()).await?;

    let Some(entry_context) = entry_context else {
        return Ok(empty_vec_response(warnings));
    };

    let account_lookup = if let Some(ref schema) = hint_schema {
        if !schema.edition.is_generic() {
            schema_lookup_context(&mut client, &schema.table_comptes, &schema.col_compte_num, &schema.col_compte_lib, &schema.col_compte_type).await?
                .or(resolve_lookup_context(&mut client, ACCOUNT_TABLE_CANDIDATES, &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"], &["CG_Intitule", "CT_Intitule", "LIBELLE_COMPTE", "ACCOUNT_LABEL", "ACCOUNT_NAME"], &[]).await?)
        } else {
            resolve_lookup_context(&mut client, ACCOUNT_TABLE_CANDIDATES, &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"], &["CG_Intitule", "CT_Intitule", "LIBELLE_COMPTE", "ACCOUNT_LABEL", "ACCOUNT_NAME"], &[]).await?
        }
    } else {
        resolve_lookup_context(&mut client, ACCOUNT_TABLE_CANDIDATES, &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"], &["CG_Intitule", "CT_Intitule", "LIBELLE_COMPTE", "ACCOUNT_LABEL", "ACCOUNT_NAME"], &[]).await?
    };

    if account_lookup.is_none() && entry_context.account_label_col.is_none() {
        warnings.push("Account labels table not detected; labels will be blank when not present on entries".to_string());
    }

    let parsed_date_expr = format!("TRY_CAST({} AS DATE)", qualify("e", &entry_context.date_col));
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
        .map(|prefix| format!(" AND {} LIKE {}", account_no_expr, sql_literal(&format!("{}%", prefix.trim()))))
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

    let data = db::execute_raw_query(&mut client, &sql).await?;
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
    let config = load_connection_config(&state, &id)?;
    let hint_schema = state.get_connection_schema(&id);
    let mut client = db::connect(&config).await?;
    if is_sage1000_schema(hint_schema.as_ref()) {
        return get_balance_sage1000(&mut client, &date_from, &date_to, account_prefix.as_deref()).await;
    }
    let (entry_context, mut warnings) = resolve_entry_context_smart(&mut client, hint_schema.as_ref()).await?;

    let Some(entry_context) = entry_context else {
        return Ok(empty_vec_response(warnings));
    };

    let account_lookup = if let Some(ref schema) = hint_schema {
        if !schema.edition.is_generic() {
            schema_lookup_context(&mut client, &schema.table_comptes, &schema.col_compte_num, &schema.col_compte_lib, &schema.col_compte_type).await?
                .or(resolve_lookup_context(&mut client, ACCOUNT_TABLE_CANDIDATES, &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"], &["CG_Intitule", "CT_Intitule", "LIBELLE_COMPTE", "ACCOUNT_LABEL", "ACCOUNT_NAME"], &[]).await?)
        } else {
            resolve_lookup_context(&mut client, ACCOUNT_TABLE_CANDIDATES, &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"], &["CG_Intitule", "CT_Intitule", "LIBELLE_COMPTE", "ACCOUNT_LABEL", "ACCOUNT_NAME"], &[]).await?
        }
    } else {
        resolve_lookup_context(&mut client, ACCOUNT_TABLE_CANDIDATES, &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"], &["CG_Intitule", "CT_Intitule", "LIBELLE_COMPTE", "ACCOUNT_LABEL", "ACCOUNT_NAME"], &[]).await?
    };

    if entry_context.journal_col.is_none() {
        warnings.push("Journal column not detected; opening balance falls back to entries before the start date only".to_string());
    }

    let parsed_date_expr = format!("TRY_CAST({} AS DATE)", qualify("e", &entry_context.date_col));
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
        .map(|prefix| format!(" AND {} LIKE {}", account_no_expr, sql_literal(&format!("{}%", prefix.trim()))))
        .unwrap_or_default();
    let opening_condition = if entry_context.journal_col.is_some() {
        format!(
            "(journal IN ('RAN', 'AN', 'OUV') OR parsed_date < {})",
            sql_literal(&date_from)
        )
    } else {
        format!("parsed_date < {}", sql_literal(&date_from))
    };
    let cutoff_condition = if entry_context.journal_col.is_some() {
        format!(
            "(parsed_date <= {} OR journal IN ('RAN', 'AN', 'OUV'))",
            sql_literal(&date_to)
        )
    } else {
        format!("parsed_date <= {}", sql_literal(&date_to))
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
                SUM(CASE WHEN parsed_date BETWEEN {date_from} AND {date_to} THEN debit ELSE CAST(0 AS DECIMAL(38, 6)) END) AS mvt_debit,
                SUM(CASE WHEN parsed_date BETWEEN {date_from} AND {date_to} THEN credit ELSE CAST(0 AS DECIMAL(38, 6)) END) AS mvt_credit
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
        date_from = sql_literal(&date_from),
        date_to = sql_literal(&date_to),
        cutoff_condition = cutoff_condition
    );

    let data = db::execute_raw_query(&mut client, &sql).await?;
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
    let mut client = db::connect(&config).await?;
    if is_sage1000_schema(hint_schema.as_ref()) {
        return get_grand_livre_auxiliaire_sage1000(
            &mut client,
            &date_from,
            &date_to,
            &normalized_type,
            account_prefix.as_deref(),
        )
        .await;
    }
    let (entry_context, mut warnings) = resolve_entry_context_smart(&mut client, hint_schema.as_ref()).await?;

    let Some(entry_context) = entry_context else {
        return Ok(empty_vec_response(warnings));
    };

    let Some(tiers_code_col) = entry_context.tiers_code_col.clone() else {
        warnings.push("No tiers code column detected on accounting entries".to_string());
        return Ok(empty_vec_response(warnings));
    };

    let account_lookup = if let Some(ref schema) = hint_schema {
        if !schema.edition.is_generic() {
            schema_lookup_context(&mut client, &schema.table_comptes, &schema.col_compte_num, &schema.col_compte_lib, &schema.col_compte_type).await?
                .or(resolve_lookup_context(&mut client, ACCOUNT_TABLE_CANDIDATES, &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"], &["CG_Intitule", "CT_Intitule", "LIBELLE_COMPTE", "ACCOUNT_LABEL", "ACCOUNT_NAME"], &[]).await?)
        } else {
            resolve_lookup_context(&mut client, ACCOUNT_TABLE_CANDIDATES, &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"], &["CG_Intitule", "CT_Intitule", "LIBELLE_COMPTE", "ACCOUNT_LABEL", "ACCOUNT_NAME"], &[]).await?
        }
    } else {
        resolve_lookup_context(&mut client, ACCOUNT_TABLE_CANDIDATES, &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"], &["CG_Intitule", "CT_Intitule", "LIBELLE_COMPTE", "ACCOUNT_LABEL", "ACCOUNT_NAME"], &[]).await?
    };
    let tiers_lookup = if let Some(ref schema) = hint_schema {
        if !schema.edition.is_generic() {
            schema_lookup_context(&mut client, &schema.table_tiers, &schema.col_tiers_code, &schema.col_tiers_nom, &schema.col_tiers_type).await?
                .or(resolve_lookup_context(&mut client, TIERS_TABLE_CANDIDATES, &["CT_Num", "TIERS_CODE", "CODE_TIERS", "ACCOUNT_NO", "CUSTOMER_NO", "SUPPLIER_NO"], &["CT_Intitule", "TIERS_NAME", "LIBELLE", "NAME", "ACCOUNT_NAME", "CUSTOMER_NAME", "SUPPLIER_NAME"], &["CT_Type", "TIERS_TYPE", "ACCOUNT_TYPE", "TYPE_TIERS"]).await?)
        } else {
            resolve_lookup_context(&mut client, TIERS_TABLE_CANDIDATES, &["CT_Num", "TIERS_CODE", "CODE_TIERS", "ACCOUNT_NO", "CUSTOMER_NO", "SUPPLIER_NO"], &["CT_Intitule", "TIERS_NAME", "LIBELLE", "NAME", "ACCOUNT_NAME", "CUSTOMER_NAME", "SUPPLIER_NAME"], &["CT_Type", "TIERS_TYPE", "ACCOUNT_TYPE", "TYPE_TIERS"]).await?
        }
    } else {
        resolve_lookup_context(&mut client, TIERS_TABLE_CANDIDATES, &["CT_Num", "TIERS_CODE", "CODE_TIERS", "ACCOUNT_NO", "CUSTOMER_NO", "SUPPLIER_NO"], &["CT_Intitule", "TIERS_NAME", "LIBELLE", "NAME", "ACCOUNT_NAME", "CUSTOMER_NAME", "SUPPLIER_NAME"], &["CT_Type", "TIERS_TYPE", "ACCOUNT_TYPE", "TYPE_TIERS"]).await?
    };

    if tiers_lookup.is_none() {
        warnings.push("No tiers lookup table detected; tiers names will be blank".to_string());
    }

    let parsed_date_expr = format!("TRY_CAST({} AS DATE)", qualify("e", &entry_context.date_col));
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
        format!("({account_no_expr} LIKE '40%' OR {account_no_expr} LIKE '401%')", account_no_expr = account_no_expr)
    } else {
        format!("({account_no_expr} LIKE '41%' OR {account_no_expr} LIKE '411%')", account_no_expr = account_no_expr)
    };
    let type_filter = tiers_lookup
        .as_ref()
        .and_then(|lookup| lookup.type_col.as_ref())
        .map(|type_col| {
            let type_value = if is_fournisseurs { 1 } else { 0 };
            format!("COALESCE(TRY_CAST({} AS INT), -1) = {}", qualify("t", type_col), type_value)
        });
    let base_kind_filter = type_filter
        .map(|type_filter| format!("({} OR {})", type_filter, default_account_filter))
        .unwrap_or(default_account_filter);
    let scoped_prefix_filter = account_prefix
        .as_ref()
        .filter(|prefix| !prefix.trim().is_empty())
        .map(|prefix| format!(" AND {} LIKE {}", account_no_expr, sql_literal(&format!("{}%", prefix.trim()))))
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

    let data = db::execute_raw_query(&mut client, &sql).await?;
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
    let mut client = db::connect(&config).await?;
    if is_sage1000_schema(hint_schema.as_ref()) {
        return get_dashboard_kpis_sage1000(&mut client, &date_from, &date_to, account_prefix.as_deref()).await;
    }
    let (entry_context, mut warnings) = resolve_entry_context_smart(&mut client, hint_schema.as_ref()).await?;

    let Some(entry_context) = entry_context else {
        return Ok(empty_kpi_response(warnings));
    };

    let account_lookup = if let Some(ref schema) = hint_schema {
        if !schema.edition.is_generic() {
            schema_lookup_context(&mut client, &schema.table_comptes, &schema.col_compte_num, &schema.col_compte_lib, &schema.col_compte_type).await?
                .or(resolve_lookup_context(&mut client, ACCOUNT_TABLE_CANDIDATES, &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"], &["CG_Intitule", "CT_Intitule", "LIBELLE_COMPTE", "ACCOUNT_LABEL", "ACCOUNT_NAME"], &[]).await?)
        } else {
            resolve_lookup_context(&mut client, ACCOUNT_TABLE_CANDIDATES, &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"], &["CG_Intitule", "CT_Intitule", "LIBELLE_COMPTE", "ACCOUNT_LABEL", "ACCOUNT_NAME"], &[]).await?
        }
    } else {
        resolve_lookup_context(&mut client, ACCOUNT_TABLE_CANDIDATES, &["CG_Num", "CT_Num", "COMPTE", "ACCOUNT_NO"], &["CG_Intitule", "CT_Intitule", "LIBELLE_COMPTE", "ACCOUNT_LABEL", "ACCOUNT_NAME"], &[]).await?
    };

    let parsed_date_expr = format!("TRY_CAST({} AS DATE)", qualify("e", &entry_context.date_col));
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
        .map(|prefix| format!(" AND account_no LIKE {}", sql_literal(&format!("{}%", prefix.trim()))))
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

    let summary = db::execute_raw_query(&mut client, &summary_sql).await?;
    let top_charges = db::execute_raw_query(&mut client, &top_charges_sql).await?;
    let top_produits = db::execute_raw_query(&mut client, &top_produits_sql).await?;
    let evolution = db::execute_raw_query(&mut client, &evolution_sql).await?;

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
        warnings.push("No tiers code column detected; active tiers count is approximate and may be zero".to_string());
    }

    Ok(DashboardResponse {
        data: kpis,
        warning: join_warnings(warnings),
    })
}
