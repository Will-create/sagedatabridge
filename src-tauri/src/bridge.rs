use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;
use uuid::Uuid;

use crate::db;
use crate::invoice_commands::InvoiceHeader;
use crate::invoice_compat::InvoiceSchema;
use crate::sage_compat::{SageEdition, SageSchema};
use crate::sage_entity_service::{
    self, parse_f64, parse_string, round2, sql_opt_string, sql_string,
};
use crate::state::AppState;

const SCHEMA: &str = "sdb";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BridgeDocumentSummary {
    pub id: String,
    pub document_type: String,
    pub number: String,
    pub status: String,
    pub document_date: String,
    pub due_date: String,
    pub partner_code: String,
    pub partner_name: String,
    pub currency: String,
    pub total_ht: f64,
    pub total_tax: f64,
    pub total_ttc: f64,
    pub source_document_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PostingLine {
    pub line_no: i32,
    pub role: String,
    pub general_account: String,
    pub auxiliary_account: String,
    pub label: String,
    pub debit: f64,
    pub credit: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PostingBatch {
    pub id: String,
    pub source_kind: String,
    pub source_id: String,
    pub journal_code: String,
    pub piece_number: String,
    pub posting_date: String,
    pub status: String,
    pub total_debit: f64,
    pub total_credit: f64,
    pub lines: Vec<PostingLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PaymentInput {
    pub invoice_id: String,
    pub payment_date: String,
    pub amount: f64,
    pub currency: String,
    pub payment_method_code: String,
    pub reference: String,
    pub journal_code: String,
    pub debit_account: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountingConfiguration {
    pub partner_code: String,
    pub partner_account: String,
    pub product_code: String,
    pub product_family_code: String,
    pub accounting_category_code: String,
    pub revenue_account: String,
    pub expense_account: String,
    pub tax_rate: f64,
    pub tax_account: String,
    pub payment_method_code: String,
    pub payment_journal_code: String,
    pub payment_debit_account: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CatalogItem {
    pub id: String,
    pub code: String,
    pub name: String,
    pub item_type: String,
    pub source: String,
    pub active: bool,
    pub parent_code: String,
    pub account_code: String,
    pub rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProductAccountingProfile {
    pub product_code: String,
    pub product_family_code: String,
    pub accounting_category_code: String,
    pub tax_code: String,
    #[serde(default)]
    pub revenue_account: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BridgeMasterData {
    pub accounts: Vec<CatalogItem>,
    pub journals: Vec<CatalogItem>,
    pub product_families: Vec<CatalogItem>,
    pub accounting_categories: Vec<CatalogItem>,
    pub tax_codes: Vec<CatalogItem>,
    pub payment_methods: Vec<CatalogItem>,
    pub product_profiles: Vec<ProductAccountingProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountingPreviewLine {
    pub product_code: String,
    pub product_family_code: String,
    pub accounting_category_code: String,
    pub tax_code: String,
    pub revenue_account: String,
    pub tax_account: String,
    pub ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountingReadiness {
    pub ready: bool,
    pub journal_code: String,
    pub customer_account: String,
    pub schema_code: String,
    pub issues: Vec<String>,
    pub lines: Vec<AccountingPreviewLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BridgeManagementData {
    pub dashboard: Value,
    pub partner_profiles: Vec<Value>,
    pub accounting_schemas: Vec<Value>,
    pub context_mappings: Vec<Value>,
    pub document_journal_mappings: Vec<Value>,
    pub integration_profiles: Vec<Value>,
    pub export_jobs: Vec<Value>,
}

#[derive(Debug, Clone, Default)]
struct ResolvedDocumentLine {
    line_id: String,
    line_no: i32,
    revenue_account: String,
    tax_account: String,
    amount_ht: f64,
    tax_amount: f64,
}

#[derive(Debug, Clone, Default)]
struct ResolvedDocumentContext {
    schema_id: String,
    schema_code: String,
    journal_code: String,
    partner_account: String,
    lines: Vec<ResolvedDocumentLine>,
}

trait AccountingAdapter: Send + Sync {
    fn adapter_type(&self) -> &'static str;
    fn validate(&self, batch: &PostingBatch) -> Result<(), String>;
    fn build_package(&self, batch: &PostingBatch) -> Value;
}

struct NeutralAdapter;
struct Sage100Adapter;
struct Sage1000Adapter;

fn validate_canonical_batch(batch: &PostingBatch) -> Result<(), String> {
    if batch.lines.is_empty() {
        return Err("Posting batch has no lines".to_string());
    }
    if (batch.total_debit - batch.total_credit).abs() > 0.005 {
        return Err("Posting batch is not balanced".to_string());
    }
    if batch
        .lines
        .iter()
        .any(|line| line.general_account.trim().is_empty())
    {
        return Err("Every posting line requires a general account".to_string());
    }
    Ok(())
}

impl AccountingAdapter for NeutralAdapter {
    fn adapter_type(&self) -> &'static str {
        "neutral"
    }

    fn validate(&self, batch: &PostingBatch) -> Result<(), String> {
        validate_canonical_batch(batch)
    }

    fn build_package(&self, batch: &PostingBatch) -> Value {
        json!({ "canonical_batch": batch })
    }
}

impl AccountingAdapter for Sage100Adapter {
    fn adapter_type(&self) -> &'static str {
        "sage100"
    }

    fn validate(&self, batch: &PostingBatch) -> Result<(), String> {
        validate_canonical_batch(batch)?;
        if batch.journal_code.trim().is_empty() || batch.piece_number.trim().is_empty() {
            return Err("Sage 100 packages require journal and piece numbers".to_string());
        }
        Ok(())
    }

    fn build_package(&self, batch: &PostingBatch) -> Value {
        json!({
            "format": "sage100-neutral-v1",
            "delivery_enabled": false,
            "field_candidates": {
                "date": "EC_Date",
                "journal": "JO_Num",
                "account": "CG_Num",
                "auxiliary_account": "CT_Num",
                "label": "EC_Intitule",
                "piece": "EC_Piece",
                "reference": "EC_RefPiece",
                "amount": "EC_Montant",
                "direction": "EC_Sens"
            },
            "canonical_batch": batch
        })
    }
}

impl AccountingAdapter for Sage1000Adapter {
    fn adapter_type(&self) -> &'static str {
        "sage1000"
    }

    fn validate(&self, batch: &PostingBatch) -> Result<(), String> {
        validate_canonical_batch(batch)?;
        if batch.piece_number.trim().is_empty() {
            return Err("Sage 1000 packages require a piece number".to_string());
        }
        Ok(())
    }

    fn build_package(&self, batch: &PostingBatch) -> Value {
        json!({
            "format": "sage1000-neutral-v1",
            "delivery_enabled": false,
            "field_candidates": {
                "date": "TECRITURE.eDate",
                "general_account": "TECRITURE.oidcompteGeneral",
                "auxiliary_account": "TECRITURE.oidroleTiers",
                "piece": "TPIECE.numero",
                "reference": "TPIECE.reference",
                "debit": "TECRITURE.debit",
                "credit": "TECRITURE.credit",
                "journal": "UNKNOWN"
            },
            "canonical_batch": batch
        })
    }
}

fn adapter_for(adapter_type: &str) -> Box<dyn AccountingAdapter> {
    match adapter_type.trim().to_ascii_lowercase().as_str() {
        "sage100" => Box::new(Sage100Adapter),
        "sage1000" => Box::new(Sage1000Adapter),
        _ => Box::new(NeutralAdapter),
    }
}

fn sql_number(value: f64) -> String {
    format!("{:.2}", round2(value))
}

fn value_string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn value_bool(value: &Value, key: &str, default: bool) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn value_i64(value: &Value, key: &str, default: i64) -> i64 {
    value.get(key).and_then(Value::as_i64).unwrap_or(default)
}

fn value_f64(value: &Value, key: &str, default: f64) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(default)
}

fn non_empty_or(value: String, fallback: String) -> String {
    if value.is_empty() {
        fallback
    } else {
        value
    }
}

fn tax_code_for_rate(rate: f64) -> String {
    let rounded = round2(rate);
    if (rounded.fract()).abs() < 0.005 {
        format!("TVA{:.0}", rounded)
    } else {
        format!("TVA{:.2}", rounded).replace('.', "_")
    }
}

fn looks_like_revenue_account(code: &str) -> bool {
    let trimmed = code.trim();
    trimmed.len() >= 6 && trimmed.starts_with('7') && trimmed.chars().all(|ch| ch.is_ascii_digit())
}

async fn query_rows_as_values(client: &mut db::DbClient, sql: &str) -> Result<Vec<Value>, String> {
    let data = db::execute_raw_query(client, sql).await?;
    Ok(data
        .rows
        .iter()
        .map(|row| Value::Array(row.clone()))
        .collect())
}

async fn client(state: &State<'_, AppState>, id: &str) -> Result<db::DbClient, String> {
    db::get_cached_client_snapshot(state.inner(), id).await
}

fn catalog_item(row: &[Value], item_type: &str, source: &str) -> CatalogItem {
    CatalogItem {
        id: parse_string(row.first()),
        code: parse_string(row.get(1)),
        name: parse_string(row.get(2)),
        item_type: item_type.to_string(),
        source: source.to_string(),
        active: true,
        parent_code: parse_string(row.get(3)),
        account_code: parse_string(row.get(4)),
        rate: parse_f64(row.get(5)),
    }
}

async fn native_catalog(
    state: &State<'_, AppState>,
    client: &mut db::DbClient,
    id: &str,
    catalog_type: &str,
) -> Result<Vec<CatalogItem>, String> {
    let connection = state.resolve_connection_config(id)?;
    let edition = state
        .get_connection_schema(id)
        .map(|schema| schema.edition)
        .unwrap_or_else(|| SageEdition::from_edition_str(&connection.sage_edition));
    let accounting = SageSchema::for_edition(&edition);
    let commercial = InvoiceSchema::for_edition(&edition);
    let tables = db::get_tables(client).await?;
    let candidates = if catalog_type == "account" {
        vec![accounting.table_comptes.clone()]
    } else {
        vec![
            commercial.table_journal.clone(),
            accounting.table_journaux.clone(),
            "TJOURNAL".to_string(),
            "F_JOURNAL".to_string(),
        ]
    };
    let table = match sage_entity_service::load_table(client, &tables, &candidates).await {
        Ok(table) => table,
        Err(_) => return Ok(Vec::new()),
    };
    let (code, name, kind) = if catalog_type == "account" {
        (
            sage_entity_service::pick_required(
                &table,
                &[
                    &accounting.col_compte_num,
                    "codeCompte",
                    "CG_Num",
                    "CT_Num",
                    "ACC",
                ],
            )?,
            sage_entity_service::pick_optional(
                &table,
                &[
                    &accounting.col_compte_lib,
                    "Caption",
                    "CG_Intitule",
                    "CT_Intitule",
                    "DES",
                ],
            ),
            sage_entity_service::pick_optional(
                &table,
                &[
                    &accounting.col_compte_type,
                    "typeCompte",
                    "CG_Type",
                    "CT_Type",
                ],
            ),
        )
    } else {
        (
            sage_entity_service::pick_required(
                &table,
                &["code", "JO_Num", "codeJournal", "numero", "oid"],
            )?,
            sage_entity_service::pick_optional(
                &table,
                &["Caption", "JO_Intitule", "libelle", "intitule", "code"],
            ),
            sage_entity_service::pick_optional(
                &table,
                &["JO_Type", "typeJournal", "type", "nature"],
            ),
        )
    };
    let name_expr = name
        .map(|column| sage_entity_service::br(&column))
        .unwrap_or_else(|| sage_entity_service::br(&code));
    let kind_expr = kind
        .map(|column| sage_entity_service::br(&column))
        .unwrap_or_else(|| "N''".to_string());
    let sql = format!(
        "SELECT TOP (500) CAST({code} AS NVARCHAR(100)),CAST({code} AS NVARCHAR(100)),CAST({name} AS NVARCHAR(255)),CAST({kind} AS NVARCHAR(100)),N'',0 FROM {table} WHERE CAST({code} AS NVARCHAR(100))<>N'' ORDER BY CAST({code} AS NVARCHAR(100))",
        code = sage_entity_service::br(&code),
        name = name_expr,
        kind = kind_expr,
        table = table.quoted_name(),
    );
    let data = db::execute_raw_query(client, &sql).await?;
    Ok(data
        .rows
        .iter()
        .map(|row| catalog_item(row, catalog_type, accounting.edition.as_str()))
        .collect())
}

async fn ensure_schema(client: &mut db::DbClient) -> Result<(), String> {
    let sql = format!(
        r#"
IF SCHEMA_ID(N'{schema}') IS NULL EXEC(N'CREATE SCHEMA [{schema}]');

IF OBJECT_ID(N'{schema}.schema_version', N'U') IS NULL
CREATE TABLE [{schema}].[schema_version] (
    [version] INT NOT NULL PRIMARY KEY,
    [applied_at] DATETIME2 NOT NULL DEFAULT SYSUTCDATETIME()
);

IF OBJECT_ID(N'{schema}.document_number_sequence', N'U') IS NULL
CREATE TABLE [{schema}].[document_number_sequence] (
    [sequence_key] NVARCHAR(150) NOT NULL PRIMARY KEY,
    [next_value] BIGINT NOT NULL,
    [updated_at] DATETIME2 NOT NULL DEFAULT SYSUTCDATETIME()
);

IF OBJECT_ID(N'{schema}.commercial_document', N'U') IS NULL
CREATE TABLE [{schema}].[commercial_document] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [document_type] NVARCHAR(30) NOT NULL,
    [official_number] NVARCHAR(100) NOT NULL,
    [status] NVARCHAR(30) NOT NULL DEFAULT N'Draft',
    [source_document_id] NVARCHAR(50) NULL,
    [partner_id] NVARCHAR(100) NULL,
    [partner_code] NVARCHAR(100) NULL,
    [partner_name] NVARCHAR(255) NULL,
    [partner_account] NVARCHAR(100) NULL,
    [partner_category_code] NVARCHAR(100) NULL,
    [resolved_schema_id] NVARCHAR(50) NULL,
    [resolved_journal_code] NVARCHAR(50) NULL,
    [document_date] DATE NOT NULL,
    [due_date] DATE NULL,
    [reference] NVARCHAR(255) NULL,
    [currency] NVARCHAR(10) NOT NULL,
    [total_ht] DECIMAL(19,4) NOT NULL DEFAULT 0,
    [total_tax] DECIMAL(19,4) NOT NULL DEFAULT 0,
    [total_ttc] DECIMAL(19,4) NOT NULL DEFAULT 0,
    [notes] NVARCHAR(MAX) NULL,
    [conditions] NVARCHAR(MAX) NULL,
    [validated_at] DATETIME2 NULL,
    [validated_by] NVARCHAR(100) NULL,
    [lock_version] INT NOT NULL DEFAULT 1,
    [created_at] DATETIME2 NOT NULL DEFAULT SYSUTCDATETIME(),
    [updated_at] DATETIME2 NOT NULL DEFAULT SYSUTCDATETIME(),
    CONSTRAINT [UQ_sdb_document_number] UNIQUE ([document_type], [official_number])
);
IF COL_LENGTH(N'{schema}.commercial_document', N'partner_category_code') IS NULL ALTER TABLE [{schema}].[commercial_document] ADD [partner_category_code] NVARCHAR(100) NULL;
IF COL_LENGTH(N'{schema}.commercial_document', N'resolved_schema_id') IS NULL ALTER TABLE [{schema}].[commercial_document] ADD [resolved_schema_id] NVARCHAR(50) NULL;
IF COL_LENGTH(N'{schema}.commercial_document', N'resolved_journal_code') IS NULL ALTER TABLE [{schema}].[commercial_document] ADD [resolved_journal_code] NVARCHAR(50) NULL;

IF OBJECT_ID(N'{schema}.commercial_document_line', N'U') IS NULL
CREATE TABLE [{schema}].[commercial_document_line] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [document_id] NVARCHAR(50) NOT NULL,
    [line_no] INT NOT NULL,
    [product_id] NVARCHAR(100) NULL,
    [product_code] NVARCHAR(100) NULL,
    [product_family_code] NVARCHAR(100) NULL,
    [accounting_category_code] NVARCHAR(100) NULL,
    [tax_code] NVARCHAR(100) NULL,
    [description] NVARCHAR(500) NULL,
    [quantity] DECIMAL(19,6) NOT NULL DEFAULT 0,
    [unit] NVARCHAR(50) NULL,
    [unit_price_ht] DECIMAL(19,6) NOT NULL DEFAULT 0,
    [amount_ht] DECIMAL(19,4) NOT NULL DEFAULT 0,
    [tax_rate] DECIMAL(9,4) NOT NULL DEFAULT 0,
    [tax_amount] DECIMAL(19,4) NOT NULL DEFAULT 0,
    [amount_ttc] DECIMAL(19,4) NOT NULL DEFAULT 0,
    [revenue_account] NVARCHAR(100) NULL,
    [expense_account] NVARCHAR(100) NULL,
    [tax_account] NVARCHAR(100) NULL,
    [resolved_revenue_account] NVARCHAR(100) NULL,
    [resolved_expense_account] NVARCHAR(100) NULL,
    [resolved_tax_account] NVARCHAR(100) NULL,
    [resolution_json] NVARCHAR(MAX) NULL,
    CONSTRAINT [FK_sdb_document_line_document] FOREIGN KEY ([document_id])
        REFERENCES [{schema}].[commercial_document]([id]),
    CONSTRAINT [UQ_sdb_document_line] UNIQUE ([document_id], [line_no])
);
IF COL_LENGTH(N'{schema}.commercial_document_line', N'product_family_code') IS NULL ALTER TABLE [{schema}].[commercial_document_line] ADD [product_family_code] NVARCHAR(100) NULL;
IF COL_LENGTH(N'{schema}.commercial_document_line', N'accounting_category_code') IS NULL ALTER TABLE [{schema}].[commercial_document_line] ADD [accounting_category_code] NVARCHAR(100) NULL;
IF COL_LENGTH(N'{schema}.commercial_document_line', N'tax_code') IS NULL ALTER TABLE [{schema}].[commercial_document_line] ADD [tax_code] NVARCHAR(100) NULL;
IF COL_LENGTH(N'{schema}.commercial_document_line', N'resolved_revenue_account') IS NULL ALTER TABLE [{schema}].[commercial_document_line] ADD [resolved_revenue_account] NVARCHAR(100) NULL;
IF COL_LENGTH(N'{schema}.commercial_document_line', N'resolved_expense_account') IS NULL ALTER TABLE [{schema}].[commercial_document_line] ADD [resolved_expense_account] NVARCHAR(100) NULL;
IF COL_LENGTH(N'{schema}.commercial_document_line', N'resolved_tax_account') IS NULL ALTER TABLE [{schema}].[commercial_document_line] ADD [resolved_tax_account] NVARCHAR(100) NULL;
IF COL_LENGTH(N'{schema}.commercial_document_line', N'resolution_json') IS NULL ALTER TABLE [{schema}].[commercial_document_line] ADD [resolution_json] NVARCHAR(MAX) NULL;

IF OBJECT_ID(N'{schema}.document_status_history', N'U') IS NULL
CREATE TABLE [{schema}].[document_status_history] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [document_id] NVARCHAR(50) NOT NULL,
    [from_status] NVARCHAR(30) NULL,
    [to_status] NVARCHAR(30) NOT NULL,
    [changed_by] NVARCHAR(100) NULL,
    [changed_at] DATETIME2 NOT NULL DEFAULT SYSUTCDATETIME()
);

IF OBJECT_ID(N'{schema}.document_relation', N'U') IS NULL
CREATE TABLE [{schema}].[document_relation] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [source_document_id] NVARCHAR(50) NOT NULL,
    [target_document_id] NVARCHAR(50) NOT NULL,
    [relation_type] NVARCHAR(50) NOT NULL,
    [created_at] DATETIME2 NOT NULL DEFAULT SYSUTCDATETIME(),
    CONSTRAINT [UQ_sdb_document_relation] UNIQUE ([source_document_id], [target_document_id], [relation_type])
);

IF OBJECT_ID(N'{schema}.journal', N'U') IS NULL
CREATE TABLE [{schema}].[journal] (
    [code] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [name] NVARCHAR(255) NOT NULL,
    [journal_type] NVARCHAR(30) NOT NULL,
    [active] BIT NOT NULL DEFAULT 1
);

IF NOT EXISTS (SELECT 1 FROM [{schema}].[journal] WHERE [code] = N'VT')
INSERT INTO [{schema}].[journal] ([code], [name], [journal_type]) VALUES (N'VT', N'Ventes', N'sales');
IF NOT EXISTS (SELECT 1 FROM [{schema}].[journal] WHERE [code] = N'BQ')
INSERT INTO [{schema}].[journal] ([code], [name], [journal_type]) VALUES (N'BQ', N'Banque', N'payment');

IF OBJECT_ID(N'{schema}.document_journal_mapping', N'U') IS NULL
CREATE TABLE [{schema}].[document_journal_mapping] (
    [document_type] NVARCHAR(30) NOT NULL PRIMARY KEY,
    [journal_code] NVARCHAR(50) NOT NULL
);
IF NOT EXISTS (SELECT 1 FROM [{schema}].[document_journal_mapping] WHERE [document_type] = N'Facture')
INSERT INTO [{schema}].[document_journal_mapping] VALUES (N'Facture', N'VT');

IF OBJECT_ID(N'{schema}.accounting_category', N'U') IS NULL
CREATE TABLE [{schema}].[accounting_category] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [code] NVARCHAR(100) NOT NULL UNIQUE,
    [name] NVARCHAR(255) NOT NULL,
    [revenue_account] NVARCHAR(100) NULL,
    [expense_account] NVARCHAR(100) NULL,
    [tax_account] NVARCHAR(100) NULL,
    [active] BIT NOT NULL DEFAULT 1
);

IF OBJECT_ID(N'{schema}.product_family', N'U') IS NULL
CREATE TABLE [{schema}].[product_family] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [code] NVARCHAR(100) NOT NULL UNIQUE,
    [name] NVARCHAR(255) NOT NULL,
    [accounting_category_id] NVARCHAR(50) NULL,
    [revenue_account] NVARCHAR(100) NULL,
    [expense_account] NVARCHAR(100) NULL,
    [tax_account] NVARCHAR(100) NULL,
    [active] BIT NOT NULL DEFAULT 1
);

IF OBJECT_ID(N'{schema}.accounting_rule', N'U') IS NULL
CREATE TABLE [{schema}].[accounting_rule] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [rule_scope] NVARCHAR(50) NOT NULL,
    [scope_key] NVARCHAR(100) NOT NULL,
    [account_role] NVARCHAR(50) NOT NULL,
    [account_code] NVARCHAR(100) NOT NULL,
    [priority] INT NOT NULL DEFAULT 100,
    [active] BIT NOT NULL DEFAULT 1,
    CONSTRAINT [UQ_sdb_accounting_rule] UNIQUE ([rule_scope], [scope_key], [account_role])
);

IF OBJECT_ID(N'{schema}.product_accounting_profile', N'U') IS NULL
CREATE TABLE [{schema}].[product_accounting_profile] (
    [product_code] NVARCHAR(100) NOT NULL PRIMARY KEY,
    [product_family_code] NVARCHAR(100) NULL,
    [accounting_category_code] NVARCHAR(100) NULL,
    [revenue_account] NVARCHAR(100) NULL,
    [expense_account] NVARCHAR(100) NULL,
    [tax_code] NVARCHAR(100) NULL,
    [active] BIT NOT NULL DEFAULT 1
);

IF OBJECT_ID(N'{schema}.partner_accounting_profile', N'U') IS NULL
CREATE TABLE [{schema}].[partner_accounting_profile] (
    [partner_code] NVARCHAR(100) NOT NULL PRIMARY KEY,
    [partner_type] NVARCHAR(30) NOT NULL,
    [accounting_category_code] NVARCHAR(100) NULL,
    [collective_account] NVARCHAR(100) NOT NULL,
    [active] BIT NOT NULL DEFAULT 1
);

IF OBJECT_ID(N'{schema}.tax_code', N'U') IS NULL
CREATE TABLE [{schema}].[tax_code] (
    [code] NVARCHAR(100) NOT NULL PRIMARY KEY,
    [name] NVARCHAR(255) NOT NULL,
    [tax_type] NVARCHAR(50) NOT NULL,
    [rate] DECIMAL(9,4) NOT NULL,
    [collected_account] NVARCHAR(100) NULL,
    [deductible_account] NVARCHAR(100) NULL,
    [active] BIT NOT NULL DEFAULT 1
);
IF NOT EXISTS (SELECT 1 FROM [{schema}].[accounting_category] WHERE [code]=N'VENTES')
INSERT INTO [{schema}].[accounting_category] ([id],[code],[name],[revenue_account],[tax_account])
VALUES (N'category-sales',N'VENTES',N'Ventes de marchandises',N'701000',N'443000');
IF NOT EXISTS (SELECT 1 FROM [{schema}].[product_family] WHERE [code]=N'STANDARD')
INSERT INTO [{schema}].[product_family] ([id],[code],[name],[accounting_category_id])
VALUES (N'family-standard',N'STANDARD',N'Articles standard',N'category-sales');
IF NOT EXISTS (SELECT 1 FROM [{schema}].[tax_code] WHERE [code]=N'TVA18')
INSERT INTO [{schema}].[tax_code] ([code],[name],[tax_type],[rate],[collected_account])
VALUES (N'TVA18',N'TVA facturée 18%',N'collected',18,N'443000');

IF OBJECT_ID(N'{schema}.accounting_schema', N'U') IS NULL
CREATE TABLE [{schema}].[accounting_schema] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [code] NVARCHAR(100) NOT NULL UNIQUE,
    [name] NVARCHAR(255) NOT NULL,
    [operation_type] NVARCHAR(50) NOT NULL,
    [direction] NVARCHAR(30) NOT NULL,
    [journal_code] NVARCHAR(50) NOT NULL,
    [edition_scope] NVARCHAR(30) NOT NULL DEFAULT N'all',
    [active] BIT NOT NULL DEFAULT 1
);

IF OBJECT_ID(N'{schema}.accounting_schema_line', N'U') IS NULL
CREATE TABLE [{schema}].[accounting_schema_line] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [schema_id] NVARCHAR(50) NOT NULL,
    [line_no] INT NOT NULL,
    [posting_role] NVARCHAR(50) NOT NULL,
    [debit_credit] NVARCHAR(10) NOT NULL,
    [amount_basis] NVARCHAR(50) NOT NULL,
    [account_source] NVARCHAR(100) NOT NULL,
    [fixed_account] NVARCHAR(100) NULL,
    [auxiliary_source] NVARCHAR(100) NULL,
    [grouping_rule] NVARCHAR(50) NOT NULL DEFAULT N'account',
    [active] BIT NOT NULL DEFAULT 1,
    CONSTRAINT [UQ_sdb_schema_line] UNIQUE ([schema_id], [line_no])
);

IF OBJECT_ID(N'{schema}.accounting_context_mapping', N'U') IS NULL
CREATE TABLE [{schema}].[accounting_context_mapping] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [operation_type] NVARCHAR(50) NOT NULL,
    [document_type] NVARCHAR(30) NULL,
    [partner_category_code] NVARCHAR(100) NULL,
    [product_category_code] NVARCHAR(100) NULL,
    [payment_method_code] NVARCHAR(50) NULL,
    [schema_id] NVARCHAR(50) NOT NULL,
    [priority] INT NOT NULL DEFAULT 100,
    [active] BIT NOT NULL DEFAULT 1
);

IF OBJECT_ID(N'{schema}.business_partner_account', N'U') IS NULL
CREATE TABLE [{schema}].[business_partner_account] (
    [partner_code] NVARCHAR(100) NOT NULL,
    [partner_type] NVARCHAR(30) NOT NULL,
    [account_code] NVARCHAR(100) NOT NULL,
    CONSTRAINT [PK_sdb_partner_account] PRIMARY KEY ([partner_code], [partner_type])
);

IF OBJECT_ID(N'{schema}.tax_account_mapping', N'U') IS NULL
CREATE TABLE [{schema}].[tax_account_mapping] (
    [tax_type] NVARCHAR(50) NOT NULL,
    [rate] DECIMAL(9,4) NOT NULL,
    [collected_account] NVARCHAR(100) NULL,
    [deductible_account] NVARCHAR(100) NULL,
    CONSTRAINT [PK_sdb_tax_account_mapping] PRIMARY KEY ([tax_type], [rate])
);

IF OBJECT_ID(N'{schema}.payment_method', N'U') IS NULL
CREATE TABLE [{schema}].[payment_method] (
    [code] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [name] NVARCHAR(255) NOT NULL,
    [journal_code] NVARCHAR(50) NOT NULL,
    [debit_account] NVARCHAR(100) NOT NULL,
    [active] BIT NOT NULL DEFAULT 1
);

IF OBJECT_ID(N'{schema}.payment', N'U') IS NULL
CREATE TABLE [{schema}].[payment] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [payment_date] DATE NOT NULL,
    [amount] DECIMAL(19,4) NOT NULL,
    [currency] NVARCHAR(10) NOT NULL,
    [payment_method_code] NVARCHAR(50) NOT NULL,
    [reference] NVARCHAR(255) NULL,
    [journal_code] NVARCHAR(50) NOT NULL,
    [debit_account] NVARCHAR(100) NOT NULL,
    [partner_account] NVARCHAR(100) NULL,
    [resolved_schema_id] NVARCHAR(50) NULL,
    [status] NVARCHAR(30) NOT NULL DEFAULT N'Registered',
    [created_at] DATETIME2 NOT NULL DEFAULT SYSUTCDATETIME()
);
IF COL_LENGTH(N'{schema}.payment', N'partner_account') IS NULL ALTER TABLE [{schema}].[payment] ADD [partner_account] NVARCHAR(100) NULL;
IF COL_LENGTH(N'{schema}.payment', N'resolved_schema_id') IS NULL ALTER TABLE [{schema}].[payment] ADD [resolved_schema_id] NVARCHAR(50) NULL;
IF NOT EXISTS (SELECT 1 FROM sys.indexes WHERE [name]=N'UX_sdb_payment_reference' AND [object_id]=OBJECT_ID(N'{schema}.payment'))
CREATE UNIQUE INDEX [UX_sdb_payment_reference] ON [{schema}].[payment]([reference]) WHERE [reference] IS NOT NULL;

IF OBJECT_ID(N'{schema}.payment_allocation', N'U') IS NULL
CREATE TABLE [{schema}].[payment_allocation] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [payment_id] NVARCHAR(50) NOT NULL,
    [document_id] NVARCHAR(50) NOT NULL,
    [amount] DECIMAL(19,4) NOT NULL,
    CONSTRAINT [UQ_sdb_payment_allocation] UNIQUE ([payment_id], [document_id])
);

IF OBJECT_ID(N'{schema}.posting_batch', N'U') IS NULL
CREATE TABLE [{schema}].[posting_batch] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [source_kind] NVARCHAR(30) NOT NULL,
    [source_id] NVARCHAR(50) NOT NULL,
    [journal_code] NVARCHAR(50) NOT NULL,
    [piece_number] NVARCHAR(100) NOT NULL,
    [posting_date] DATE NOT NULL,
    [status] NVARCHAR(30) NOT NULL DEFAULT N'Generated',
    [total_debit] DECIMAL(19,4) NOT NULL,
    [total_credit] DECIMAL(19,4) NOT NULL,
    [validation_hash] NVARCHAR(100) NULL,
    [created_at] DATETIME2 NOT NULL DEFAULT SYSUTCDATETIME(),
    CONSTRAINT [UQ_sdb_posting_source] UNIQUE ([source_kind], [source_id])
);

IF OBJECT_ID(N'{schema}.posting_line', N'U') IS NULL
CREATE TABLE [{schema}].[posting_line] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [batch_id] NVARCHAR(50) NOT NULL,
    [line_no] INT NOT NULL,
    [role] NVARCHAR(50) NOT NULL,
    [general_account] NVARCHAR(100) NOT NULL,
    [auxiliary_account] NVARCHAR(100) NULL,
    [label] NVARCHAR(255) NOT NULL,
    [debit] DECIMAL(19,4) NOT NULL DEFAULT 0,
    [credit] DECIMAL(19,4) NOT NULL DEFAULT 0,
    CONSTRAINT [FK_sdb_posting_line_batch] FOREIGN KEY ([batch_id])
        REFERENCES [{schema}].[posting_batch]([id]),
    CONSTRAINT [UQ_sdb_posting_line] UNIQUE ([batch_id], [line_no])
);

IF OBJECT_ID(N'{schema}.integration_profile', N'U') IS NULL
CREATE TABLE [{schema}].[integration_profile] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [name] NVARCHAR(255) NOT NULL,
    [adapter_type] NVARCHAR(50) NOT NULL,
    [sage_edition] NVARCHAR(30) NOT NULL,
    [delivery_enabled] BIT NOT NULL DEFAULT 0,
    [configuration_json] NVARCHAR(MAX) NULL,
    [active] BIT NOT NULL DEFAULT 1
);

IF OBJECT_ID(N'{schema}.export_job', N'U') IS NULL
CREATE TABLE [{schema}].[export_job] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [batch_id] NVARCHAR(50) NOT NULL,
    [adapter_type] NVARCHAR(50) NOT NULL,
    [status] NVARCHAR(30) NOT NULL DEFAULT N'Prepared',
    [payload_json] NVARCHAR(MAX) NOT NULL,
    [external_id] NVARCHAR(255) NULL,
    [last_error] NVARCHAR(MAX) NULL,
    [created_at] DATETIME2 NOT NULL DEFAULT SYSUTCDATETIME(),
    [updated_at] DATETIME2 NOT NULL DEFAULT SYSUTCDATETIME(),
    CONSTRAINT [UQ_sdb_export_job] UNIQUE ([batch_id], [adapter_type])
);

IF OBJECT_ID(N'{schema}.export_attempt', N'U') IS NULL
CREATE TABLE [{schema}].[export_attempt] (
    [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
    [export_job_id] NVARCHAR(50) NOT NULL,
    [status] NVARCHAR(30) NOT NULL,
    [error_code] NVARCHAR(100) NULL,
    [error_message] NVARCHAR(MAX) NULL,
    [attempted_at] DATETIME2 NOT NULL DEFAULT SYSUTCDATETIME()
);

IF NOT EXISTS (SELECT 1 FROM [{schema}].[accounting_schema] WHERE [code]=N'SALE_STANDARD')
BEGIN
    INSERT INTO [{schema}].[accounting_schema] ([id],[code],[name],[operation_type],[direction],[journal_code])
    VALUES (N'schema-sale-standard',N'SALE_STANDARD',N'Vente client standard',N'invoice_sale',N'credit',N'VT');
    INSERT INTO [{schema}].[accounting_schema_line] ([id],[schema_id],[line_no],[posting_role],[debit_credit],[amount_basis],[account_source],[auxiliary_source],[grouping_rule])
    VALUES
      (N'schema-sale-customer',N'schema-sale-standard',1,N'customer',N'debit',N'document_total_ttc',N'CUSTOMER_COLLECTIVE_ACCOUNT',N'PARTNER_CODE',N'document'),
      (N'schema-sale-revenue',N'schema-sale-standard',2,N'sales',N'credit',N'line_amount_ht',N'RESOLVED_REVENUE_ACCOUNT',NULL,N'account'),
      (N'schema-sale-tax',N'schema-sale-standard',3,N'tax',N'credit',N'line_tax_amount',N'RESOLVED_TAX_ACCOUNT',NULL,N'account');
    INSERT INTO [{schema}].[accounting_context_mapping] ([id],[operation_type],[document_type],[schema_id],[priority])
    VALUES (N'mapping-sale-standard',N'invoice_sale',N'Facture',N'schema-sale-standard',1000);
END;

IF NOT EXISTS (SELECT 1 FROM [{schema}].[accounting_schema] WHERE [code]=N'PAYMENT_STANDARD')
BEGIN
    INSERT INTO [{schema}].[accounting_schema] ([id],[code],[name],[operation_type],[direction],[journal_code])
    VALUES (N'schema-payment-standard',N'PAYMENT_STANDARD',N'Règlement client standard',N'payment_receipt',N'debit',N'BQ');
    INSERT INTO [{schema}].[accounting_schema_line] ([id],[schema_id],[line_no],[posting_role],[debit_credit],[amount_basis],[account_source],[auxiliary_source],[grouping_rule])
    VALUES
      (N'schema-payment-bank',N'schema-payment-standard',1,N'bank',N'debit',N'payment_amount',N'PAYMENT_METHOD_ACCOUNT',NULL,N'document'),
      (N'schema-payment-customer',N'schema-payment-standard',2,N'customer',N'credit',N'payment_amount',N'CUSTOMER_COLLECTIVE_ACCOUNT',N'PARTNER_CODE',N'document');
    INSERT INTO [{schema}].[accounting_context_mapping] ([id],[operation_type],[payment_method_code],[schema_id],[priority])
    VALUES (N'mapping-payment-standard',N'payment_receipt',NULL,N'schema-payment-standard',1000);
END;

IF NOT EXISTS (SELECT 1 FROM [{schema}].[payment_method] WHERE [code]=N'bank')
INSERT INTO [{schema}].[payment_method] ([code],[name],[journal_code],[debit_account]) VALUES (N'bank',N'Virement bancaire',N'BQ',N'521000');

UPDATE [{schema}].[accounting_category]
SET [name]=N'Ventes de marchandises',[tax_account]=N'443000'
WHERE [id]=N'category-sales' AND [code]=N'VENTES' AND [name]=N'Ventes standard' AND [tax_account]=N'445700';

UPDATE [{schema}].[tax_code]
SET [name]=N'TVA facturée 18%',[collected_account]=N'443000'
WHERE [code]=N'TVA18' AND [name]=N'TVA collectée 18%' AND [collected_account]=N'445700';

UPDATE [{schema}].[accounting_schema]
SET [name]=N'Vente client standard'
WHERE [id]=N'schema-sale-standard' AND [code]=N'SALE_STANDARD' AND [name]=N'Standard customer sale';

UPDATE [{schema}].[accounting_schema]
SET [name]=N'Règlement client standard'
WHERE [id]=N'schema-payment-standard' AND [code]=N'PAYMENT_STANDARD' AND [name]=N'Standard customer payment';

UPDATE [{schema}].[payment_method]
SET [name]=N'Virement bancaire',[debit_account]=N'521000'
WHERE [code]=N'bank' AND [name]=N'Bank transfer' AND [debit_account]=N'512000';

IF NOT EXISTS (SELECT 1 FROM [{schema}].[schema_version] WHERE [version] = 1)
INSERT INTO [{schema}].[schema_version] ([version]) VALUES (1);
IF NOT EXISTS (SELECT 1 FROM [{schema}].[schema_version] WHERE [version] = 2)
INSERT INTO [{schema}].[schema_version] ([version]) VALUES (2);
IF NOT EXISTS (SELECT 1 FROM [{schema}].[schema_version] WHERE [version] = 3)
INSERT INTO [{schema}].[schema_version] ([version]) VALUES (3);
IF NOT EXISTS (SELECT 1 FROM [{schema}].[schema_version] WHERE [version] = 4)
INSERT INTO [{schema}].[schema_version] ([version]) VALUES (4);
"#,
        schema = SCHEMA,
    );
    db::execute_raw_query(client, &sql).await?;
    Ok(())
}

fn prefix(document_type: &str) -> &'static str {
    match document_type {
        "Devis" => "DEV",
        "Avoir" => "AVO",
        "Commande" => "CMD",
        _ => "FAC",
    }
}

fn document_from_row(row: &[Value]) -> BridgeDocumentSummary {
    BridgeDocumentSummary {
        id: parse_string(row.first()),
        document_type: parse_string(row.get(1)),
        number: parse_string(row.get(2)),
        status: parse_string(row.get(3)),
        document_date: parse_string(row.get(4)),
        due_date: parse_string(row.get(5)),
        partner_code: parse_string(row.get(6)),
        partner_name: parse_string(row.get(7)),
        currency: parse_string(row.get(8)),
        total_ht: round2(parse_f64(row.get(9))),
        total_tax: round2(parse_f64(row.get(10))),
        total_ttc: round2(parse_f64(row.get(11))),
        source_document_id: parse_string(row.get(12)),
    }
}

async fn resolve_invoice_context(
    state: &State<'_, AppState>,
    client: &mut db::DbClient,
    document: &BridgeDocumentSummary,
) -> Result<ResolvedDocumentContext, String> {
    let (default_ar, default_sales, default_vat) = {
        let config = state.config.lock().map_err(|error| error.to_string())?;
        (
            config.account_ar.clone(),
            config.account_sales.clone(),
            config.account_vat.clone(),
        )
    };
    let partner_sql = format!(
        "SELECT TOP 1 COALESCE(NULLIF([collective_account],N''),{fallback}),COALESCE([accounting_category_code],N'') FROM [{schema}].[partner_accounting_profile] WHERE [partner_code]={partner_code} AND [active]=1",
        schema = SCHEMA,
        partner_code = sql_string(&document.partner_code),
        fallback = sql_string(&default_ar),
    );
    let partner_data = db::execute_raw_query(client, &partner_sql).await?;
    let partner_account = partner_data
        .rows
        .first()
        .map(|row| parse_string(row.first()))
        .filter(|value| !value.is_empty())
        .unwrap_or(default_ar);
    let partner_category = partner_data
        .rows
        .first()
        .map(|row| parse_string(row.get(1)))
        .unwrap_or_default();
    if partner_account.trim().is_empty() {
        return Err(format!(
            "Missing collective account for customer '{}'",
            document.partner_code
        ));
    }

    let schema_sql = format!(
        "SELECT TOP 1 s.[id],s.[code],s.[journal_code] FROM [{schema}].[accounting_context_mapping] m JOIN [{schema}].[accounting_schema] s ON s.[id]=m.[schema_id] AND s.[active]=1 WHERE m.[active]=1 AND m.[operation_type]=N'invoice_sale' AND (m.[document_type] IS NULL OR m.[document_type]={document_type}) AND (m.[partner_category_code] IS NULL OR m.[partner_category_code]={partner_category}) ORDER BY CASE WHEN m.[partner_category_code] IS NULL THEN 1 ELSE 0 END,m.[priority] ASC",
        schema = SCHEMA,
        document_type = sql_string(&document.document_type),
        partner_category = sql_string(&partner_category),
    );
    let schema_data = db::execute_raw_query(client, &schema_sql).await?;
    let schema_row = schema_data
        .rows
        .first()
        .ok_or_else(|| "No active accounting schema matches this invoice".to_string())?;
    let schema_id = parse_string(schema_row.first());
    let schema_code = parse_string(schema_row.get(1));
    let journal_code = parse_string(schema_row.get(2));
    if journal_code.trim().is_empty() {
        return Err(format!(
            "Accounting schema '{}' has no journal",
            schema_code
        ));
    }

    let lines_sql = format!(
        r#"
SELECT
    l.[id],
    l.[line_no],
    COALESCE(
      NULLIF(l.[revenue_account],N''),
      NULLIF(p.[revenue_account],N''),
      NULLIF(f.[revenue_account],N''),
      NULLIF(c.[revenue_account],N''),
      (SELECT TOP 1 r.[account_code] FROM [{schema}].[accounting_rule] r WHERE r.[active]=1 AND r.[account_role]=N'revenue' AND ((r.[rule_scope]=N'product' AND r.[scope_key]=l.[product_code]) OR (r.[rule_scope]=N'family' AND r.[scope_key]=f.[code]) OR (r.[rule_scope]=N'category' AND r.[scope_key]=c.[code])) ORDER BY r.[priority]),
      {default_sales}
    ) AS resolved_revenue_account,
    COALESCE(
      NULLIF(l.[tax_account],N''),
      NULLIF(t.[collected_account],N''),
      NULLIF(f.[tax_account],N''),
      NULLIF(c.[tax_account],N''),
      (SELECT TOP 1 r.[account_code] FROM [{schema}].[accounting_rule] r WHERE r.[active]=1 AND r.[account_role]=N'tax' AND ((r.[rule_scope]=N'product' AND r.[scope_key]=l.[product_code]) OR (r.[rule_scope]=N'family' AND r.[scope_key]=f.[code]) OR (r.[rule_scope]=N'category' AND r.[scope_key]=c.[code])) ORDER BY r.[priority]),
      {default_vat}
    ) AS resolved_tax_account,
    l.[amount_ht],
    l.[tax_amount],
    COALESCE(f.[code],p.[product_family_code],l.[product_family_code],N''),
    COALESCE(c.[code],p.[accounting_category_code],l.[accounting_category_code],N''),
    COALESCE(p.[tax_code],l.[tax_code],N'')
FROM [{schema}].[commercial_document_line] l
LEFT JOIN [{schema}].[product_accounting_profile] p ON p.[product_code]=l.[product_code] AND p.[active]=1
LEFT JOIN [{schema}].[product_family] f ON f.[code]=COALESCE(p.[product_family_code],l.[product_family_code]) AND f.[active]=1
LEFT JOIN [{schema}].[accounting_category] c ON c.[id]=COALESCE((SELECT TOP 1 pc.[id] FROM [{schema}].[accounting_category] pc WHERE pc.[code]=COALESCE(p.[accounting_category_code],l.[accounting_category_code]) AND pc.[active]=1),f.[accounting_category_id]) AND c.[active]=1
LEFT JOIN [{schema}].[tax_code] t ON t.[code]=COALESCE(p.[tax_code],l.[tax_code]) AND t.[active]=1
WHERE l.[document_id]={document_id}
ORDER BY l.[line_no]
"#,
        schema = SCHEMA,
        document_id = sql_string(&document.id),
        default_sales = sql_string(&default_sales),
        default_vat = sql_string(&default_vat),
    );
    let line_data = db::execute_raw_query(client, &lines_sql).await?;
    if line_data.rows.is_empty() {
        return Err("The invoice has no lines to resolve".to_string());
    }
    let mut lines = Vec::new();
    let mut updates = Vec::new();
    for row in &line_data.rows {
        let line_id = parse_string(row.first());
        let line_no = parse_f64(row.get(1)) as i32;
        let revenue_account = parse_string(row.get(2));
        let tax_account = parse_string(row.get(3));
        if revenue_account.is_empty() {
            return Err(format!(
                "Missing revenue account for invoice line {}",
                line_no
            ));
        }
        let tax_amount = round2(parse_f64(row.get(5)));
        if tax_amount != 0.0 && tax_account.is_empty() {
            return Err(format!("Missing tax account for invoice line {}", line_no));
        }
        let resolution = json!({
            "schema_code": schema_code,
            "product_family_code": parse_string(row.get(6)),
            "accounting_category_code": parse_string(row.get(7)),
            "tax_code": parse_string(row.get(8)),
            "resolved_revenue_account": revenue_account.clone(),
            "resolved_tax_account": tax_account.clone(),
            "resolved_at": chrono::Utc::now().to_rfc3339(),
        });
        updates.push(format!(
            "UPDATE [{schema}].[commercial_document_line] SET [resolved_revenue_account]={revenue},[resolved_tax_account]={tax},[resolution_json]={resolution} WHERE [id]={line_id};",
            schema = SCHEMA,
            revenue = sql_string(&revenue_account),
            tax = sql_opt_string(&tax_account),
            resolution = sql_string(&resolution.to_string()),
            line_id = sql_string(&line_id),
        ));
        lines.push(ResolvedDocumentLine {
            line_id,
            line_no,
            revenue_account,
            tax_account,
            amount_ht: round2(parse_f64(row.get(4))),
            tax_amount,
        });
    }
    updates.push(format!(
        "UPDATE [{schema}].[commercial_document] SET [partner_account]={partner_account},[partner_category_code]={partner_category},[resolved_schema_id]={schema_id},[resolved_journal_code]={journal} WHERE [id]={document_id};",
        schema = SCHEMA,
        partner_account = sql_string(&partner_account),
        partner_category = sql_opt_string(&partner_category),
        schema_id = sql_string(&schema_id),
        journal = sql_string(&journal_code),
        document_id = sql_string(&document.id),
    ));
    db::execute_raw_query(client, &updates.join("\n")).await?;
    Ok(ResolvedDocumentContext {
        schema_id,
        schema_code,
        journal_code,
        partner_account,
        lines,
    })
}

async fn fetch_document(
    client: &mut db::DbClient,
    document_id: &str,
) -> Result<BridgeDocumentSummary, String> {
    let sql = format!(
        "SELECT [id],[document_type],[official_number],[status],CONVERT(VARCHAR(10),[document_date],23),COALESCE(CONVERT(VARCHAR(10),[due_date],23),N''),COALESCE([partner_code],N''),COALESCE([partner_name],N''),[currency],[total_ht],[total_tax],[total_ttc],COALESCE([source_document_id],N'') FROM [{schema}].[commercial_document] WHERE [id]={id}",
        schema = SCHEMA,
        id = sql_string(document_id),
    );
    let data = db::execute_raw_query(client, &sql).await?;
    data.rows
        .first()
        .map(|row| document_from_row(row))
        .ok_or_else(|| format!("Bridge document '{}' not found", document_id))
}

#[tauri::command]
pub async fn initialize_bridge(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await
}

#[tauri::command]
pub async fn list_bridge_documents(
    state: State<'_, AppState>,
    id: String,
    document_type: Option<String>,
) -> Result<Vec<BridgeDocumentSummary>, String> {
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let filter = document_type
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!(" WHERE [document_type]={}", sql_string(&value)))
        .unwrap_or_default();
    let sql = format!(
        "SELECT [id],[document_type],[official_number],[status],CONVERT(VARCHAR(10),[document_date],23),COALESCE(CONVERT(VARCHAR(10),[due_date],23),N''),COALESCE([partner_code],N''),COALESCE([partner_name],N''),[currency],[total_ht],[total_tax],[total_ttc],COALESCE([source_document_id],N'') FROM [{schema}].[commercial_document]{filter} ORDER BY [document_date] DESC,[official_number] DESC",
        schema = SCHEMA,
        filter = filter,
    );
    let data = db::execute_raw_query(&mut client, &sql).await?;
    Ok(data.rows.iter().map(|row| document_from_row(row)).collect())
}

#[tauri::command]
pub async fn create_bridge_document(
    state: State<'_, AppState>,
    id: String,
    invoice: InvoiceHeader,
) -> Result<BridgeDocumentSummary, String> {
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let document_id = Uuid::new_v4().to_string();
    let document_type = if invoice.nature.trim().is_empty() {
        "Devis"
    } else {
        invoice.nature.trim()
    };
    let sequence_key = format!(
        "{}:{}",
        document_type,
        invoice.date.get(0..4).unwrap_or("0000")
    );
    let supplied_number = invoice.numero.trim();
    let official_number_expr = if supplied_number.is_empty() {
        format!("CONCAT(N'{}-', LEFT(CONVERT(VARCHAR(10), @doc_date, 112), 4), N'-', RIGHT(N'000000' + CAST(@sequence_value AS NVARCHAR(20)), 6))", prefix(document_type))
    } else {
        sql_string(supplied_number)
    };
    let lines = invoice.lignes.iter().map(|line| format!(
        "INSERT INTO [{schema}].[commercial_document_line] ([id],[document_id],[line_no],[product_id],[product_code],[product_family_code],[accounting_category_code],[tax_code],[description],[quantity],[unit],[unit_price_ht],[amount_ht],[tax_rate],[tax_amount],[amount_ttc],[revenue_account],[expense_account],[tax_account]) VALUES ({line_id},{doc_id},{line_no},{product_id},{product_code},{family},{category},{tax_code},{description},{quantity},{unit},{unit_price},{amount_ht},{tax_rate},{tax_amount},{amount_ttc},{revenue},{expense},{tax_account});",
        schema = SCHEMA,
        line_id = sql_string(&Uuid::new_v4().to_string()),
        doc_id = sql_string(&document_id),
        line_no = line.ordre,
        product_id = sql_opt_string(&line.article_id),
        product_code = sql_opt_string(&line.article_code),
        family = sql_opt_string(&line.product_family_code),
        category = sql_opt_string(&line.accounting_category_code),
        tax_code = sql_opt_string(&line.tax_code),
        description = sql_opt_string(&line.libelle),
        quantity = sql_number(line.quantite),
        unit = sql_opt_string(&line.unite),
        unit_price = sql_number(line.prix_ht),
        amount_ht = sql_number(line.montant_ht),
        tax_rate = sql_number(line.taux_tva),
        tax_amount = sql_number(line.montant_tva + line.montant_bic),
        amount_ttc = sql_number(line.montant_ttc),
        revenue = sql_opt_string(&line.revenue_account),
        expense = sql_opt_string(&line.expense_account),
        tax_account = sql_opt_string(&line.vat_account),
    )).collect::<Vec<_>>().join("\n");
    let sql = format!(
        r#"
BEGIN TRY
BEGIN TRANSACTION;
DECLARE @doc_date DATE = {doc_date};
DECLARE @sequence_value BIGINT;
IF EXISTS (SELECT 1 FROM [{schema}].[document_number_sequence] WITH (UPDLOCK,HOLDLOCK) WHERE [sequence_key]={sequence_key})
BEGIN
    UPDATE [{schema}].[document_number_sequence] SET [next_value]=[next_value]+1,[updated_at]=SYSUTCDATETIME() WHERE [sequence_key]={sequence_key};
    SELECT @sequence_value=[next_value]-1 FROM [{schema}].[document_number_sequence] WHERE [sequence_key]={sequence_key};
END
ELSE
BEGIN
    INSERT INTO [{schema}].[document_number_sequence] ([sequence_key],[next_value]) VALUES ({sequence_key},2);
    SET @sequence_value=1;
END;
DECLARE @official_number NVARCHAR(100) = {official_number};
INSERT INTO [{schema}].[commercial_document] ([id],[document_type],[official_number],[status],[partner_id],[partner_code],[partner_name],[document_date],[due_date],[reference],[currency],[total_ht],[total_tax],[total_ttc],[notes],[conditions])
VALUES ({doc_id},{doc_type},@official_number,N'Draft',{partner_id},{partner_code},{partner_name},@doc_date,{due_date},{reference},{currency},{total_ht},{total_tax},{total_ttc},{notes},{conditions});
{lines}
INSERT INTO [{schema}].[document_status_history] ([id],[document_id],[from_status],[to_status],[changed_by]) VALUES ({history_id},{doc_id},NULL,N'Draft',N'SDB');
COMMIT TRANSACTION;
END TRY
BEGIN CATCH
IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION;
THROW;
END CATCH
"#,
        schema = SCHEMA,
        doc_date = sql_string(&invoice.date),
        sequence_key = sql_string(&sequence_key),
        official_number = official_number_expr,
        doc_id = sql_string(&document_id),
        doc_type = sql_string(document_type),
        partner_id = sql_opt_string(&invoice.tiers_id),
        partner_code = sql_opt_string(&invoice.tiers_code),
        partner_name = sql_opt_string(&invoice.tiers_nom),
        due_date = sql_opt_string(&invoice.date_echeance),
        reference = sql_opt_string(&invoice.reference),
        currency = sql_string(if invoice.devise.trim().is_empty() {
            "XOF"
        } else {
            &invoice.devise
        }),
        total_ht = sql_number(invoice.total_ht),
        total_tax = sql_number(invoice.total_tva + invoice.total_bic),
        total_ttc = sql_number(invoice.total_ttc),
        notes = sql_opt_string(&invoice.notes),
        conditions = sql_opt_string(&invoice.conditions),
        lines = lines,
        history_id = sql_string(&Uuid::new_v4().to_string()),
    );
    db::execute_raw_query(&mut client, &sql).await?;
    fetch_document(&mut client, &document_id).await
}

#[tauri::command]
pub async fn validate_bridge_document(
    state: State<'_, AppState>,
    id: String,
    document_id: String,
    validated_by: Option<String>,
) -> Result<BridgeDocumentSummary, String> {
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let actor = validated_by.unwrap_or_else(|| "SDB".to_string());
    let document = fetch_document(&mut client, &document_id).await?;
    if document.document_type == "Facture" {
        resolve_invoice_context(&state, &mut client, &document).await?;
    }
    let sql = format!(
        "BEGIN TRY BEGIN TRANSACTION; IF NOT EXISTS (SELECT 1 FROM [{schema}].[commercial_document] WHERE [id]={id} AND [status]=N'Draft') THROW 50001, 'Only Draft documents can be validated', 1; UPDATE [{schema}].[commercial_document] SET [status]=N'Validated',[validated_at]=SYSUTCDATETIME(),[validated_by]={actor},[updated_at]=SYSUTCDATETIME(),[lock_version]=[lock_version]+1 WHERE [id]={id}; INSERT INTO [{schema}].[document_status_history] ([id],[document_id],[from_status],[to_status],[changed_by]) VALUES ({history_id},{id},N'Draft',N'Validated',{actor}); COMMIT TRANSACTION; END TRY BEGIN CATCH IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION; THROW; END CATCH",
        schema = SCHEMA,
        id = sql_string(&document_id),
        actor = sql_string(&actor),
        history_id = sql_string(&Uuid::new_v4().to_string()),
    );
    db::execute_raw_query(&mut client, &sql).await?;
    fetch_document(&mut client, &document_id).await
}

#[tauri::command]
pub async fn transform_bridge_quote(
    state: State<'_, AppState>,
    id: String,
    quote_id: String,
) -> Result<BridgeDocumentSummary, String> {
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let invoice_id = Uuid::new_v4().to_string();
    let relation_id = Uuid::new_v4().to_string();
    let source_history_id = Uuid::new_v4().to_string();
    let target_history_id = Uuid::new_v4().to_string();
    let sql = format!(
        r#"
BEGIN TRY
BEGIN TRANSACTION;
IF NOT EXISTS (SELECT 1 FROM [{schema}].[commercial_document] WITH (UPDLOCK,HOLDLOCK) WHERE [id]={quote_id} AND [document_type]=N'Devis' AND [status]=N'Validated')
    THROW 50002, 'Only a validated Devis can be transformed', 1;
IF EXISTS (SELECT 1 FROM [{schema}].[commercial_document] WHERE [source_document_id]={quote_id} AND [document_type]=N'Facture')
    THROW 50003, 'This Devis has already been transformed', 1;
DECLARE @year NVARCHAR(4)=(SELECT LEFT(CONVERT(VARCHAR(10),[document_date],112),4) FROM [{schema}].[commercial_document] WHERE [id]={quote_id});
DECLARE @sequence_key NVARCHAR(150)=CONCAT(N'Facture:',@year);
DECLARE @sequence_value BIGINT;
IF EXISTS (SELECT 1 FROM [{schema}].[document_number_sequence] WITH (UPDLOCK,HOLDLOCK) WHERE [sequence_key]=@sequence_key)
BEGIN UPDATE [{schema}].[document_number_sequence] SET [next_value]=[next_value]+1,[updated_at]=SYSUTCDATETIME() WHERE [sequence_key]=@sequence_key; SELECT @sequence_value=[next_value]-1 FROM [{schema}].[document_number_sequence] WHERE [sequence_key]=@sequence_key; END
ELSE BEGIN INSERT INTO [{schema}].[document_number_sequence] ([sequence_key],[next_value]) VALUES (@sequence_key,2); SET @sequence_value=1; END;
DECLARE @number NVARCHAR(100)=CONCAT(N'FAC-',@year,N'-',RIGHT(N'000000'+CAST(@sequence_value AS NVARCHAR(20)),6));
INSERT INTO [{schema}].[commercial_document] ([id],[document_type],[official_number],[status],[source_document_id],[partner_id],[partner_code],[partner_name],[partner_account],[document_date],[due_date],[reference],[currency],[total_ht],[total_tax],[total_ttc],[notes],[conditions])
SELECT {invoice_id},N'Facture',@number,N'Draft',[id],[partner_id],[partner_code],[partner_name],[partner_account],CONVERT(DATE,GETDATE()),[due_date],[reference],[currency],[total_ht],[total_tax],[total_ttc],[notes],[conditions] FROM [{schema}].[commercial_document] WHERE [id]={quote_id};
INSERT INTO [{schema}].[commercial_document_line] ([id],[document_id],[line_no],[product_id],[product_code],[product_family_code],[accounting_category_code],[tax_code],[description],[quantity],[unit],[unit_price_ht],[amount_ht],[tax_rate],[tax_amount],[amount_ttc],[revenue_account],[expense_account],[tax_account])
SELECT CONVERT(NVARCHAR(50),NEWID()),{invoice_id},[line_no],[product_id],[product_code],[product_family_code],[accounting_category_code],[tax_code],[description],[quantity],[unit],[unit_price_ht],[amount_ht],[tax_rate],[tax_amount],[amount_ttc],[revenue_account],[expense_account],[tax_account] FROM [{schema}].[commercial_document_line] WHERE [document_id]={quote_id};
UPDATE [{schema}].[commercial_document] SET [status]=N'Converted',[updated_at]=SYSUTCDATETIME(),[lock_version]=[lock_version]+1 WHERE [id]={quote_id};
INSERT INTO [{schema}].[document_relation] ([id],[source_document_id],[target_document_id],[relation_type]) VALUES ({relation_id},{quote_id},{invoice_id},N'transformed_to');
INSERT INTO [{schema}].[document_status_history] ([id],[document_id],[from_status],[to_status],[changed_by]) VALUES ({source_history},{quote_id},N'Validated',N'Converted',N'SDB');
INSERT INTO [{schema}].[document_status_history] ([id],[document_id],[from_status],[to_status],[changed_by]) VALUES ({target_history},{invoice_id},NULL,N'Draft',N'SDB');
COMMIT TRANSACTION;
END TRY
BEGIN CATCH IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION; THROW; END CATCH
"#,
        schema = SCHEMA,
        quote_id = sql_string(&quote_id),
        invoice_id = sql_string(&invoice_id),
        relation_id = sql_string(&relation_id),
        source_history = sql_string(&source_history_id),
        target_history = sql_string(&target_history_id),
    );
    db::execute_raw_query(&mut client, &sql).await?;
    fetch_document(&mut client, &invoice_id).await
}

async fn fetch_batch(client: &mut db::DbClient, batch_id: &str) -> Result<PostingBatch, String> {
    let header_sql = format!("SELECT [id],[source_kind],[source_id],[journal_code],[piece_number],CONVERT(VARCHAR(10),[posting_date],23),[status],[total_debit],[total_credit] FROM [{schema}].[posting_batch] WHERE [id]={id}", schema=SCHEMA, id=sql_string(batch_id));
    let header = db::execute_raw_query(client, &header_sql).await?;
    let row = header
        .rows
        .first()
        .ok_or_else(|| format!("Posting batch '{}' not found", batch_id))?;
    let line_sql = format!("SELECT [line_no],[role],[general_account],COALESCE([auxiliary_account],N''),[label],[debit],[credit] FROM [{schema}].[posting_line] WHERE [batch_id]={id} ORDER BY [line_no]", schema=SCHEMA, id=sql_string(batch_id));
    let line_data = db::execute_raw_query(client, &line_sql).await?;
    Ok(PostingBatch {
        id: parse_string(row.first()),
        source_kind: parse_string(row.get(1)),
        source_id: parse_string(row.get(2)),
        journal_code: parse_string(row.get(3)),
        piece_number: parse_string(row.get(4)),
        posting_date: parse_string(row.get(5)),
        status: parse_string(row.get(6)),
        total_debit: round2(parse_f64(row.get(7))),
        total_credit: round2(parse_f64(row.get(8))),
        lines: line_data
            .rows
            .iter()
            .map(|line| PostingLine {
                line_no: parse_f64(line.first()) as i32,
                role: parse_string(line.get(1)),
                general_account: parse_string(line.get(2)),
                auxiliary_account: parse_string(line.get(3)),
                label: parse_string(line.get(4)),
                debit: round2(parse_f64(line.get(5))),
                credit: round2(parse_f64(line.get(6))),
            })
            .collect(),
    })
}

async fn insert_batch(
    client: &mut db::DbClient,
    mut batch: PostingBatch,
) -> Result<PostingBatch, String> {
    let total_debit = round2(batch.lines.iter().map(|line| line.debit).sum());
    let total_credit = round2(batch.lines.iter().map(|line| line.credit).sum());
    if total_debit <= 0.0 || (total_debit - total_credit).abs() > 0.005 {
        return Err(format!(
            "Posting batch is not balanced: debit={} credit={}",
            total_debit, total_credit
        ));
    }
    batch.total_debit = total_debit;
    batch.total_credit = total_credit;
    let lines = batch.lines.iter().map(|line| format!(
        "INSERT INTO [{schema}].[posting_line] ([id],[batch_id],[line_no],[role],[general_account],[auxiliary_account],[label],[debit],[credit]) VALUES ({id},{batch_id},{line_no},{role},{account},{aux},{label},{debit},{credit});",
        schema=SCHEMA, id=sql_string(&Uuid::new_v4().to_string()), batch_id=sql_string(&batch.id), line_no=line.line_no,
        role=sql_string(&line.role), account=sql_string(&line.general_account), aux=sql_opt_string(&line.auxiliary_account),
        label=sql_string(&line.label), debit=sql_number(line.debit), credit=sql_number(line.credit)
    )).collect::<Vec<_>>().join("\n");
    let sql = format!("BEGIN TRY BEGIN TRANSACTION; INSERT INTO [{schema}].[posting_batch] ([id],[source_kind],[source_id],[journal_code],[piece_number],[posting_date],[status],[total_debit],[total_credit]) VALUES ({id},{source_kind},{source_id},{journal},{piece},{date},N'Generated',{debit},{credit}); {lines} COMMIT TRANSACTION; END TRY BEGIN CATCH IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION; THROW; END CATCH",
        schema=SCHEMA, id=sql_string(&batch.id), source_kind=sql_string(&batch.source_kind), source_id=sql_string(&batch.source_id),
        journal=sql_string(&batch.journal_code), piece=sql_string(&batch.piece_number), date=sql_string(&batch.posting_date),
        debit=sql_number(total_debit), credit=sql_number(total_credit), lines=lines);
    db::execute_raw_query(client, &sql).await?;
    fetch_batch(client, &batch.id).await
}

async fn load_frozen_invoice_context(
    client: &mut db::DbClient,
    document: &BridgeDocumentSummary,
) -> Result<ResolvedDocumentContext, String> {
    let header_sql = format!(
        "SELECT COALESCE(d.[resolved_schema_id],N''),COALESCE(s.[code],N''),COALESCE(d.[resolved_journal_code],N''),COALESCE(d.[partner_account],N'') FROM [{schema}].[commercial_document] d LEFT JOIN [{schema}].[accounting_schema] s ON s.[id]=d.[resolved_schema_id] WHERE d.[id]={id}",
        schema = SCHEMA,
        id = sql_string(&document.id),
    );
    let header_data = db::execute_raw_query(client, &header_sql).await?;
    let header = header_data
        .rows
        .first()
        .ok_or_else(|| "Validated invoice accounting snapshot is missing".to_string())?;
    let context = ResolvedDocumentContext {
        schema_id: parse_string(header.first()),
        schema_code: parse_string(header.get(1)),
        journal_code: parse_string(header.get(2)),
        partner_account: parse_string(header.get(3)),
        lines: Vec::new(),
    };
    if context.schema_id.is_empty() || context.partner_account.is_empty() {
        return Err("Invoice was validated without a complete accounting snapshot".to_string());
    }
    let line_sql = format!(
        "SELECT [id],[line_no],COALESCE([resolved_revenue_account],N''),COALESCE([resolved_tax_account],N''),[amount_ht],[tax_amount] FROM [{schema}].[commercial_document_line] WHERE [document_id]={id} ORDER BY [line_no]",
        schema = SCHEMA,
        id = sql_string(&document.id),
    );
    let line_data = db::execute_raw_query(client, &line_sql).await?;
    Ok(ResolvedDocumentContext {
        lines: line_data
            .rows
            .iter()
            .map(|row| ResolvedDocumentLine {
                line_id: parse_string(row.first()),
                line_no: parse_f64(row.get(1)) as i32,
                revenue_account: parse_string(row.get(2)),
                tax_account: parse_string(row.get(3)),
                amount_ht: round2(parse_f64(row.get(4))),
                tax_amount: round2(parse_f64(row.get(5))),
            })
            .collect(),
        ..context
    })
}

fn push_schema_posting_line(
    output: &mut Vec<PostingLine>,
    role: &str,
    debit_credit: &str,
    account: String,
    auxiliary_account: String,
    label: String,
    amount: f64,
) -> Result<(), String> {
    if amount.abs() < 0.005 {
        return Ok(());
    }
    if account.trim().is_empty() {
        return Err(format!(
            "Accounting schema role '{}' resolved no account",
            role
        ));
    }
    let line_no = output.len() as i32 + 1;
    output.push(PostingLine {
        line_no,
        role: role.to_string(),
        general_account: account,
        auxiliary_account,
        label,
        debit: if debit_credit.eq_ignore_ascii_case("debit") {
            round2(amount)
        } else {
            0.0
        },
        credit: if debit_credit.eq_ignore_ascii_case("credit") {
            round2(amount)
        } else {
            0.0
        },
    });
    Ok(())
}

async fn build_invoice_lines_from_schema(
    client: &mut db::DbClient,
    document: &BridgeDocumentSummary,
    context: &ResolvedDocumentContext,
) -> Result<Vec<PostingLine>, String> {
    let sql = format!(
        "SELECT [posting_role],[debit_credit],[amount_basis],[account_source],COALESCE([fixed_account],N''),COALESCE([auxiliary_source],N''),[grouping_rule] FROM [{schema}].[accounting_schema_line] WHERE [schema_id]={schema_id} AND [active]=1 ORDER BY [line_no]",
        schema = SCHEMA,
        schema_id = sql_string(&context.schema_id),
    );
    let data = db::execute_raw_query(client, &sql).await?;
    if data.rows.is_empty() {
        return Err(format!(
            "Accounting schema '{}' has no active lines",
            context.schema_code
        ));
    }
    let mut output = Vec::new();
    for row in &data.rows {
        let role = parse_string(row.first());
        let debit_credit = parse_string(row.get(1));
        let amount_basis = parse_string(row.get(2));
        let account_source = parse_string(row.get(3));
        let fixed_account = parse_string(row.get(4));
        let auxiliary_source = parse_string(row.get(5));
        match amount_basis.as_str() {
            "document_total_ttc" => {
                let account = match account_source.as_str() {
                    "CUSTOMER_COLLECTIVE_ACCOUNT" => context.partner_account.clone(),
                    "FIXED_ACCOUNT" => fixed_account,
                    _ => {
                        return Err(format!(
                            "Unsupported document account source '{}'",
                            account_source
                        ))
                    }
                };
                let auxiliary = if auxiliary_source == "PARTNER_CODE" {
                    document.partner_code.clone()
                } else {
                    String::new()
                };
                push_schema_posting_line(
                    &mut output,
                    &role,
                    &debit_credit,
                    account,
                    auxiliary,
                    format!("{} {}", role, document.number),
                    document.total_ttc,
                )?;
            }
            "line_amount_ht" | "line_tax_amount" => {
                let mut grouped = BTreeMap::<String, f64>::new();
                for line in &context.lines {
                    let (account, amount) = if amount_basis == "line_amount_ht" {
                        (line.revenue_account.clone(), line.amount_ht)
                    } else {
                        (line.tax_account.clone(), line.tax_amount)
                    };
                    if amount.abs() >= 0.005 {
                        *grouped.entry(account).or_default() += amount;
                    }
                }
                for (account, amount) in grouped {
                    push_schema_posting_line(
                        &mut output,
                        &role,
                        &debit_credit,
                        account,
                        String::new(),
                        format!("{} {}", role, document.number),
                        amount,
                    )?;
                }
            }
            _ => {
                return Err(format!(
                    "Unsupported invoice amount basis '{}'",
                    amount_basis
                ))
            }
        }
    }
    Ok(output)
}

async fn build_payment_lines_from_schema(
    client: &mut db::DbClient,
    schema_id: &str,
    document: &BridgeDocumentSummary,
    partner_account: &str,
    payment_account: &str,
    amount: f64,
) -> Result<Vec<PostingLine>, String> {
    let sql = format!(
        "SELECT [posting_role],[debit_credit],[amount_basis],[account_source],COALESCE([fixed_account],N''),COALESCE([auxiliary_source],N'') FROM [{schema}].[accounting_schema_line] WHERE [schema_id]={schema_id} AND [active]=1 ORDER BY [line_no]",
        schema = SCHEMA,
        schema_id = sql_string(schema_id),
    );
    let data = db::execute_raw_query(client, &sql).await?;
    if data.rows.is_empty() {
        return Err("Payment accounting schema has no active lines".to_string());
    }
    let mut output = Vec::new();
    for row in &data.rows {
        let role = parse_string(row.first());
        let debit_credit = parse_string(row.get(1));
        let amount_basis = parse_string(row.get(2));
        if amount_basis != "payment_amount" {
            return Err(format!(
                "Unsupported payment amount basis '{}'",
                amount_basis
            ));
        }
        let account_source = parse_string(row.get(3));
        let account = match account_source.as_str() {
            "PAYMENT_METHOD_ACCOUNT" => payment_account.to_string(),
            "CUSTOMER_COLLECTIVE_ACCOUNT" => partner_account.to_string(),
            "FIXED_ACCOUNT" => parse_string(row.get(4)),
            _ => {
                return Err(format!(
                    "Unsupported payment account source '{}'",
                    account_source
                ))
            }
        };
        let auxiliary = if parse_string(row.get(5)) == "PARTNER_CODE" {
            document.partner_code.clone()
        } else {
            String::new()
        };
        push_schema_posting_line(
            &mut output,
            &role,
            &debit_credit,
            account,
            auxiliary,
            format!("Paiement {}", document.number),
            amount,
        )?;
    }
    Ok(output)
}

#[tauri::command]
pub async fn generate_bridge_invoice_posting(
    state: State<'_, AppState>,
    id: String,
    document_id: String,
) -> Result<PostingBatch, String> {
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let document = fetch_document(&mut client, &document_id).await?;
    if document.document_type != "Facture"
        || !matches!(document.status.as_str(), "Validated" | "Posted")
    {
        return Err("Only a validated Facture can generate an invoice posting".to_string());
    }
    let existing_sql = format!("SELECT TOP 1 [id] FROM [{schema}].[posting_batch] WHERE [source_kind]=N'invoice' AND [source_id]={id}", schema=SCHEMA, id=sql_string(&document_id));
    let existing = db::execute_raw_query(&mut client, &existing_sql).await?;
    if let Some(row) = existing.rows.first() {
        return fetch_batch(&mut client, &parse_string(row.first())).await;
    }
    let context = load_frozen_invoice_context(&mut client, &document).await?;
    let posting_lines = build_invoice_lines_from_schema(&mut client, &document, &context).await?;
    let batch = insert_batch(
        &mut client,
        PostingBatch {
            id: Uuid::new_v4().to_string(),
            source_kind: "invoice".into(),
            source_id: document_id,
            journal_code: context.journal_code,
            piece_number: document.number,
            posting_date: document.document_date,
            status: "Generated".into(),
            total_debit: 0.0,
            total_credit: 0.0,
            lines: posting_lines,
        },
    )
    .await?;
    if document.status == "Validated" {
        let sql = format!(
            "BEGIN TRY BEGIN TRANSACTION; UPDATE [{schema}].[commercial_document] SET [status]=N'Posted',[updated_at]=SYSUTCDATETIME(),[lock_version]=[lock_version]+1 WHERE [id]={id} AND [status]=N'Validated'; INSERT INTO [{schema}].[document_status_history] ([id],[document_id],[from_status],[to_status],[changed_by]) VALUES ({history_id},{id},N'Validated',N'Posted',N'SDB'); COMMIT TRANSACTION; END TRY BEGIN CATCH IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION; THROW; END CATCH",
            schema = SCHEMA,
            id = sql_string(&document.id),
            history_id = sql_string(&Uuid::new_v4().to_string()),
        );
        db::execute_raw_query(&mut client, &sql).await?;
    }
    Ok(batch)
}

#[tauri::command]
pub async fn register_bridge_payment(
    state: State<'_, AppState>,
    id: String,
    payment: PaymentInput,
) -> Result<Value, String> {
    if payment.amount <= 0.0 {
        return Err("Payment amount must be greater than zero".to_string());
    }
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let document = fetch_document(&mut client, &payment.invoice_id).await?;
    if document.document_type != "Facture"
        || !matches!(document.status.as_str(), "Validated" | "Posted")
    {
        return Err("Payments can only be registered against validated invoices".to_string());
    }
    let allocated_sql = format!(
        "SELECT COALESCE(SUM([amount]),0) FROM [{schema}].[payment_allocation] WHERE [document_id]={document_id}",
        schema = SCHEMA,
        document_id = sql_string(&payment.invoice_id),
    );
    let allocated_data = db::execute_raw_query(&mut client, &allocated_sql).await?;
    let already_allocated = round2(parse_f64(
        allocated_data.rows.first().and_then(|row| row.first()),
    ));
    if round2(already_allocated + payment.amount) > document.total_ttc {
        return Err(format!(
            "Payment exceeds outstanding balance of {:.2}",
            round2(document.total_ttc - already_allocated)
        ));
    }
    let payment_id = Uuid::new_v4().to_string();
    let method_code = if payment.payment_method_code.trim().is_empty() {
        "bank"
    } else {
        payment.payment_method_code.trim()
    };
    let method_sql = format!(
        "SELECT TOP 1 [journal_code],[debit_account] FROM [{schema}].[payment_method] WHERE [code]={code} AND [active]=1",
        schema = SCHEMA,
        code = sql_string(method_code),
    );
    let method_data = db::execute_raw_query(&mut client, &method_sql).await?;
    let method = method_data.rows.first().ok_or_else(|| {
        format!(
            "No active accounting configuration exists for payment method '{}'",
            method_code
        )
    })?;
    let journal = if payment.journal_code.trim().is_empty() {
        parse_string(method.first())
    } else {
        payment.journal_code.trim().to_string()
    };
    let debit_account = if payment.debit_account.trim().is_empty() {
        parse_string(method.get(1))
    } else {
        payment.debit_account.trim().to_string()
    };
    let snapshot_sql = format!(
        "SELECT COALESCE([partner_account],N'') FROM [{schema}].[commercial_document] WHERE [id]={id}",
        schema = SCHEMA,
        id = sql_string(&payment.invoice_id),
    );
    let snapshot_data = db::execute_raw_query(&mut client, &snapshot_sql).await?;
    let mut partner_account = snapshot_data
        .rows
        .first()
        .map(|row| parse_string(row.first()))
        .unwrap_or_default();
    if partner_account.is_empty() {
        partner_account = resolve_invoice_context(&state, &mut client, &document)
            .await?
            .partner_account;
    }
    let schema_sql = format!(
        "SELECT TOP 1 s.[id] FROM [{schema}].[accounting_context_mapping] m JOIN [{schema}].[accounting_schema] s ON s.[id]=m.[schema_id] AND s.[active]=1 WHERE m.[active]=1 AND m.[operation_type]=N'payment_receipt' AND (m.[payment_method_code] IS NULL OR m.[payment_method_code]={method}) ORDER BY CASE WHEN m.[payment_method_code] IS NULL THEN 1 ELSE 0 END,m.[priority] ASC",
        schema = SCHEMA,
        method = sql_string(method_code),
    );
    let schema_data = db::execute_raw_query(&mut client, &schema_sql).await?;
    let payment_schema_id = schema_data
        .rows
        .first()
        .map(|row| parse_string(row.first()))
        .ok_or_else(|| "No active accounting schema matches this payment".to_string())?;
    let sql = format!("BEGIN TRY BEGIN TRANSACTION; INSERT INTO [{schema}].[payment] ([id],[payment_date],[amount],[currency],[payment_method_code],[reference],[journal_code],[debit_account],[partner_account],[resolved_schema_id]) VALUES ({id},{date},{amount},{currency},{method},{reference},{journal},{debit_account},{partner_account},{schema_id}); INSERT INTO [{schema}].[payment_allocation] ([id],[payment_id],[document_id],[amount]) VALUES ({allocation_id},{id},{document_id},{amount}); COMMIT TRANSACTION; END TRY BEGIN CATCH IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION; THROW; END CATCH",
        schema=SCHEMA, id=sql_string(&payment_id), date=sql_string(&payment.payment_date), amount=sql_number(payment.amount),
        currency=sql_string(if payment.currency.trim().is_empty() { &document.currency } else { &payment.currency }), method=sql_string(method_code),
        reference=sql_opt_string(&payment.reference), journal=sql_string(&journal), debit_account=sql_string(&debit_account),
        partner_account=sql_string(&partner_account), schema_id=sql_string(&payment_schema_id),
        allocation_id=sql_string(&Uuid::new_v4().to_string()), document_id=sql_string(&payment.invoice_id));
    db::execute_raw_query(&mut client, &sql).await?;
    Ok(json!({
        "id": payment_id,
        "invoice_id": payment.invoice_id,
        "status": "Registered",
        "posting_status": "Pending",
    }))
}

#[tauri::command]
pub async fn generate_bridge_payment_posting(
    state: State<'_, AppState>,
    id: String,
    payment_id: String,
) -> Result<PostingBatch, String> {
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let existing_sql = format!("SELECT TOP 1 [id] FROM [{schema}].[posting_batch] WHERE [source_kind]=N'payment' AND [source_id]={id}", schema=SCHEMA, id=sql_string(&payment_id));
    let existing = db::execute_raw_query(&mut client, &existing_sql).await?;
    if let Some(row) = existing.rows.first() {
        return fetch_batch(&mut client, &parse_string(row.first())).await;
    }
    let sql = format!(
        "SELECT p.[payment_date],p.[amount],COALESCE(p.[reference],N''),p.[journal_code],p.[debit_account],COALESCE(p.[partner_account],N''),COALESCE(p.[resolved_schema_id],N''),a.[document_id] FROM [{schema}].[payment] p JOIN [{schema}].[payment_allocation] a ON a.[payment_id]=p.[id] WHERE p.[id]={id}",
        schema = SCHEMA,
        id = sql_string(&payment_id),
    );
    let data = db::execute_raw_query(&mut client, &sql).await?;
    let row = data
        .rows
        .first()
        .ok_or_else(|| format!("Registered payment '{}' not found", payment_id))?;
    let posting_date = parse_string(row.first());
    let amount = round2(parse_f64(row.get(1)));
    let reference = parse_string(row.get(2));
    let journal = parse_string(row.get(3));
    let debit_account = parse_string(row.get(4));
    let partner_account = parse_string(row.get(5));
    let schema_id = parse_string(row.get(6));
    let document = fetch_document(&mut client, &parse_string(row.get(7))).await?;
    if partner_account.is_empty() || schema_id.is_empty() {
        return Err("Payment was registered without a complete accounting snapshot".to_string());
    }
    let posting_lines = build_payment_lines_from_schema(
        &mut client,
        &schema_id,
        &document,
        &partner_account,
        &debit_account,
        amount,
    )
    .await?;
    let batch = insert_batch(
        &mut client,
        PostingBatch {
            id: Uuid::new_v4().to_string(),
            source_kind: "payment".into(),
            source_id: payment_id.clone(),
            journal_code: journal,
            piece_number: if reference.trim().is_empty() {
                format!("PAY-{}", document.number)
            } else {
                reference
            },
            posting_date,
            status: "Generated".into(),
            total_debit: 0.0,
            total_credit: 0.0,
            lines: posting_lines,
        },
    )
    .await?;
    db::execute_raw_query(
        &mut client,
        &format!(
            "UPDATE [{schema}].[payment] SET [status]=N'Posted' WHERE [id]={id}",
            schema = SCHEMA,
            id = sql_string(&payment_id),
        ),
    )
    .await?;
    Ok(batch)
}

#[tauri::command]
pub async fn get_bridge_accounting_configuration(
    state: State<'_, AppState>,
    id: String,
) -> Result<AccountingConfiguration, String> {
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let (default_ar, default_sales, default_vat) = {
        let config = state.config.lock().map_err(|error| error.to_string())?;
        (
            config.account_ar.clone(),
            config.account_sales.clone(),
            config.account_vat.clone(),
        )
    };
    let mut output = AccountingConfiguration {
        partner_account: default_ar,
        revenue_account: default_sales,
        tax_rate: 18.0,
        tax_account: default_vat,
        payment_method_code: "bank".into(),
        payment_journal_code: "BQ".into(),
        payment_debit_account: "521000".into(),
        ..AccountingConfiguration::default()
    };
    let partner_data = db::execute_raw_query(
        &mut client,
        &format!(
            "SELECT TOP 1 [partner_code],[collective_account] FROM [{schema}].[partner_accounting_profile] WHERE [active]=1 ORDER BY [partner_code]",
            schema = SCHEMA
        ),
    )
    .await?;
    if let Some(row) = partner_data.rows.first() {
        output.partner_code = parse_string(row.first());
        output.partner_account = parse_string(row.get(1));
    }
    let product_data = db::execute_raw_query(
        &mut client,
        &format!(
            "SELECT TOP 1 p.[product_code],COALESCE(p.[product_family_code],N''),COALESCE(c.[code],p.[accounting_category_code],N''),COALESCE(p.[revenue_account],f.[revenue_account],c.[revenue_account],N''),COALESCE(p.[expense_account],f.[expense_account],c.[expense_account],N''),COALESCE(p.[tax_code],N'') FROM [{schema}].[product_accounting_profile] p LEFT JOIN [{schema}].[product_family] f ON f.[code]=p.[product_family_code] AND f.[active]=1 LEFT JOIN [{schema}].[accounting_category] c ON c.[id]=COALESCE((SELECT TOP 1 pc.[id] FROM [{schema}].[accounting_category] pc WHERE pc.[code]=p.[accounting_category_code] AND pc.[active]=1),f.[accounting_category_id]) AND c.[active]=1 WHERE p.[active]=1 ORDER BY p.[product_code]",
            schema = SCHEMA
        ),
    )
    .await?;
    let mut tax_code = String::new();
    if let Some(row) = product_data.rows.first() {
        output.product_code = parse_string(row.first());
        output.product_family_code = parse_string(row.get(1));
        output.accounting_category_code = parse_string(row.get(2));
        let revenue = parse_string(row.get(3));
        if !revenue.is_empty() {
            output.revenue_account = revenue;
        }
        output.expense_account = parse_string(row.get(4));
        tax_code = parse_string(row.get(5));
    }
    if !tax_code.is_empty() {
        let tax_data = db::execute_raw_query(
            &mut client,
            &format!(
                "SELECT TOP 1 [rate],COALESCE([collected_account],N'') FROM [{schema}].[tax_code] WHERE [code]={code} AND [active]=1",
                schema = SCHEMA,
                code = sql_string(&tax_code),
            ),
        )
        .await?;
        if let Some(row) = tax_data.rows.first() {
            output.tax_rate = round2(parse_f64(row.first()));
            output.tax_account = parse_string(row.get(1));
        }
    }
    let payment_data = db::execute_raw_query(
        &mut client,
        &format!(
            "SELECT TOP 1 [code],[journal_code],[debit_account] FROM [{schema}].[payment_method] WHERE [active]=1 ORDER BY CASE WHEN [code]=N'bank' THEN 0 ELSE 1 END,[code]",
            schema = SCHEMA
        ),
    )
    .await?;
    if let Some(row) = payment_data.rows.first() {
        output.payment_method_code = parse_string(row.first());
        output.payment_journal_code = parse_string(row.get(1));
        output.payment_debit_account = parse_string(row.get(2));
    }
    Ok(output)
}

#[tauri::command]
pub async fn get_bridge_master_data(
    state: State<'_, AppState>,
    id: String,
) -> Result<BridgeMasterData, String> {
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let accounts = native_catalog(&state, &mut client, &id, "account").await?;
    let mut journals = native_catalog(&state, &mut client, &id, "journal").await?;
    let bridge_journals = db::execute_raw_query(
        &mut client,
        &format!(
            "SELECT j.[code],j.[code],j.[name],j.[journal_type],N'',0 FROM [{schema}].[journal] j WHERE j.[active]=1 ORDER BY j.[code]",
            schema = SCHEMA
        ),
    )
    .await?;
    for row in &bridge_journals.rows {
        let item = catalog_item(row, "journal", "bridge");
        if !journals.iter().any(|existing| existing.code == item.code) {
            journals.push(item);
        }
    }

    let categories = db::execute_raw_query(
        &mut client,
        &format!(
            "SELECT [id],[code],[name],N'',COALESCE([revenue_account],N''),0 FROM [{schema}].[accounting_category] WHERE [active]=1 ORDER BY [code]",
            schema = SCHEMA
        ),
    )
    .await?;
    let families = db::execute_raw_query(
        &mut client,
        &format!(
            "SELECT f.[id],f.[code],f.[name],COALESCE(c.[code],N''),N'',0 FROM [{schema}].[product_family] f LEFT JOIN [{schema}].[accounting_category] c ON c.[id]=f.[accounting_category_id] WHERE f.[active]=1 ORDER BY f.[code]",
            schema = SCHEMA
        ),
    )
    .await?;
    let taxes = db::execute_raw_query(
        &mut client,
        &format!(
            "SELECT t.[code],t.[code],t.[name],t.[tax_type],COALESCE(t.[collected_account],N''),t.[rate] FROM [{schema}].[tax_code] t WHERE t.[active]=1 ORDER BY t.[code]",
            schema = SCHEMA
        ),
    )
    .await?;
    let payment_methods = db::execute_raw_query(
        &mut client,
        &format!(
            "SELECT p.[code],p.[code],p.[name],p.[journal_code],p.[debit_account],0 FROM [{schema}].[payment_method] p WHERE p.[active]=1 ORDER BY p.[code]",
            schema = SCHEMA
        ),
    )
    .await?;
    let profiles = db::execute_raw_query(
        &mut client,
        &format!(
            "SELECT [product_code],COALESCE([product_family_code],N''),COALESCE([accounting_category_code],N''),COALESCE([tax_code],N''),COALESCE([revenue_account],N''),[active] FROM [{schema}].[product_accounting_profile] ORDER BY [product_code]",
            schema = SCHEMA
        ),
    )
    .await?;

    Ok(BridgeMasterData {
        accounts,
        journals,
        product_families: families
            .rows
            .iter()
            .map(|row| catalog_item(row, "product_family", "bridge"))
            .collect(),
        accounting_categories: categories
            .rows
            .iter()
            .map(|row| catalog_item(row, "accounting_category", "bridge"))
            .collect(),
        tax_codes: taxes
            .rows
            .iter()
            .map(|row| catalog_item(row, "tax_code", "bridge"))
            .collect(),
        payment_methods: payment_methods
            .rows
            .iter()
            .map(|row| catalog_item(row, "payment_method", "bridge"))
            .collect(),
        product_profiles: profiles
            .rows
            .iter()
            .map(|row| ProductAccountingProfile {
                product_code: parse_string(row.first()),
                product_family_code: parse_string(row.get(1)),
                accounting_category_code: parse_string(row.get(2)),
                tax_code: parse_string(row.get(3)),
                revenue_account: parse_string(row.get(4)),
                active: parse_string(row.get(5)) != "0",
            })
            .collect(),
    })
}

#[tauri::command]
pub async fn save_bridge_product_profile(
    state: State<'_, AppState>,
    id: String,
    profile: ProductAccountingProfile,
) -> Result<ProductAccountingProfile, String> {
    if profile.product_code.trim().is_empty()
        || profile.product_family_code.trim().is_empty()
        || profile.accounting_category_code.trim().is_empty()
        || profile.tax_code.trim().is_empty()
    {
        return Err(
            "Article, famille, catégorie comptable et code taxe sont obligatoires".to_string(),
        );
    }
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let revenue_account = profile.revenue_account.trim();
    let sql = format!(
        "MERGE [{schema}].[product_accounting_profile] AS target USING (SELECT {product} AS [product_code]) AS source ON target.[product_code]=source.[product_code] WHEN MATCHED THEN UPDATE SET [product_family_code]={family},[accounting_category_code]={category},[tax_code]={tax},[revenue_account]={revenue},[expense_account]=NULL,[active]=1 WHEN NOT MATCHED THEN INSERT ([product_code],[product_family_code],[accounting_category_code],[tax_code],[revenue_account],[active]) VALUES (source.[product_code],{family},{category},{tax},{revenue},1);",
        schema = SCHEMA,
        product = sql_string(profile.product_code.trim()),
        family = sql_string(profile.product_family_code.trim()),
        category = sql_string(profile.accounting_category_code.trim()),
        tax = sql_string(profile.tax_code.trim()),
        revenue = sql_opt_string(revenue_account),
    );
    db::execute_raw_query(&mut client, &sql).await?;
    Ok(ProductAccountingProfile {
        active: true,
        ..profile
    })
}

#[tauri::command]
pub async fn get_bridge_management_data(
    state: State<'_, AppState>,
    id: String,
) -> Result<BridgeManagementData, String> {
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let scalar = |row: Option<&Vec<Value>>, index: usize| -> i64 {
        row.and_then(|values| values.get(index))
            .and_then(Value::as_i64)
            .unwrap_or_default()
    };
    let dashboard_data = db::execute_raw_query(
        &mut client,
        &format!(
            "SELECT
            (SELECT COUNT(*) FROM [{schema}].[commercial_document]) AS documents,
            (SELECT COUNT(*) FROM [{schema}].[commercial_document] WHERE [status]=N'Draft') AS draft_documents,
            (SELECT COUNT(*) FROM [{schema}].[posting_batch]) AS posting_batches,
            (SELECT COUNT(*) FROM [{schema}].[export_job] WHERE [status]<>N'Exported') AS pending_exports,
            (SELECT COUNT(*) FROM [{schema}].[product_accounting_profile] WHERE [active]=1) AS mapped_products,
            (SELECT COUNT(*) FROM [{schema}].[partner_accounting_profile] WHERE [active]=1) AS mapped_partners",
            schema = SCHEMA
        ),
    )
    .await?;
    let dashboard_row = dashboard_data.rows.first();
    let partner_profiles = query_rows_as_values(&mut client, &format!(
        "SELECT [partner_code],[partner_type],COALESCE([accounting_category_code],N''),[collective_account],[active] FROM [{schema}].[partner_accounting_profile] ORDER BY [partner_code]",
        schema = SCHEMA
    ))
    .await?;
    let accounting_schemas = query_rows_as_values(&mut client, &format!(
        "SELECT s.[id],s.[code],s.[name],s.[operation_type],s.[direction],s.[journal_code],s.[edition_scope],s.[active],(SELECT COUNT(*) FROM [{schema}].[accounting_schema_line] l WHERE l.[schema_id]=s.[id] AND l.[active]=1) FROM [{schema}].[accounting_schema] s ORDER BY s.[code]",
        schema = SCHEMA
    ))
    .await?;
    let context_mappings = query_rows_as_values(&mut client, &format!(
        "SELECT m.[id],m.[operation_type],COALESCE(m.[document_type],N''),COALESCE(m.[partner_category_code],N''),COALESCE(m.[product_category_code],N''),COALESCE(m.[payment_method_code],N''),s.[code],m.[priority],m.[active] FROM [{schema}].[accounting_context_mapping] m LEFT JOIN [{schema}].[accounting_schema] s ON s.[id]=m.[schema_id] ORDER BY m.[priority],m.[operation_type]",
        schema = SCHEMA
    ))
    .await?;
    let document_journal_mappings = query_rows_as_values(&mut client, &format!(
        "SELECT [document_type],[journal_code] FROM [{schema}].[document_journal_mapping] ORDER BY [document_type]",
        schema = SCHEMA
    ))
    .await?;
    let integration_profiles = query_rows_as_values(&mut client, &format!(
        "SELECT [id],[name],[adapter_type],[sage_edition],[delivery_enabled],[active],COALESCE([configuration_json],N'') FROM [{schema}].[integration_profile] ORDER BY [name]",
        schema = SCHEMA
    ))
    .await?;
    let export_jobs = query_rows_as_values(&mut client, &format!(
        "SELECT j.[id],j.[batch_id],j.[adapter_type],j.[status],COALESCE(j.[external_id],N''),COALESCE(j.[last_error],N''),CONVERT(VARCHAR(19),j.[created_at],120),CONVERT(VARCHAR(19),j.[updated_at],120),(SELECT COUNT(*) FROM [{schema}].[export_attempt] a WHERE a.[export_job_id]=j.[id]) FROM [{schema}].[export_job] j ORDER BY j.[created_at] DESC",
        schema = SCHEMA
    ))
    .await?;
    Ok(BridgeManagementData {
        dashboard: json!({
            "documents": scalar(dashboard_row, 0),
            "draft_documents": scalar(dashboard_row, 1),
            "posting_batches": scalar(dashboard_row, 2),
            "pending_exports": scalar(dashboard_row, 3),
            "mapped_products": scalar(dashboard_row, 4),
            "mapped_partners": scalar(dashboard_row, 5),
        }),
        partner_profiles,
        accounting_schemas,
        context_mappings,
        document_journal_mappings,
        integration_profiles,
        export_jobs,
    })
}

#[tauri::command]
pub async fn save_bridge_management_record(
    state: State<'_, AppState>,
    id: String,
    entity: String,
    record: Value,
) -> Result<(), String> {
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let sql = match entity.as_str() {
        "accounting_category" => {
            let code = value_string(&record, "code");
            if code.is_empty() {
                return Err("Category code is required".to_string());
            }
            format!(
                "MERGE [{schema}].[accounting_category] t USING (SELECT {id} [id],{code} [code]) s ON t.[code]=s.[code] WHEN MATCHED THEN UPDATE SET [name]={name},[revenue_account]={revenue},[expense_account]={expense},[tax_account]={tax},[active]={active} WHEN NOT MATCHED THEN INSERT ([id],[code],[name],[revenue_account],[expense_account],[tax_account],[active]) VALUES (s.[id],s.[code],{name},{revenue},{expense},{tax},{active});",
                schema=SCHEMA, id=sql_string(&non_empty_or(value_string(&record, "id"), Uuid::new_v4().to_string())),
                code=sql_string(&code), name=sql_string(&value_string(&record, "name")),
                revenue=sql_opt_string(&value_string(&record, "revenue_account")),
                expense=sql_opt_string(&value_string(&record, "expense_account")),
                tax=sql_opt_string(&value_string(&record, "tax_account")),
                active=if value_bool(&record, "active", true) {1} else {0}
            )
        }
        "product_family" => {
            let code = value_string(&record, "code");
            if code.is_empty() {
                return Err("Family code is required".to_string());
            }
            format!(
                "MERGE [{schema}].[product_family] t USING (SELECT {id} [id],{code} [code]) s ON t.[code]=s.[code] WHEN MATCHED THEN UPDATE SET [name]={name},[accounting_category_id]=(SELECT TOP 1 category.[id] FROM [{schema}].[accounting_category] category WHERE category.[code]={category}),[active]={active} WHEN NOT MATCHED THEN INSERT ([id],[code],[name],[accounting_category_id],[active]) VALUES (s.[id],s.[code],{name},(SELECT TOP 1 category.[id] FROM [{schema}].[accounting_category] category WHERE category.[code]={category}),{active});",
                schema=SCHEMA, id=sql_string(&non_empty_or(value_string(&record, "id"), Uuid::new_v4().to_string())),
                code=sql_string(&code), name=sql_string(&value_string(&record, "name")),
                category=sql_string(&value_string(&record, "accounting_category_code")),
                active=if value_bool(&record, "active", true) {1} else {0}
            )
        }
        "tax_code" => format!(
            "MERGE [{schema}].[tax_code] t USING (SELECT {code} [code]) s ON t.[code]=s.[code] WHEN MATCHED THEN UPDATE SET [name]={name},[tax_type]={tax_type},[rate]={rate},[collected_account]={collected},[deductible_account]={deductible},[active]={active} WHEN NOT MATCHED THEN INSERT ([code],[name],[tax_type],[rate],[collected_account],[deductible_account],[active]) VALUES (s.[code],{name},{tax_type},{rate},{collected},{deductible},{active});",
            schema=SCHEMA, code=sql_string(&value_string(&record, "code")),
            name=sql_string(&value_string(&record, "name")), tax_type=sql_string(&value_string(&record, "tax_type")),
            rate=sql_number(value_f64(&record, "rate", 0.0)), collected=sql_opt_string(&value_string(&record, "collected_account")),
            deductible=sql_opt_string(&value_string(&record, "deductible_account")), active=if value_bool(&record, "active", true) {1} else {0}
        ),
        "payment_method" => format!(
            "MERGE [{schema}].[payment_method] t USING (SELECT {code} [code]) s ON t.[code]=s.[code] WHEN MATCHED THEN UPDATE SET [name]={name},[journal_code]={journal},[debit_account]={account},[active]={active} WHEN NOT MATCHED THEN INSERT ([code],[name],[journal_code],[debit_account],[active]) VALUES (s.[code],{name},{journal},{account},{active});",
            schema=SCHEMA, code=sql_string(&value_string(&record, "code")), name=sql_string(&value_string(&record, "name")),
            journal=sql_string(&value_string(&record, "journal_code")), account=sql_string(&value_string(&record, "debit_account")),
            active=if value_bool(&record, "active", true) {1} else {0}
        ),
        "partner_profile" => format!(
            "MERGE [{schema}].[partner_accounting_profile] t USING (SELECT {code} [partner_code]) s ON t.[partner_code]=s.[partner_code] WHEN MATCHED THEN UPDATE SET [partner_type]={kind},[accounting_category_code]={category},[collective_account]={account},[active]={active} WHEN NOT MATCHED THEN INSERT ([partner_code],[partner_type],[accounting_category_code],[collective_account],[active]) VALUES (s.[partner_code],{kind},{category},{account},{active});",
            schema=SCHEMA, code=sql_string(&value_string(&record, "partner_code")), kind=sql_string(&value_string(&record, "partner_type")),
            category=sql_opt_string(&value_string(&record, "accounting_category_code")), account=sql_string(&value_string(&record, "collective_account")),
            active=if value_bool(&record, "active", true) {1} else {0}
        ),
        "accounting_schema" => {
            let schema_id = non_empty_or(value_string(&record, "id"), Uuid::new_v4().to_string());
            let operation = value_string(&record, "operation_type");
            let default_lines = if operation == "payment_receipt" {
                format!(
                    "IF NOT EXISTS (SELECT 1 FROM [{schema}].[accounting_schema_line] WHERE [schema_id]={id}) INSERT INTO [{schema}].[accounting_schema_line] ([id],[schema_id],[line_no],[posting_role],[debit_credit],[amount_basis],[account_source],[grouping_rule]) VALUES ({line1},{id},1,N'bank',N'debit',N'payment_amount',N'PAYMENT_METHOD_ACCOUNT',N'document'),({line2},{id},2,N'customer',N'credit',N'payment_amount',N'CUSTOMER_COLLECTIVE_ACCOUNT',N'document');",
                    schema=SCHEMA, id=sql_string(&schema_id), line1=sql_string(&Uuid::new_v4().to_string()), line2=sql_string(&Uuid::new_v4().to_string())
                )
            } else {
                format!(
                    "IF NOT EXISTS (SELECT 1 FROM [{schema}].[accounting_schema_line] WHERE [schema_id]={id}) INSERT INTO [{schema}].[accounting_schema_line] ([id],[schema_id],[line_no],[posting_role],[debit_credit],[amount_basis],[account_source],[grouping_rule]) VALUES ({line1},{id},1,N'customer',N'debit',N'document_total_ttc',N'CUSTOMER_COLLECTIVE_ACCOUNT',N'document'),({line2},{id},2,N'sales',N'credit',N'line_amount_ht',N'RESOLVED_REVENUE_ACCOUNT',N'account'),({line3},{id},3,N'tax',N'credit',N'line_tax_amount',N'RESOLVED_TAX_ACCOUNT',N'account');",
                    schema=SCHEMA, id=sql_string(&schema_id), line1=sql_string(&Uuid::new_v4().to_string()), line2=sql_string(&Uuid::new_v4().to_string()), line3=sql_string(&Uuid::new_v4().to_string())
                )
            };
            format!(
                "BEGIN TRY BEGIN TRANSACTION; MERGE [{schema}].[accounting_schema] t USING (SELECT {id} [id]) s ON t.[id]=s.[id] WHEN MATCHED THEN UPDATE SET [code]={code},[name]={name},[operation_type]={operation},[direction]={direction},[journal_code]={journal},[edition_scope]={edition},[active]={active} WHEN NOT MATCHED THEN INSERT ([id],[code],[name],[operation_type],[direction],[journal_code],[edition_scope],[active]) VALUES (s.[id],{code},{name},{operation},{direction},{journal},{edition},{active}); {lines} COMMIT TRANSACTION; END TRY BEGIN CATCH IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION; THROW; END CATCH",
                schema=SCHEMA, id=sql_string(&schema_id), code=sql_string(&value_string(&record, "code")),
                name=sql_string(&value_string(&record, "name")), operation=sql_string(&operation),
                direction=sql_string(&value_string(&record, "direction")), journal=sql_string(&value_string(&record, "journal_code")),
                edition=sql_string(&value_string(&record, "edition_scope")), active=if value_bool(&record, "active", true) {1} else {0},
                lines=default_lines
            )
        }
        "document_journal_mapping" => format!(
            "MERGE [{schema}].[document_journal_mapping] t USING (SELECT {kind} [document_type]) s ON t.[document_type]=s.[document_type] WHEN MATCHED THEN UPDATE SET [journal_code]={journal} WHEN NOT MATCHED THEN INSERT ([document_type],[journal_code]) VALUES (s.[document_type],{journal});",
            schema=SCHEMA, kind=sql_string(&value_string(&record, "document_type")), journal=sql_string(&value_string(&record, "journal_code"))
        ),
        "integration_profile" => format!(
            "MERGE [{schema}].[integration_profile] t USING (SELECT {id} [id]) s ON t.[id]=s.[id] WHEN MATCHED THEN UPDATE SET [name]={name},[adapter_type]={adapter},[sage_edition]={edition},[delivery_enabled]=0,[configuration_json]={config},[active]={active} WHEN NOT MATCHED THEN INSERT ([id],[name],[adapter_type],[sage_edition],[delivery_enabled],[configuration_json],[active]) VALUES (s.[id],{name},{adapter},{edition},0,{config},{active});",
            schema=SCHEMA, id=sql_string(&non_empty_or(value_string(&record, "id"), Uuid::new_v4().to_string())),
            name=sql_string(&value_string(&record, "name")), adapter=sql_string(&value_string(&record, "adapter_type")),
            edition=sql_string(&value_string(&record, "sage_edition")), config=sql_opt_string(&value_string(&record, "configuration_json")),
            active=if value_bool(&record, "active", true) {1} else {0}
        ),
        _ => return Err(format!("Unsupported bridge management entity '{}'", entity)),
    };
    db::execute_raw_query(&mut client, &sql).await?;
    Ok(())
}

#[tauri::command]
pub async fn deactivate_bridge_management_record(
    state: State<'_, AppState>,
    id: String,
    entity: String,
    key: String,
) -> Result<(), String> {
    let (table, column) = match entity.as_str() {
        "accounting_category" => ("accounting_category", "code"),
        "product_family" => ("product_family", "code"),
        "tax_code" => ("tax_code", "code"),
        "payment_method" => ("payment_method", "code"),
        "partner_profile" => ("partner_accounting_profile", "partner_code"),
        "product_profile" => ("product_accounting_profile", "product_code"),
        "integration_profile" => ("integration_profile", "id"),
        "accounting_schema" => ("accounting_schema", "id"),
        _ => return Err(format!("Unsupported bridge management entity '{}'", entity)),
    };
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    db::execute_raw_query(
        &mut client,
        &format!(
            "UPDATE [{schema}].[{table}] SET [active]=0 WHERE [{column}]={key}",
            schema = SCHEMA,
            table = table,
            column = column,
            key = sql_string(&key),
        ),
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn preview_bridge_accounting(
    state: State<'_, AppState>,
    id: String,
    invoice: InvoiceHeader,
) -> Result<AccountingReadiness, String> {
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let mut preview = AccountingReadiness {
        schema_code: "SALE_STANDARD".to_string(),
        ..AccountingReadiness::default()
    };
    let schema_data = db::execute_raw_query(
        &mut client,
        &format!(
            "SELECT TOP 1 COALESCE([journal_code],N'') FROM [{schema}].[accounting_schema] WHERE [code]=N'SALE_STANDARD' AND [active]=1",
            schema = SCHEMA
        ),
    )
    .await?;
    if let Some(row) = schema_data.rows.first() {
        preview.journal_code = parse_string(row.first());
    }
    let partner_data = db::execute_raw_query(
        &mut client,
        &format!(
            "SELECT TOP 1 COALESCE([collective_account],N'') FROM [{schema}].[partner_accounting_profile] WHERE [partner_code]={partner} AND [active]=1",
            schema = SCHEMA,
            partner = sql_string(invoice.tiers_code.trim()),
        ),
    )
    .await?;
    if let Some(row) = partner_data.rows.first() {
        preview.customer_account = parse_string(row.first());
    } else {
        preview.issues.push(format!(
            "Aucun profil comptable pour le client {}",
            invoice.tiers_code
        ));
    }

    for line in &invoice.lignes {
        if (line.article_id.starts_with("sdb-") || line.article_id.starts_with("local-item-"))
            && !looks_like_revenue_account(&line.article_code)
        {
            preview.issues.push(format!(
                "L’article {} est en attente de synchronisation Sage",
                line.article_code
            ));
        }
        let data = db::execute_raw_query(
            &mut client,
            &format!(
                "SELECT TOP 1 COALESCE(p.[product_family_code],N''),COALESCE(p.[accounting_category_code],N''),COALESCE(p.[tax_code],N''),COALESCE(p.[revenue_account],c.[revenue_account],N''),COALESCE(t.[collected_account],N'') FROM [{schema}].[product_accounting_profile] p LEFT JOIN [{schema}].[accounting_category] c ON c.[code]=p.[accounting_category_code] AND c.[active]=1 LEFT JOIN [{schema}].[tax_code] t ON t.[code]=p.[tax_code] AND t.[active]=1 WHERE p.[product_code]={product} AND p.[active]=1",
                schema = SCHEMA,
                product = sql_string(line.article_code.trim()),
            ),
        )
        .await?;
        let mut resolved = AccountingPreviewLine {
            product_code: line.article_code.clone(),
            ..AccountingPreviewLine::default()
        };
        if let Some(row) = data.rows.first() {
            resolved.product_family_code = parse_string(row.first());
            resolved.accounting_category_code = parse_string(row.get(1));
            resolved.tax_code = parse_string(row.get(2));
            resolved.revenue_account = parse_string(row.get(3));
            resolved.tax_account = parse_string(row.get(4));
        }
        if resolved.product_family_code.trim().is_empty() {
            resolved.product_family_code = line.product_family_code.clone();
        }
        if resolved.accounting_category_code.trim().is_empty() {
            resolved.accounting_category_code = line.accounting_category_code.clone();
        }
        if resolved.tax_code.trim().is_empty() {
            resolved.tax_code = line.tax_code.clone();
        }
        if resolved.revenue_account.trim().is_empty() {
            resolved.revenue_account = line.revenue_account.clone();
        }
        if resolved.tax_account.trim().is_empty() {
            resolved.tax_account = line.vat_account.clone();
        }
        resolved.ready = !resolved.product_family_code.is_empty()
            && !resolved.accounting_category_code.is_empty()
            && !resolved.tax_code.is_empty()
            && !resolved.revenue_account.is_empty()
            && (line.taux_tva == 0.0 || !resolved.tax_account.is_empty());
        if !resolved.ready {
            preview.issues.push(format!(
                "Configuration comptable incomplète pour l’article {}",
                line.article_code
            ));
        }
        preview.lines.push(resolved);
    }
    if preview.journal_code.is_empty() {
        preview
            .issues
            .push("Journal de vente introuvable".to_string());
    }
    if preview.customer_account.is_empty() {
        preview
            .issues
            .push("Compte collectif client introuvable".to_string());
    }
    preview.ready = preview.issues.is_empty() && !preview.lines.is_empty();
    Ok(preview)
}

#[tauri::command]
pub async fn save_bridge_accounting_configuration(
    state: State<'_, AppState>,
    id: String,
    configuration: AccountingConfiguration,
) -> Result<AccountingConfiguration, String> {
    let required = [
        ("customer code", configuration.partner_code.trim()),
        ("customer account", configuration.partner_account.trim()),
        ("product code", configuration.product_code.trim()),
        ("product family", configuration.product_family_code.trim()),
        (
            "accounting category",
            configuration.accounting_category_code.trim(),
        ),
        ("revenue account", configuration.revenue_account.trim()),
        ("tax account", configuration.tax_account.trim()),
        ("payment method", configuration.payment_method_code.trim()),
        ("payment journal", configuration.payment_journal_code.trim()),
        (
            "payment debit account",
            configuration.payment_debit_account.trim(),
        ),
    ];
    if let Some((name, _)) = required.iter().find(|(_, value)| value.is_empty()) {
        return Err(format!("Accounting configuration requires {}", name));
    }
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let category_id = Uuid::new_v4().to_string();
    let family_id = Uuid::new_v4().to_string();
    let tax_code = tax_code_for_rate(configuration.tax_rate);
    let sql = format!(
        r#"
BEGIN TRY
BEGIN TRANSACTION;
MERGE [{schema}].[accounting_category] AS target
USING (SELECT {category_id} AS [id],{category_code} AS [code]) AS source ON target.[code]=source.[code]
WHEN MATCHED THEN UPDATE SET [name]=source.[code],[revenue_account]={revenue},[expense_account]={expense},[tax_account]={tax_account},[active]=1
WHEN NOT MATCHED THEN INSERT ([id],[code],[name],[revenue_account],[expense_account],[tax_account]) VALUES (source.[id],source.[code],source.[code],{revenue},{expense},{tax_account});

MERGE [{schema}].[product_family] AS target
USING (SELECT {family_id} AS [id],{family_code} AS [code]) AS source ON target.[code]=source.[code]
WHEN MATCHED THEN UPDATE SET [name]=source.[code],[accounting_category_id]=(SELECT TOP 1 category.[id] FROM [{schema}].[accounting_category] category WHERE category.[code]={category_code}),[revenue_account]=NULL,[expense_account]=NULL,[tax_account]=NULL,[active]=1
WHEN NOT MATCHED THEN INSERT ([id],[code],[name],[accounting_category_id]) VALUES (source.[id],source.[code],source.[code],(SELECT TOP 1 category.[id] FROM [{schema}].[accounting_category] category WHERE category.[code]={category_code}));

MERGE [{schema}].[tax_code] AS target
USING (SELECT {tax_code} AS [code]) AS source ON target.[code]=source.[code]
WHEN MATCHED THEN UPDATE SET [name]=source.[code],[tax_type]=N'collected',[rate]={tax_rate},[collected_account]={tax_account},[active]=1
WHEN NOT MATCHED THEN INSERT ([code],[name],[tax_type],[rate],[collected_account]) VALUES (source.[code],source.[code],N'collected',{tax_rate},{tax_account});

MERGE [{schema}].[product_accounting_profile] AS target
USING (SELECT {product_code} AS [product_code]) AS source ON target.[product_code]=source.[product_code]
WHEN MATCHED THEN UPDATE SET [product_family_code]={family_code},[accounting_category_code]={category_code},[revenue_account]={revenue},[expense_account]=NULL,[tax_code]={tax_code},[active]=1
WHEN NOT MATCHED THEN INSERT ([product_code],[product_family_code],[accounting_category_code],[revenue_account],[tax_code]) VALUES (source.[product_code],{family_code},{category_code},{revenue},{tax_code});

MERGE [{schema}].[partner_accounting_profile] AS target
USING (SELECT {partner_code} AS [partner_code]) AS source ON target.[partner_code]=source.[partner_code]
WHEN MATCHED THEN UPDATE SET [partner_type]=N'customer',[collective_account]={partner_account},[active]=1
WHEN NOT MATCHED THEN INSERT ([partner_code],[partner_type],[collective_account]) VALUES (source.[partner_code],N'customer',{partner_account});

MERGE [{schema}].[journal] AS target
USING (SELECT {payment_journal} AS [code]) AS source ON target.[code]=source.[code]
WHEN MATCHED THEN UPDATE SET [active]=1
WHEN NOT MATCHED THEN INSERT ([code],[name],[journal_type]) VALUES (source.[code],source.[code],N'payment');

MERGE [{schema}].[payment_method] AS target
USING (SELECT {payment_method} AS [code]) AS source ON target.[code]=source.[code]
WHEN MATCHED THEN UPDATE SET [name]=source.[code],[journal_code]={payment_journal},[debit_account]={payment_account},[active]=1
WHEN NOT MATCHED THEN INSERT ([code],[name],[journal_code],[debit_account]) VALUES (source.[code],source.[code],{payment_journal},{payment_account});
COMMIT TRANSACTION;
END TRY
BEGIN CATCH IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION; THROW; END CATCH
"#,
        schema = SCHEMA,
        category_id = sql_string(&category_id),
        category_code = sql_string(configuration.accounting_category_code.trim()),
        revenue = sql_string(configuration.revenue_account.trim()),
        expense = sql_opt_string(configuration.expense_account.trim()),
        tax_account = sql_string(configuration.tax_account.trim()),
        family_id = sql_string(&family_id),
        family_code = sql_string(configuration.product_family_code.trim()),
        tax_code = sql_string(&tax_code),
        tax_rate = sql_number(configuration.tax_rate),
        product_code = sql_string(configuration.product_code.trim()),
        partner_code = sql_string(configuration.partner_code.trim()),
        partner_account = sql_string(configuration.partner_account.trim()),
        payment_journal = sql_string(configuration.payment_journal_code.trim()),
        payment_method = sql_string(configuration.payment_method_code.trim()),
        payment_account = sql_string(configuration.payment_debit_account.trim()),
    );
    db::execute_raw_query(&mut client, &sql).await?;
    Ok(configuration)
}

#[tauri::command]
pub async fn list_bridge_posting_batches(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<PostingBatch>, String> {
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let data = db::execute_raw_query(
        &mut client,
        &format!(
            "SELECT [id] FROM [{schema}].[posting_batch] ORDER BY [created_at] DESC",
            schema = SCHEMA
        ),
    )
    .await?;
    let mut batches = Vec::new();
    for row in &data.rows {
        batches.push(fetch_batch(&mut client, &parse_string(row.first())).await?);
    }
    Ok(batches)
}

#[tauri::command]
pub async fn prepare_bridge_export(
    state: State<'_, AppState>,
    id: String,
    batch_id: String,
    adapter_type: String,
) -> Result<Value, String> {
    let mut client = client(&state, &id).await?;
    ensure_schema(&mut client).await?;
    let batch = fetch_batch(&mut client, &batch_id).await?;
    let adapter = adapter_for(&adapter_type);
    adapter.validate(&batch)?;
    let package = json!({
        "schema_version": 1,
        "adapter": adapter.adapter_type(),
        "delivery_enabled": false,
        "payload": adapter.build_package(&batch),
    });
    let payload = serde_json::to_string(&package).map_err(|error| error.to_string())?;
    let job_id = Uuid::new_v4().to_string();
    let sql = format!("IF NOT EXISTS (SELECT 1 FROM [{schema}].[export_job] WHERE [batch_id]={batch_id} AND [adapter_type]={adapter}) INSERT INTO [{schema}].[export_job] ([id],[batch_id],[adapter_type],[status],[payload_json]) VALUES ({job_id},{batch_id},{adapter},N'Prepared',{payload});",
        schema=SCHEMA, batch_id=sql_string(&batch_id), adapter=sql_string(adapter.adapter_type()), job_id=sql_string(&job_id), payload=sql_string(&payload));
    db::execute_raw_query(&mut client, &sql).await?;
    Ok(package)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_posting_lines_are_balanced() {
        let mut lines = Vec::new();
        push_schema_posting_line(
            &mut lines,
            "customer",
            "debit",
            "411000".into(),
            "C0001".into(),
            "Facture FAC-2026-000001".into(),
            1180.0,
        )
        .unwrap();
        push_schema_posting_line(
            &mut lines,
            "sales",
            "credit",
            "701000".into(),
            String::new(),
            "Facture FAC-2026-000001".into(),
            1000.0,
        )
        .unwrap();
        push_schema_posting_line(
            &mut lines,
            "tax",
            "credit",
            "443000".into(),
            String::new(),
            "Facture FAC-2026-000001".into(),
            180.0,
        )
        .unwrap();
        let debit = round2(lines.iter().map(|line| line.debit).sum());
        let credit = round2(lines.iter().map(|line| line.credit).sum());

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].general_account, "411000");
        assert_eq!(debit, 1180.0);
        assert_eq!(credit, 1180.0);
    }

    #[test]
    fn document_number_prefixes_are_distinct() {
        assert_eq!(prefix("Devis"), "DEV");
        assert_eq!(prefix("Facture"), "FAC");
        assert_eq!(prefix("Avoir"), "AVO");
    }

    #[test]
    fn bridge_tax_codes_use_french_ohada_prefix() {
        assert_eq!(tax_code_for_rate(18.0), "TVA18");
        assert_eq!(tax_code_for_rate(18.5), "TVA18_50");
    }

    #[test]
    fn sage_adapters_keep_delivery_disabled() {
        let batch = PostingBatch {
            journal_code: "VT".into(),
            piece_number: "FAC-2026-000001".into(),
            total_debit: 118.0,
            total_credit: 118.0,
            lines: vec![
                PostingLine {
                    general_account: "411000".into(),
                    debit: 118.0,
                    ..PostingLine::default()
                },
                PostingLine {
                    general_account: "701000".into(),
                    credit: 118.0,
                    ..PostingLine::default()
                },
            ],
            ..PostingBatch::default()
        };
        let adapter = adapter_for("sage100");

        adapter.validate(&batch).unwrap();
        assert_eq!(adapter.build_package(&batch)["delivery_enabled"], false);
    }
}
