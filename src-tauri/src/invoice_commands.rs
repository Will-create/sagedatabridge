use std::collections::BTreeMap;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;
use uuid::Uuid;

use crate::db;
use crate::invoice_compat::InvoiceSchema;
use crate::sage_compat::{SageEdition, SageSchema};
use crate::state::{AppState, ColumnInfo, ConnectionConfig};

use crate::sage_entity_service::{
    self, br, parse_bool, parse_f64, parse_i32, parse_i64, parse_string, qualify, round2,
    sql_opt_string, sql_string, ArticleSummary, ResolvedTable, TiersSummary,
};

const STATUS_TABLE: &str = "SDB_PIECE_STATUS";
const STATUS_HISTORY_TABLE: &str = "SDB_PIECE_STATUS_HISTORY";
const COUNTER_TABLE: &str = "SDB_COUNTER";
const META_TABLE: &str = "SDB_PIECE_META";
const LINE_META_TABLE: &str = "SDB_PIECE_LINE_META";
const COMPTA_LOG_TABLE: &str = "SDB_COMPTA_ENTRY_LOG";
const LOCAL_TIERS_TABLE: &str = "SDB_TIERS";
const LOCAL_ARTICLE_TABLE: &str = "SDB_ARTICLE";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatusHistoryEntry {
    pub id: String,
    pub from_statut: String,
    pub to_statut: String,
    pub updated_at: String,
    pub updated_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InvoiceLine {
    pub id: String,
    pub ordre: i32,
    pub article_id: String,
    pub article_code: String,
    pub libelle: String,
    pub quantite: f64,
    pub unite: String,
    pub prix_ht: f64,
    pub remise_pct: f64,
    pub montant_ht: f64,
    pub taux_tva: f64,
    pub montant_tva: f64,
    pub montant_ttc: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InvoiceHeader {
    pub id: String,
    pub numero: String,
    pub date: String,
    pub date_echeance: String,
    pub tiers_id: String,
    pub tiers_code: String,
    pub tiers_nom: String,
    pub tiers_adresse: String,
    pub tiers_cp: String,
    pub tiers_ville: String,
    pub tiers_siret: String,
    #[serde(default)]
    pub tiers_pays: String,
    #[serde(default)]
    pub tiers_tva_intra: String,
    pub nature: String,
    pub statut: String,
    pub reference: String,
    pub devise: String,
    pub total_ht: f64,
    pub total_tva: f64,
    pub total_ttc: f64,
    pub lignes: Vec<InvoiceLine>,
    pub notes: String,
    pub conditions: String,
    #[serde(default)]
    pub remise_globale: f64,
    #[serde(default)]
    pub is_supplier: bool,
    #[serde(default)]
    pub statut_history: Vec<StatusHistoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StatutUpdateResult {
    pub piece_id: String,
    pub statut: String,
    pub updated_at: String,
    pub updated_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComptabilisationResult {
    pub piece_id: String,
    pub entry_ids: Vec<String>,
    pub statut: String,
}

#[derive(Debug, Clone)]
struct ResolvedInvoiceSchema {
    edition: SageEdition,
    sage_schema: SageSchema,
    native: InvoiceSchema,
    piece: ResolvedTable,
    line: ResolvedTable,
    article: ResolvedTable,
    tiers: ResolvedTable,
    role_tiers: Option<ResolvedTable>,
    journal: Option<ResolvedTable>,
    piece_oid: String,
    piece_numero: String,
    piece_date: String,
    piece_reference: String,
    piece_tiers: String,
    piece_nature: String,
    piece_devise: Option<String>,
    piece_journal_fk: Option<String>,
    line_piece: String,
    line_article: Option<String>,
    line_libelle: Option<String>,
    line_qte: Option<String>,
    line_pu_ht: Option<String>,
    line_taux_tva: Option<String>,
    line_montant_ht: Option<String>,
    line_montant_ttc: Option<String>,
    line_remise: Option<String>,
    line_ordre: Option<String>,
    line_unite: Option<String>,
    article_id: Option<String>,
    article_code: String,
    article_libelle: String,
    article_pu: Option<String>,
    article_tva: Option<String>,
    article_ref: Option<String>,
    article_unite: Option<String>,
    article_active: Option<String>,
    tiers_id: String,
    tiers_code: String,
    tiers_nom: String,
    tiers_adresse: Option<String>,
    tiers_cp: Option<String>,
    tiers_ville: Option<String>,
    tiers_pays: Option<String>,
    tiers_siret: Option<String>,
    tiers_email: Option<String>,
    tiers_telephone: Option<String>,
    tiers_tva: Option<String>,
    tiers_type: Option<String>,
    tiers_account: Option<String>,
    role_tiers_tiers: Option<String>,
    role_tiers_account: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct InvoiceSupportTableAvailability {
    status: bool,
    status_history: bool,
    meta: bool,
    line_meta: bool,
    local_tiers: bool,
    local_article: bool,
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

fn is_numeric_type(data_type: &str) -> bool {
    let dt = data_type.to_ascii_lowercase();
    dt.contains("int")
        || dt.contains("decimal")
        || dt.contains("numeric")
        || dt.contains("money")
        || dt.contains("float")
        || dt.contains("real")
        || dt.contains("bit")
}

fn is_guid_type(data_type: &str) -> bool {
    data_type.to_ascii_lowercase().contains("uniqueidentifier")
}

fn cast_text_to_column(text_expr: &str, column: &ColumnInfo) -> String {
    let dt = column.data_type.to_ascii_lowercase();
    if dt.contains("bigint") {
        format!("TRY_CAST({} AS BIGINT)", text_expr)
    } else if dt.contains("smallint") {
        format!("TRY_CAST({} AS SMALLINT)", text_expr)
    } else if dt.contains("tinyint") {
        format!("TRY_CAST({} AS TINYINT)", text_expr)
    } else if dt.contains("int") {
        format!("TRY_CAST({} AS INT)", text_expr)
    } else if dt.contains("decimal")
        || dt.contains("numeric")
        || dt.contains("money")
        || dt.contains("float")
        || dt.contains("real")
    {
        format!("TRY_CAST({} AS DECIMAL(38, 6))", text_expr)
    } else if dt.contains("bit") {
        format!("TRY_CAST({} AS BIT)", text_expr)
    } else if is_guid_type(&dt) {
        format!("TRY_CAST({} AS UNIQUEIDENTIFIER)", text_expr)
    } else {
        text_expr.to_string()
    }
}

fn sql_value_for_column(column: &ColumnInfo, value: &str) -> String {
    if value.trim().is_empty() {
        return "NULL".to_string();
    }

    let dt = column.data_type.to_ascii_lowercase();
    if is_guid_type(&dt) || (!is_numeric_type(&dt) && !dt.contains("date") && !dt.contains("time"))
    {
        sql_string(value.trim())
    } else if dt.contains("date") || dt.contains("time") {
        sql_string(value.trim())
    } else if dt.contains("bit") {
        if matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "oui"
        ) {
            "1".to_string()
        } else {
            "0".to_string()
        }
    } else {
        value.trim().replace(',', ".")
    }
}

fn sql_number(value: f64) -> String {
    format!("{:.2}", round2(value))
}

fn nature_to_label(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "avoir" | "1" => "Avoir".to_string(),
        "proforma" | "2" => "Proforma".to_string(),
        "commande" | "bon_commande" | "3" => "Commande".to_string(),
        "bl" | "bon_livraison" | "4" => "BL".to_string(),
        _ => "Facture".to_string(),
    }
}

fn nature_to_status(value: &str) -> String {
    match nature_to_label(value).as_str() {
        "Avoir" => "Avoir".to_string(),
        "Proforma" => "Proforma".to_string(),
        "Commande" => "Bon_Commande".to_string(),
        _ => "Brouillon".to_string(),
    }
}

fn normalize_status(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "brouillon" => "Brouillon".to_string(),
        "proforma" => "Proforma".to_string(),
        "bon_commande" | "commande" => "Bon_Commande".to_string(),
        "facture" => "Facture".to_string(),
        "avoir" => "Avoir".to_string(),
        "comptabilise" | "comptabilisé" => "Comptabilise".to_string(),
        _ => value.trim().to_string(),
    }
}

fn status_display(value: &str) -> String {
    if value == "Comptabilise" {
        "Comptabilise".to_string()
    } else {
        value.to_string()
    }
}

fn nature_sql_case(edition: &SageEdition, expr: &str) -> String {
    match edition {
        SageEdition::Sage100 | SageEdition::Sage1000 => format!(
            "CASE \
                WHEN TRY_CAST({expr} AS INT) = 1 THEN N'Avoir' \
                WHEN TRY_CAST({expr} AS INT) = 2 THEN N'Proforma' \
                WHEN TRY_CAST({expr} AS INT) = 3 THEN N'Commande' \
                WHEN TRY_CAST({expr} AS INT) = 4 THEN N'BL' \
                WHEN UPPER(CAST({expr} AS NVARCHAR(50))) IN (N'AVOIR', N'AV') THEN N'Avoir' \
                WHEN UPPER(CAST({expr} AS NVARCHAR(50))) LIKE N'%PROFORMA%' THEN N'Proforma' \
                WHEN UPPER(CAST({expr} AS NVARCHAR(50))) LIKE N'%COMMAND%' THEN N'Commande' \
                WHEN UPPER(CAST({expr} AS NVARCHAR(50))) IN (N'BL', N'BON_LIVRAISON') THEN N'BL' \
                ELSE N'Facture' \
            END"
        ),
        _ => "N'Facture'".to_string(),
    }
}

fn nature_to_sql_value(nature: &str, column: &ColumnInfo) -> String {
    let label = nature_to_label(nature);
    if is_numeric_type(&column.data_type) {
        match label.as_str() {
            "Avoir" => "1".to_string(),
            "Proforma" => "2".to_string(),
            "Commande" => "3".to_string(),
            "BL" => "4".to_string(),
            _ => "0".to_string(),
        }
    } else {
        sql_string(&label)
    }
}

fn is_editable_status(value: &str) -> bool {
    matches!(normalize_status(value).as_str(), "Brouillon" | "Proforma")
}

fn validate_transition(current: &str, next: &str) -> bool {
    match normalize_status(current).as_str() {
        "Brouillon" => matches!(
            normalize_status(next).as_str(),
            "Proforma" | "Facture" | "Bon_Commande"
        ),
        "Proforma" => normalize_status(next) == "Facture",
        "Facture" => matches!(normalize_status(next).as_str(), "Avoir" | "Comptabilise"),
        "Comptabilise" => false,
        _ => false,
    }
}

fn default_prefix_for_nature(nature: &str) -> &'static str {
    match nature_to_label(nature).as_str() {
        "Avoir" => "AVO",
        "Proforma" => "PRO",
        "Commande" => "CMD",
        "BL" => "BL",
        _ => "FAC",
    }
}

fn normalize_invoice(mut invoice: InvoiceHeader) -> InvoiceHeader {
    invoice.nature = nature_to_label(&invoice.nature);
    invoice.statut = if invoice.statut.trim().is_empty() {
        nature_to_status(&invoice.nature)
    } else {
        normalize_status(&invoice.statut)
    };

    let discount_factor = 1.0 - (invoice.remise_globale.max(0.0).min(100.0) / 100.0);
    let mut total_ht = 0.0;
    let mut total_tva = 0.0;
    for (index, line) in invoice.lignes.iter_mut().enumerate() {
        if line.id.trim().is_empty() {
            line.id = format!("line-{}", Uuid::new_v4());
        }
        line.ordre = if line.ordre <= 0 {
            (index + 1) as i32
        } else {
            line.ordre
        };
        let base_ht = round2(
            line.quantite * line.prix_ht * (1.0 - line.remise_pct.max(0.0).min(100.0) / 100.0),
        );
        line.montant_ht = base_ht;
        line.montant_tva = round2(base_ht * (line.taux_tva / 100.0));
        line.montant_ttc = round2(line.montant_ht + line.montant_tva);
        total_ht += line.montant_ht;
        total_tva += line.montant_tva;
    }

    invoice.total_ht = round2(total_ht * discount_factor);
    invoice.total_tva = round2(total_tva * discount_factor);
    invoice.total_ttc = round2(invoice.total_ht + invoice.total_tva);
    invoice
}

fn invoice_query_context(
    endpoint: &str,
    query_name: &str,
    connection_id: &str,
    database: &str,
    table: impl Into<String>,
    timeout_secs: u64,
) -> db::QueryExecutionContext {
    db::QueryExecutionContext::new(endpoint, query_name)
        .with_connection_id(connection_id.to_string())
        .with_database(database.to_string())
        .with_table(table.into())
        .with_timeout_secs(timeout_secs)
}

async fn resolve_invoice_schema(
    state: &State<'_, AppState>,
    id: &str,
    client: &mut db::DbClient,
) -> Result<ResolvedInvoiceSchema, String> {
    let connection = load_connection_config(state, id)?;
    let edition = state
        .get_connection_schema(id)
        .map(|schema| schema.edition)
        .filter(|edition| matches!(edition, SageEdition::Sage100 | SageEdition::Sage1000))
        .unwrap_or_else(|| SageEdition::from_edition_str(&connection.sage_edition));

    if !matches!(edition, SageEdition::Sage100 | SageEdition::Sage1000) {
        return Err("Invoicing is supported only for Sage 100 and Sage 1000".to_string());
    }

    let sage_schema = SageSchema::for_edition(&edition);
    let native = InvoiceSchema::for_edition(&edition);
    if !native.is_supported() {
        return Err("Invoice schema is not available for this edition".to_string());
    }

    let tables = db::get_tables(client).await?;
    let piece =
        sage_entity_service::load_table(client, &tables, &[native.table_piece.clone()]).await?;
    let line = sage_entity_service::load_table(
        client,
        &tables,
        &[
            native.table_ligne.clone(),
            "TLIGNEPIECE".to_string(),
            "TLIGNEPIECES".to_string(),
            "TLIGNEPIEC".to_string(),
        ],
    )
    .await?;
    let article =
        sage_entity_service::load_table(client, &tables, &[native.table_article.clone()]).await?;
    let tiers = sage_entity_service::load_table(
        client,
        &tables,
        &[
            native.table_tiers.clone(),
            "F_COMPTET".to_string(),
            "COMPTET".to_string(),
            "F_TIERS".to_string(),
            "TIERS".to_string(),
        ],
    )
    .await?;
    let role_tiers = if native.table_roletiers.trim().is_empty() {
        None
    } else {
        Some(
            sage_entity_service::load_table(client, &tables, &[native.table_roletiers.clone()])
                .await?,
        )
    };
    let journal = if native.table_journal.trim().is_empty() {
        None
    } else {
        sage_entity_service::load_table(
            client,
            &tables,
            &[native.table_journal.clone(), "TPIECE".to_string()],
        )
        .await
        .ok()
    };

    let schema = ResolvedInvoiceSchema {
        edition: edition.clone(),
        sage_schema,
        native: native.clone(),
        piece_oid: sage_entity_service::pick_required(&piece, &["oid", "EC_No"])?,
        piece_numero: sage_entity_service::pick_required(&piece, &["numero", "EC_Piece"])?,
        piece_date: sage_entity_service::pick_required(&piece, &["pDate", "EC_Date"])?,
        piece_reference: sage_entity_service::pick_optional(&piece, &["reference", "EC_RefPiece"])
            .unwrap_or_else(|| {
                sage_entity_service::pick_required(&piece, &["numero", "EC_Piece"])
                    .unwrap_or_default()
            }),
        piece_tiers: sage_entity_service::pick_required(&piece, &["oidTiers", "CT_Num"])?,
        piece_nature: sage_entity_service::pick_required(&piece, &["NaturePiece", "EC_Type"])?,
        piece_devise: sage_entity_service::pick_optional(
            &piece,
            &["oiddevise", "EC_Devise", "devise"],
        ),
        piece_journal_fk: sage_entity_service::pick_optional(&piece, &["oidjournal", "JO_Num"]),
        line_piece: sage_entity_service::pick_required(
            &line,
            &[&native.col_ligne_piece, "oidpiece", "EC_No"],
        )?,
        line_article: sage_entity_service::pick_optional(
            &line,
            &[&native.col_ligne_article, "AR_Ref", "oidarticle"],
        ),
        line_libelle: sage_entity_service::pick_optional(
            &line,
            &[
                &native.col_ligne_libelle,
                "DL_Design",
                "designation",
                "Caption",
            ],
        ),
        line_qte: sage_entity_service::pick_optional(
            &line,
            &[&native.col_ligne_qte, "DL_Qte", "qte"],
        ),
        line_pu_ht: sage_entity_service::pick_optional(
            &line,
            &[&native.col_ligne_pu_ht, "DL_PrixUnitaire", "prix_ht"],
        ),
        line_taux_tva: sage_entity_service::pick_optional(
            &line,
            &[&native.col_ligne_taux_tva, "DL_Taxe1", "tva"],
        ),
        line_montant_ht: sage_entity_service::pick_optional(
            &line,
            &[&native.col_ligne_montant_ht, "DL_MontantHT", "montant_ht"],
        ),
        line_montant_ttc: sage_entity_service::pick_optional(
            &line,
            &[
                &native.col_ligne_montant_ttc,
                "DL_MontantTTC",
                "montant_ttc",
            ],
        ),
        line_remise: sage_entity_service::pick_optional(
            &line,
            &[&native.col_ligne_remise, "DL_Remise01", "remise_pct"],
        ),
        line_ordre: sage_entity_service::pick_optional(
            &line,
            &[&native.col_ligne_ordre, "DL_No", "position"],
        ),
        line_unite: sage_entity_service::pick_optional(&line, &["unite", "DL_Unite", "UV_Code"]),
        article_id: sage_entity_service::pick_optional(&article, &["oid", "AR_Ref", "id"]),
        article_code: sage_entity_service::pick_required(
            &article,
            &[&native.col_article_code, "AR_Ref", "code"],
        )?,
        article_libelle: sage_entity_service::pick_required(
            &article,
            &[
                &native.col_article_libelle,
                "AR_Design",
                "Caption",
                "libelle",
            ],
        )?,
        article_pu: sage_entity_service::pick_optional(
            &article,
            &[&native.col_article_pu, "AR_PrixVen", "prix_ht"],
        ),
        article_tva: sage_entity_service::pick_optional(
            &article,
            &[&native.col_article_tva, "AR_TauxTva", "taux_tva"],
        ),
        article_ref: sage_entity_service::pick_optional(
            &article,
            &[&native.col_article_ref, "AR_Ref", "reference"],
        ),
        article_unite: sage_entity_service::pick_optional(
            &article,
            &["unite", "AR_UniteVen", "UV_Code"],
        ),
        article_active: sage_entity_service::pick_optional(
            &article,
            &["actif", "AR_Sommeil", "en_activite"],
        ),
        tiers_id: sage_entity_service::pick_required(&tiers, &["oid", "CT_Num", "id"])?,
        tiers_code: sage_entity_service::pick_required(&tiers, &["code", "CT_Num", "numero"])?,
        tiers_nom: sage_entity_service::pick_required(
            &tiers,
            &["raisonSociale", "CT_Intitule", "Caption", "nom"],
        )?,
        tiers_adresse: sage_entity_service::pick_optional(
            &tiers,
            &["adresse", "adresse1", "CT_Adresse", "voie"],
        ),
        tiers_cp: sage_entity_service::pick_optional(
            &tiers,
            &["codePostal", "CT_CodePostal", "cp"],
        ),
        tiers_ville: sage_entity_service::pick_optional(&tiers, &["ville", "CT_Ville"]),
        tiers_pays: sage_entity_service::pick_optional(&tiers, &["pays", "CT_Pays"]),
        tiers_siret: sage_entity_service::pick_optional(&tiers, &["siret", "CT_Siret", "siren"]),
        tiers_email: sage_entity_service::pick_optional(&tiers, &["email", "EMail", "CT_EMail"]),
        tiers_telephone: sage_entity_service::pick_optional(&tiers, &["telephone", "CT_Telephone"]),
        tiers_tva: sage_entity_service::pick_optional(
            &tiers,
            &["numTvaIntracom", "CT_Identifiant", "tva"],
        ),
        tiers_type: sage_entity_service::pick_optional(
            &tiers,
            &["CT_Type", "typePersonne", "type"],
        ),
        tiers_account: sage_entity_service::pick_optional(
            &tiers,
            &["CG_NumPrinc", "compteCollectif", "compte"],
        ),
        role_tiers_tiers: role_tiers
            .as_ref()
            .and_then(|table| sage_entity_service::pick_optional(table, &["oidtiers", "oidTiers"])),
        role_tiers_account: role_tiers.as_ref().and_then(|table| {
            sage_entity_service::pick_optional(
                table,
                &[
                    "oidcomptePrivilegie",
                    "oidComptePrivilegie",
                    "oidcompteGeneral",
                ],
            )
        }),
        piece,
        line,
        article,
        tiers,
        role_tiers,
        journal,
    };

    eprintln!(
        "[sage] invoice_schema connection_id={} database={} requested={} resolved={} piece_table={}.{} line_table={}.{} tiers_table={}.{} article_table={}.{}",
        id,
        connection.database,
        connection.sage_edition,
        schema.edition.as_str(),
        schema.piece.schema,
        schema.piece.name,
        schema.line.schema,
        schema.line.name,
        schema.tiers.schema,
        schema.tiers.name,
        schema.article.schema,
        schema.article.name,
    );

    Ok(schema)
}

async fn ensure_invoice_support_tables(client: &mut db::DbClient) -> Result<(), String> {
    let sql = format!(
        "
IF OBJECT_ID(N'dbo.{status}', N'U') IS NULL
BEGIN
    CREATE TABLE [dbo].[{status}] (
        [piece_id] NVARCHAR(50) NOT NULL PRIMARY KEY,
        [statut] NVARCHAR(50) NOT NULL DEFAULT N'Brouillon',
        [updated_at] DATETIME NOT NULL DEFAULT GETDATE(),
        [updated_by] NVARCHAR(100) NULL
    );
END;

IF OBJECT_ID(N'dbo.{history}', N'U') IS NULL
BEGIN
    CREATE TABLE [dbo].[{history}] (
        [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
        [piece_id] NVARCHAR(50) NOT NULL,
        [from_statut] NVARCHAR(50) NULL,
        [to_statut] NVARCHAR(50) NOT NULL,
        [updated_at] DATETIME NOT NULL DEFAULT GETDATE(),
        [updated_by] NVARCHAR(100) NULL
    );
END;

IF OBJECT_ID(N'dbo.{counter}', N'U') IS NULL
BEGIN
    CREATE TABLE [dbo].[{counter}] (
        [counter_key] NVARCHAR(100) NOT NULL PRIMARY KEY,
        [next_value] BIGINT NOT NULL DEFAULT 1,
        [updated_at] DATETIME NOT NULL DEFAULT GETDATE()
    );
END;

IF OBJECT_ID(N'dbo.{meta}', N'U') IS NULL
BEGIN
    CREATE TABLE [dbo].[{meta}] (
        [piece_id] NVARCHAR(50) NOT NULL PRIMARY KEY,
        [date_echeance] DATE NULL,
        [tiers_code] NVARCHAR(100) NULL,
        [tiers_nom] NVARCHAR(255) NULL,
        [tiers_adresse] NVARCHAR(255) NULL,
        [tiers_cp] NVARCHAR(50) NULL,
        [tiers_ville] NVARCHAR(100) NULL,
        [tiers_pays] NVARCHAR(100) NULL,
        [tiers_siret] NVARCHAR(100) NULL,
        [tiers_tva] NVARCHAR(100) NULL,
        [notes] NVARCHAR(MAX) NULL,
        [conditions] NVARCHAR(MAX) NULL,
        [remise_globale] FLOAT NOT NULL DEFAULT 0,
        [is_supplier] BIT NOT NULL DEFAULT 0,
        [updated_at] DATETIME NOT NULL DEFAULT GETDATE()
    );
END;

IF OBJECT_ID(N'dbo.{line_meta}', N'U') IS NULL
BEGIN
    CREATE TABLE [dbo].[{line_meta}] (
        [line_key] NVARCHAR(100) NOT NULL PRIMARY KEY,
        [piece_id] NVARCHAR(50) NOT NULL,
        [ordre] INT NOT NULL,
        [article_code] NVARCHAR(100) NULL,
        [unite] NVARCHAR(50) NULL,
        [updated_at] DATETIME NOT NULL DEFAULT GETDATE()
    );
END;

IF OBJECT_ID(N'dbo.{compta_log}', N'U') IS NULL
BEGIN
    CREATE TABLE [dbo].[{compta_log}] (
        [id] NVARCHAR(50) NOT NULL PRIMARY KEY,
        [piece_id] NVARCHAR(50) NOT NULL,
        [entry_order] INT NOT NULL,
        [entry_role] NVARCHAR(50) NOT NULL,
        [amount] FLOAT NOT NULL,
        [created_at] DATETIME NOT NULL DEFAULT GETDATE()
    );
END;
",
        status = STATUS_TABLE,
        history = STATUS_HISTORY_TABLE,
        counter = COUNTER_TABLE,
        meta = META_TABLE,
        line_meta = LINE_META_TABLE,
        compta_log = COMPTA_LOG_TABLE,
    );
    db::execute_raw_query(client, &sql).await?;

    let sql2 = format!(
        "
IF OBJECT_ID(N'dbo.{tiers}', N'U') IS NULL
BEGIN
    CREATE TABLE [dbo].[{tiers}] (
        [id]         NVARCHAR(50)  NOT NULL PRIMARY KEY,
        [code]       NVARCHAR(100) NOT NULL,
        [nom]        NVARCHAR(255) NULL,
        [adresse]    NVARCHAR(255) NULL,
        [cp]         NVARCHAR(50)  NULL,
        [ville]      NVARCHAR(100) NULL,
        [pays]       NVARCHAR(100) NULL,
        [siret]      NVARCHAR(100) NULL,
        [email]      NVARCHAR(100) NULL,
        [telephone]  NVARCHAR(100) NULL,
        [tva_intra]  NVARCHAR(100) NULL,
        [type_tiers] NVARCHAR(50)  NOT NULL DEFAULT N'client',
        [created_at] NVARCHAR(50)  NULL,
        [updated_at] NVARCHAR(50)  NULL
    );
END;

IF OBJECT_ID(N'dbo.{article}', N'U') IS NULL
BEGIN
    CREATE TABLE [dbo].[{article}] (
        [id]          NVARCHAR(50)   NOT NULL PRIMARY KEY,
        [code]        NVARCHAR(100)  NOT NULL,
        [libelle]     NVARCHAR(255)  NULL,
        [prix_ht]     DECIMAL(18, 6) NULL DEFAULT 0,
        [taux_tva]    DECIMAL(18, 6) NULL DEFAULT 0,
        [unite]       NVARCHAR(50)   NULL,
        [reference]   NVARCHAR(100)  NULL,
        [en_activite] BIT            NOT NULL DEFAULT 1,
        [created_at]  NVARCHAR(50)   NULL,
        [updated_at]  NVARCHAR(50)   NULL
    );
END;
",
        tiers = LOCAL_TIERS_TABLE,
        article = LOCAL_ARTICLE_TABLE,
    );
    db::execute_raw_query(client, &sql2).await?;
    Ok(())
}

fn parse_support_flag(value: Option<&Value>) -> bool {
    parse_bool(value) || parse_i64(value) == 1
}

async fn detect_invoice_support_table_availability(
    client: &mut db::DbClient,
) -> InvoiceSupportTableAvailability {
    let sql = format!(
        "
SELECT
    CASE WHEN OBJECT_ID(N'dbo.{status}', N'U') IS NULL THEN 0 ELSE 1 END AS has_status,
    CASE WHEN OBJECT_ID(N'dbo.{history}', N'U') IS NULL THEN 0 ELSE 1 END AS has_history,
    CASE WHEN OBJECT_ID(N'dbo.{meta}', N'U') IS NULL THEN 0 ELSE 1 END AS has_meta,
    CASE WHEN OBJECT_ID(N'dbo.{line_meta}', N'U') IS NULL THEN 0 ELSE 1 END AS has_line_meta,
    CASE WHEN OBJECT_ID(N'dbo.{tiers}', N'U') IS NULL THEN 0 ELSE 1 END AS has_local_tiers,
    CASE WHEN OBJECT_ID(N'dbo.{article}', N'U') IS NULL THEN 0 ELSE 1 END AS has_local_article
",
        status = STATUS_TABLE,
        history = STATUS_HISTORY_TABLE,
        meta = META_TABLE,
        line_meta = LINE_META_TABLE,
        tiers = LOCAL_TIERS_TABLE,
        article = LOCAL_ARTICLE_TABLE,
    );

    match db::execute_raw_query(client, &sql).await {
        Ok(data) => {
            let availability = data
                .rows
                .first()
                .map(|row| InvoiceSupportTableAvailability {
                    status: parse_support_flag(row.first()),
                    status_history: parse_support_flag(row.get(1)),
                    meta: parse_support_flag(row.get(2)),
                    line_meta: parse_support_flag(row.get(3)),
                    local_tiers: parse_support_flag(row.get(4)),
                    local_article: parse_support_flag(row.get(5)),
                })
                .unwrap_or_default();

            eprintln!(
                "[sage] invoice_support_tables database={} status={} history={} meta={} line_meta={} local_tiers={} local_article={}",
                client.config.database,
                availability.status,
                availability.status_history,
                availability.meta,
                availability.line_meta,
                availability.local_tiers,
                availability.local_article,
            );

            availability
        }
        Err(error) => {
            eprintln!(
                "[sage] invoice_support_tables_probe_error database={} error={}",
                client.config.database, error,
            );
            InvoiceSupportTableAvailability::default()
        }
    }
}

fn status_join_sql(has_status: bool, piece_oid_expr: &str) -> String {
    if has_status {
        format!(
            "LEFT JOIN [dbo].[{table}] s
    ON s.[piece_id] = CAST({piece_oid_expr} AS NVARCHAR(50))",
            table = STATUS_TABLE,
            piece_oid_expr = piece_oid_expr,
        )
    } else {
        "LEFT JOIN (
    SELECT
        CAST(NULL AS NVARCHAR(50)) AS [piece_id],
        CAST(NULL AS NVARCHAR(50)) AS [statut]
    WHERE 1 = 0
) s ON 1 = 0"
            .to_string()
    }
}

fn meta_join_sql(has_meta: bool, piece_oid_expr: &str) -> String {
    if has_meta {
        format!(
            "LEFT JOIN [dbo].[{table}] m
    ON m.[piece_id] = CAST({piece_oid_expr} AS NVARCHAR(50))",
            table = META_TABLE,
            piece_oid_expr = piece_oid_expr,
        )
    } else {
        "LEFT JOIN (
    SELECT
        CAST(NULL AS NVARCHAR(50)) AS [piece_id],
        CAST(NULL AS DATE) AS [date_echeance],
        CAST(NULL AS NVARCHAR(100)) AS [tiers_code],
        CAST(NULL AS NVARCHAR(255)) AS [tiers_nom],
        CAST(NULL AS NVARCHAR(255)) AS [tiers_adresse],
        CAST(NULL AS NVARCHAR(50)) AS [tiers_cp],
        CAST(NULL AS NVARCHAR(100)) AS [tiers_ville],
        CAST(NULL AS NVARCHAR(100)) AS [tiers_pays],
        CAST(NULL AS NVARCHAR(100)) AS [tiers_siret],
        CAST(NULL AS NVARCHAR(100)) AS [tiers_tva],
        CAST(NULL AS NVARCHAR(MAX)) AS [notes],
        CAST(NULL AS NVARCHAR(MAX)) AS [conditions],
        CAST(NULL AS FLOAT) AS [remise_globale],
        CAST(NULL AS BIT) AS [is_supplier]
    WHERE 1 = 0
) m ON 1 = 0"
            .to_string()
    }
}

fn line_meta_join_sql(has_line_meta: bool, piece_id_sql: &str, ordre_expr: &str) -> String {
    if has_line_meta {
        format!(
            "LEFT JOIN [dbo].[{table}] lm
    ON lm.[piece_id] = {piece_id_sql}
   AND lm.[ordre] = COALESCE(TRY_CAST({ordre_expr} AS INT), 0)",
            table = LINE_META_TABLE,
            piece_id_sql = piece_id_sql,
            ordre_expr = ordre_expr,
        )
    } else {
        "LEFT JOIN (
    SELECT
        CAST(NULL AS NVARCHAR(100)) AS [line_key],
        CAST(NULL AS NVARCHAR(50)) AS [piece_id],
        CAST(NULL AS INT) AS [ordre],
        CAST(NULL AS NVARCHAR(100)) AS [article_code],
        CAST(NULL AS NVARCHAR(50)) AS [unite]
    WHERE 1 = 0
) lm ON 1 = 0"
            .to_string()
    }
}

async fn is_identity_column(
    client: &mut db::DbClient,
    table: &ResolvedTable,
    column: &str,
) -> Result<bool, String> {
    let sql = format!(
        "SELECT COLUMNPROPERTY(OBJECT_ID(N'{}'), '{}', 'IsIdentity') AS is_identity",
        table.quoted_name().replace('\'', "''"),
        column.replace('\'', "''")
    );
    let data = db::execute_raw_query(client, &sql).await?;
    Ok(parse_i64(data.rows.first().and_then(|row| row.first())) == 1)
}

async fn get_next_counter_value(client: &mut db::DbClient, key: &str) -> Result<i64, String> {
    let sql = format!(
        "
IF EXISTS (SELECT 1 FROM [dbo].[{table}] WHERE [counter_key] = {key})
BEGIN
    UPDATE [dbo].[{table}]
    SET [next_value] = [next_value] + 1,
        [updated_at] = GETDATE()
    OUTPUT INSERTED.[next_value] - 1 AS current_value
    WHERE [counter_key] = {key};
END
ELSE
BEGIN
    INSERT INTO [dbo].[{table}] ([counter_key], [next_value], [updated_at])
    VALUES ({key}, 2, GETDATE());
    SELECT CAST(1 AS BIGINT) AS current_value;
END
",
        table = COUNTER_TABLE,
        key = sql_string(key),
    );
    let data = db::execute_raw_query(client, &sql).await?;
    Ok(parse_i64(data.rows.first().and_then(|row| row.first())))
}

async fn get_next_piece_id(
    client: &mut db::DbClient,
    table: &ResolvedTable,
    column: &str,
) -> Result<String, String> {
    let column_info = table
        .column(column)
        .ok_or_else(|| format!("Column {} not found", column))?;
    if is_guid_type(&column_info.data_type) {
        return Ok(Uuid::new_v4().to_string());
    }
    if !is_numeric_type(&column_info.data_type) {
        return Ok(Uuid::new_v4().to_string());
    }

    let sql = format!(
        "SELECT COALESCE(MAX(TRY_CAST({} AS BIGINT)), 0) + 1 FROM {}",
        br(column),
        table.quoted_name()
    );
    let data = db::execute_raw_query(client, &sql).await?;
    Ok(parse_i64(data.rows.first().and_then(|row| row.first())).to_string())
}

async fn generate_invoice_numero(
    client: &mut db::DbClient,
    resolved: &ResolvedInvoiceSchema,
    nature: &str,
) -> Result<String, String> {
    let sql = format!(
        "SELECT TOP 1 CAST({numero} AS NVARCHAR(100)) AS numero \
         FROM {table} \
         WHERE {numero} IS NOT NULL \
         ORDER BY TRY_CAST(RIGHT(CAST({numero} AS NVARCHAR(100)), 6) AS INT) DESC, {date_col} DESC",
        numero = br(&resolved.piece_numero),
        table = resolved.piece.quoted_name(),
        date_col = br(&resolved.piece_date),
    );
    let data = db::execute_raw_query(client, &sql).await?;
    if let Some(last_numero) = data
        .rows
        .first()
        .map(|row| parse_string(row.first()))
        .filter(|value| !value.is_empty())
    {
        let digits = last_numero
            .chars()
            .rev()
            .take_while(|char| char.is_ascii_digit())
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        if digits.len() >= 3 {
            let current = digits.parse::<i64>().unwrap_or(0) + 1;
            let prefix = &last_numero[..last_numero.len().saturating_sub(digits.len())];
            return Ok(format!(
                "{}{:0width$}",
                prefix,
                current,
                width = digits.len()
            ));
        }
    }

    let counter =
        get_next_counter_value(client, &format!("invoice:{}", nature_to_label(nature))).await?;
    Ok(format!(
        "{}-{}-{:04}",
        default_prefix_for_nature(nature),
        Utc::now().format("%Y%m"),
        counter
    ))
}

fn get_current_status_sql(piece_id: &str) -> String {
    format!(
        "SELECT TOP 1 [statut], CONVERT(VARCHAR(19), [updated_at], 126), COALESCE([updated_by], N'') \
         FROM [dbo].[{table}] WHERE [piece_id] = {piece_id}",
        table = STATUS_TABLE,
        piece_id = sql_string(piece_id),
    )
}

async fn get_current_status(
    client: &mut db::DbClient,
    piece_id: &str,
) -> Result<Option<StatutUpdateResult>, String> {
    let data = db::execute_raw_query(client, &get_current_status_sql(piece_id)).await?;
    Ok(data.rows.first().map(|row| StatutUpdateResult {
        piece_id: piece_id.to_string(),
        statut: parse_string(row.first()),
        updated_at: parse_string(row.get(1)),
        updated_by: parse_string(row.get(2)),
    }))
}

async fn upsert_status_batch(
    client: &mut db::DbClient,
    piece_id: &str,
    status: &str,
    updated_by: &str,
    previous_status: Option<&str>,
) -> Result<(), String> {
    let history_id = Uuid::new_v4().to_string();
    let sql = format!(
        "
IF EXISTS (SELECT 1 FROM [dbo].[{status_table}] WHERE [piece_id] = {piece_id})
BEGIN
    UPDATE [dbo].[{status_table}]
    SET [statut] = {status},
        [updated_at] = GETDATE(),
        [updated_by] = {updated_by}
    WHERE [piece_id] = {piece_id};
END
ELSE
BEGIN
    INSERT INTO [dbo].[{status_table}] ([piece_id], [statut], [updated_at], [updated_by])
    VALUES ({piece_id}, {status}, GETDATE(), {updated_by});
END;

INSERT INTO [dbo].[{history_table}] ([id], [piece_id], [from_statut], [to_statut], [updated_at], [updated_by])
VALUES ({history_id}, {piece_id}, {from_statut}, {status}, GETDATE(), {updated_by});
",
        status_table = STATUS_TABLE,
        history_table = STATUS_HISTORY_TABLE,
        piece_id = sql_string(piece_id),
        status = sql_string(status),
        updated_by = sql_opt_string(updated_by),
        history_id = sql_string(&history_id),
        from_statut = previous_status.map(sql_string).unwrap_or_else(|| "NULL".to_string()),
    );
    db::execute_raw_query(client, &sql).await?;
    Ok(())
}

async fn fetch_status_history(
    client: &mut db::DbClient,
    availability: InvoiceSupportTableAvailability,
    piece_id: &str,
) -> Result<Vec<StatusHistoryEntry>, String> {
    if !availability.status_history {
        return Ok(Vec::new());
    }

    let sql = format!(
        "SELECT [id], COALESCE([from_statut], N''), [to_statut], CONVERT(VARCHAR(19), [updated_at], 126), COALESCE([updated_by], N'') \
         FROM [dbo].[{table}] \
         WHERE [piece_id] = {piece_id} \
         ORDER BY [updated_at] ASC",
        table = STATUS_HISTORY_TABLE,
        piece_id = sql_string(piece_id),
    );
    let data = db::execute_raw_query(client, &sql).await?;
    Ok(data
        .rows
        .iter()
        .map(|row| StatusHistoryEntry {
            id: parse_string(row.first()),
            from_statut: parse_string(row.get(1)),
            to_statut: status_display(&parse_string(row.get(2))),
            updated_at: parse_string(row.get(3)),
            updated_by: parse_string(row.get(4)),
        })
        .collect())
}

fn invoice_list_select(
    resolved: &ResolvedInvoiceSchema,
    availability: InvoiceSupportTableAvailability,
) -> String {
    let nature_expr = nature_sql_case(&resolved.edition, &qualify("p", &resolved.piece_nature));
    let piece_oid_expr = qualify("p", &resolved.piece_oid);
    let status_join = status_join_sql(availability.status, &piece_oid_expr);
    let meta_join = meta_join_sql(availability.meta, &piece_oid_expr);
    let devise_expr = resolved
        .piece_devise
        .as_ref()
        .map(|column| {
            format!(
                "COALESCE(CAST({} AS NVARCHAR(50)), N'XOF')",
                qualify("p", column)
            )
        })
        .unwrap_or_else(|| "N'XOF'".to_string());
    let tiers_nom_expr = resolved
        .tiers_adresse
        .as_ref()
        .map(|_| {
            "COALESCE(m.[tiers_nom], CAST(tnom AS NVARCHAR(255)), N'')"
                .replace("tnom", &qualify("t", &resolved.tiers_nom))
        })
        .unwrap_or_else(|| {
            format!(
                "COALESCE(m.[tiers_nom], CAST({} AS NVARCHAR(255)), N'')",
                qualify("t", &resolved.tiers_nom)
            )
        });
    let tiers_addr_expr = resolved
        .tiers_adresse
        .as_ref()
        .map(|column| {
            format!(
                "COALESCE(m.[tiers_adresse], CAST({} AS NVARCHAR(255)), N'')",
                qualify("t", column)
            )
        })
        .unwrap_or_else(|| "COALESCE(m.[tiers_adresse], N'')".to_string());
    let tiers_cp_expr = resolved
        .tiers_cp
        .as_ref()
        .map(|column| {
            format!(
                "COALESCE(m.[tiers_cp], CAST({} AS NVARCHAR(50)), N'')",
                qualify("t", column)
            )
        })
        .unwrap_or_else(|| "COALESCE(m.[tiers_cp], N'')".to_string());
    let tiers_ville_expr = resolved
        .tiers_ville
        .as_ref()
        .map(|column| {
            format!(
                "COALESCE(m.[tiers_ville], CAST({} AS NVARCHAR(100)), N'')",
                qualify("t", column)
            )
        })
        .unwrap_or_else(|| "COALESCE(m.[tiers_ville], N'')".to_string());
    let tiers_siret_expr = resolved
        .tiers_siret
        .as_ref()
        .map(|column| {
            format!(
                "COALESCE(m.[tiers_siret], CAST({} AS NVARCHAR(100)), N'')",
                qualify("t", column)
            )
        })
        .unwrap_or_else(|| "COALESCE(m.[tiers_siret], N'')".to_string());

    format!(
        "
SELECT
    CAST({piece_oid} AS NVARCHAR(50)) AS id,
    CAST({piece_numero} AS NVARCHAR(100)) AS numero,
    CONVERT(VARCHAR(10), {piece_date}, 23) AS [date],
    COALESCE(CONVERT(VARCHAR(10), m.[date_echeance], 23), N'') AS date_echeance,
    CAST({piece_tiers} AS NVARCHAR(50)) AS tiers_id,
    COALESCE(m.[tiers_code], CAST({tiers_code} AS NVARCHAR(100)), N'') AS tiers_code,
    {tiers_nom_expr} AS tiers_nom,
    {tiers_addr_expr} AS tiers_adresse,
    {tiers_cp_expr} AS tiers_cp,
    {tiers_ville_expr} AS tiers_ville,
    {tiers_siret_expr} AS tiers_siret,
    COALESCE({nature_expr}, N'Facture') AS nature,
    COALESCE(s.[statut], N'Brouillon') AS statut,
    COALESCE(CAST({piece_reference} AS NVARCHAR(255)), N'') AS reference,
    {devise_expr} AS devise,
    COALESCE(tot.[total_ht], 0) AS total_ht,
    COALESCE(tot.[total_tva], 0) AS total_tva,
    COALESCE(tot.[total_ttc], 0) AS total_ttc,
    COALESCE(m.[notes], N'') AS notes,
    COALESCE(m.[conditions], N'') AS conditions,
    COALESCE(m.[tiers_pays], N'') AS tiers_pays,
    COALESCE(m.[tiers_tva], N'') AS tiers_tva_intra,
    COALESCE(m.[remise_globale], 0) AS remise_globale,
    COALESCE(m.[is_supplier], 0) AS is_supplier
FROM {piece_table} p
LEFT JOIN {tiers_table} t
    ON {piece_tiers} = {tiers_id}
{status_join}
{meta_join}
OUTER APPLY (
    SELECT
        ROUND(SUM(COALESCE(TRY_CAST({line_ht} AS DECIMAL(18, 6)), 0)), 2) AS total_ht,
        ROUND(
            SUM(COALESCE(TRY_CAST({line_ttc} AS DECIMAL(18, 6)), 0))
            - SUM(COALESCE(TRY_CAST({line_ht} AS DECIMAL(18, 6)), 0)),
            2
        ) AS total_tva,
        ROUND(SUM(COALESCE(TRY_CAST({line_ttc} AS DECIMAL(18, 6)), 0)), 2) AS total_ttc
    FROM {line_table} l
    WHERE {line_piece} = {piece_oid}
) tot
",
        piece_oid = piece_oid_expr,
        piece_numero = qualify("p", &resolved.piece_numero),
        piece_date = qualify("p", &resolved.piece_date),
        piece_tiers = qualify("p", &resolved.piece_tiers),
        tiers_code = qualify("t", &resolved.tiers_code),
        tiers_nom_expr = tiers_nom_expr,
        tiers_addr_expr = tiers_addr_expr,
        tiers_cp_expr = tiers_cp_expr,
        tiers_ville_expr = tiers_ville_expr,
        tiers_siret_expr = tiers_siret_expr,
        nature_expr = nature_expr,
        piece_reference = qualify("p", &resolved.piece_reference),
        devise_expr = devise_expr,
        piece_table = resolved.piece.quoted_name(),
        tiers_table = resolved.tiers.quoted_name(),
        tiers_id = qualify("t", &resolved.tiers_id),
        status_join = status_join,
        meta_join = meta_join,
        line_ht = resolved
            .line_montant_ht
            .as_ref()
            .map(|column| qualify("l", column))
            .unwrap_or_else(|| "0".to_string()),
        line_ttc = resolved
            .line_montant_ttc
            .as_ref()
            .map(|column| qualify("l", column))
            .unwrap_or_else(|| "0".to_string()),
        line_table = resolved.line.quoted_name(),
        line_piece = qualify("l", &resolved.line_piece),
    )
}

fn map_invoice_row(row: &[Value]) -> InvoiceHeader {
    InvoiceHeader {
        id: parse_string(row.first()),
        numero: parse_string(row.get(1)),
        date: parse_string(row.get(2)),
        date_echeance: parse_string(row.get(3)),
        tiers_id: parse_string(row.get(4)),
        tiers_code: parse_string(row.get(5)),
        tiers_nom: parse_string(row.get(6)),
        tiers_adresse: parse_string(row.get(7)),
        tiers_cp: parse_string(row.get(8)),
        tiers_ville: parse_string(row.get(9)),
        tiers_siret: parse_string(row.get(10)),
        nature: nature_to_label(&parse_string(row.get(11))),
        statut: status_display(&parse_string(row.get(12))),
        reference: parse_string(row.get(13)),
        devise: parse_string(row.get(14)),
        total_ht: round2(parse_f64(row.get(15))),
        total_tva: round2(parse_f64(row.get(16))),
        total_ttc: round2(parse_f64(row.get(17))),
        lignes: Vec::new(),
        notes: parse_string(row.get(18)),
        conditions: parse_string(row.get(19)),
        tiers_pays: parse_string(row.get(20)),
        tiers_tva_intra: parse_string(row.get(21)),
        remise_globale: round2(parse_f64(row.get(22))),
        is_supplier: parse_bool(row.get(23)),
        statut_history: Vec::new(),
    }
}

async fn fetch_invoice_internal(
    state: &State<'_, AppState>,
    id: &str,
    piece_id: &str,
) -> Result<InvoiceHeader, String> {
    let _connection = load_connection_config(state, id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    let support_tables = detect_invoice_support_table_availability(&mut client).await;
    let resolved = resolve_invoice_schema(state, id, &mut client).await?;
    let database_name = client.config.database.clone();

    let mut header_sql = invoice_list_select(&resolved, support_tables);
    header_sql.push_str(&format!(
        " WHERE CAST({} AS NVARCHAR(50)) = {}",
        qualify("p", &resolved.piece_oid),
        sql_string(piece_id)
    ));
    let header_data = db::execute_logged_query(
        &mut client,
        &header_sql,
        invoice_query_context(
            "get_invoice",
            "invoice_get_header",
            id,
            &database_name,
            resolved.piece.quoted_name(),
            db::DEFAULT_QUERY_TIMEOUT_SECS,
        ),
    )
    .await?;
    let header_row = header_data
        .rows
        .first()
        .cloned()
        .ok_or_else(|| format!("Invoice {} not found", piece_id))?;
    let mut invoice = map_invoice_row(&header_row);

    let article_code_expr = resolved
        .article_id
        .as_ref()
        .zip(resolved.line_article.as_ref())
        .map(|(_article_id, _line_article)| {
            format!(
                "COALESCE(CAST({} AS NVARCHAR(100)), lm.[article_code], N'')",
                qualify("a", &resolved.article_code)
            )
        })
        .unwrap_or_else(|| "COALESCE(lm.[article_code], N'')".to_string());

    let article_join = if let Some(line_article) = resolved.line_article.as_ref() {
        if let Some(article_id) = resolved.article_id.as_ref() {
            format!(
                "LEFT JOIN {} a ON {} = {}",
                resolved.article.quoted_name(),
                qualify("l", line_article),
                qualify("a", article_id)
            )
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let ordre_expr = resolved
        .line_ordre
        .as_ref()
        .map(|column| qualify("l", column))
        .unwrap_or_else(|| "ROW_NUMBER() OVER (ORDER BY (SELECT 1))".to_string());
    let piece_id_sql = sql_string(piece_id);
    let line_meta_join = line_meta_join_sql(support_tables.line_meta, &piece_id_sql, &ordre_expr);

    let line_sql = format!(
        "
SELECT
    COALESCE(lm.[line_key], CONCAT({piece_id_sql}, N':', ROW_NUMBER() OVER (ORDER BY {ordre_expr}, (SELECT 1)))) AS id,
    COALESCE(TRY_CAST({ordre_expr} AS INT), ROW_NUMBER() OVER (ORDER BY {ordre_expr}, (SELECT 1))) AS ordre,
    COALESCE(CAST({article_id_expr} AS NVARCHAR(50)), N'') AS article_id,
    {article_code_expr} AS article_code,
    COALESCE(CAST({libelle_expr} AS NVARCHAR(255)), N'') AS libelle,
    COALESCE(TRY_CAST({qte_expr} AS DECIMAL(18, 6)), 0) AS quantite,
    COALESCE(lm.[unite], CAST({unite_expr} AS NVARCHAR(50)), N'') AS unite,
    COALESCE(TRY_CAST({pu_expr} AS DECIMAL(18, 6)), 0) AS prix_ht,
    COALESCE(TRY_CAST({remise_expr} AS DECIMAL(18, 6)), 0) AS remise_pct,
    COALESCE(TRY_CAST({ht_expr} AS DECIMAL(18, 6)), 0) AS montant_ht,
    COALESCE(TRY_CAST({tva_expr} AS DECIMAL(18, 6)), 0) AS taux_tva,
    ROUND(
        COALESCE(TRY_CAST({ht_expr} AS DECIMAL(18, 6)), 0)
        * COALESCE(TRY_CAST({tva_expr} AS DECIMAL(18, 6)), 0) / 100.0,
        2
    ) AS montant_tva,
    COALESCE(TRY_CAST({ttc_expr} AS DECIMAL(18, 6)), 0) AS montant_ttc
FROM {line_table} l
{article_join}
{line_meta_join}
WHERE {line_piece_expr} = {piece_fk_sql}
ORDER BY ordre ASC
",
        piece_id_sql = piece_id_sql,
        ordre_expr = ordre_expr,
        article_id_expr = resolved
            .line_article
            .as_ref()
            .map(|column| qualify("l", column))
            .unwrap_or_else(|| "NULL".to_string()),
        article_code_expr = article_code_expr,
        libelle_expr = resolved
            .line_libelle
            .as_ref()
            .map(|column| qualify("l", column))
            .unwrap_or_else(|| "N''".to_string()),
        qte_expr = resolved
            .line_qte
            .as_ref()
            .map(|column| qualify("l", column))
            .unwrap_or_else(|| "0".to_string()),
        unite_expr = resolved
            .line_unite
            .as_ref()
            .map(|column| qualify("l", column))
            .or_else(|| resolved.article_unite.as_ref().map(|column| qualify("a", column)))
            .unwrap_or_else(|| "N''".to_string()),
        pu_expr = resolved
            .line_pu_ht
            .as_ref()
            .map(|column| qualify("l", column))
            .unwrap_or_else(|| "0".to_string()),
        remise_expr = resolved
            .line_remise
            .as_ref()
            .map(|column| qualify("l", column))
            .unwrap_or_else(|| "0".to_string()),
        ht_expr = resolved
            .line_montant_ht
            .as_ref()
            .map(|column| qualify("l", column))
            .unwrap_or_else(|| format!(
                "COALESCE(TRY_CAST({} AS DECIMAL(18, 6)), 0) * COALESCE(TRY_CAST({} AS DECIMAL(18, 6)), 0)",
                resolved
                    .line_qte
                    .as_ref()
                    .map(|column| qualify("l", column))
                    .unwrap_or_else(|| "0".to_string()),
                resolved
                    .line_pu_ht
                    .as_ref()
                    .map(|column| qualify("l", column))
                    .unwrap_or_else(|| "0".to_string())
            )),
        tva_expr = resolved
            .line_taux_tva
            .as_ref()
            .map(|column| qualify("l", column))
            .unwrap_or_else(|| "0".to_string()),
        ttc_expr = resolved
            .line_montant_ttc
            .as_ref()
            .map(|column| qualify("l", column))
            .unwrap_or_else(|| format!(
                "COALESCE(TRY_CAST({ht_expr} AS DECIMAL(18, 6)), 0) + (COALESCE(TRY_CAST({ht_expr} AS DECIMAL(18, 6)), 0) * COALESCE(TRY_CAST({tva_expr} AS DECIMAL(18, 6)), 0) / 100.0)",
                ht_expr = resolved
                    .line_montant_ht
                    .as_ref()
                    .map(|column| qualify("l", column))
                    .unwrap_or_else(|| "0".to_string()),
                tva_expr = resolved
                    .line_taux_tva
                    .as_ref()
                    .map(|column| qualify("l", column))
                    .unwrap_or_else(|| "0".to_string())
            )),
        line_table = resolved.line.quoted_name(),
        article_join = article_join,
        line_meta_join = line_meta_join,
        line_piece_expr = qualify("l", &resolved.line_piece),
        piece_fk_sql = {
            let column = resolved
                .line
                .column(&resolved.line_piece)
                .ok_or_else(|| "Line piece FK column not found".to_string())?;
            cast_text_to_column(&sql_string(piece_id), column)
        },
    );

    let line_data = db::execute_logged_query(
        &mut client,
        &line_sql,
        invoice_query_context(
            "get_invoice",
            "invoice_get_lines",
            id,
            &database_name,
            resolved.line.quoted_name(),
            db::DEFAULT_QUERY_TIMEOUT_SECS,
        ),
    )
    .await?;
    invoice.lignes = line_data
        .rows
        .iter()
        .map(|row| InvoiceLine {
            id: parse_string(row.first()),
            ordre: parse_i32(row.get(1)),
            article_id: parse_string(row.get(2)),
            article_code: parse_string(row.get(3)),
            libelle: parse_string(row.get(4)),
            quantite: round2(parse_f64(row.get(5))),
            unite: parse_string(row.get(6)),
            prix_ht: round2(parse_f64(row.get(7))),
            remise_pct: round2(parse_f64(row.get(8))),
            montant_ht: round2(parse_f64(row.get(9))),
            taux_tva: round2(parse_f64(row.get(10))),
            montant_tva: round2(parse_f64(row.get(11))),
            montant_ttc: round2(parse_f64(row.get(12))),
        })
        .collect();
    invoice.statut_history = fetch_status_history(&mut client, support_tables, piece_id).await?;
    Ok(invoice)
}

#[tauri::command]
pub async fn list_invoices(
    state: State<'_, AppState>,
    id: String,
    date_from: Option<String>,
    date_to: Option<String>,
    nature: Option<String>,
    statut: Option<String>,
    tiers_id: Option<String>,
    search: Option<String>,
) -> Result<Vec<InvoiceHeader>, String> {
    let _connection = load_connection_config(&state, &id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    let support_tables = detect_invoice_support_table_availability(&mut client).await;
    let resolved = resolve_invoice_schema(&state, &id, &mut client).await?;
    let database_name = client.config.database.clone();

    let mut sql = invoice_list_select(&resolved, support_tables);
    sql.push_str(" WHERE 1=1");

    if let Some(value) = date_from.as_ref().filter(|value| !value.trim().is_empty()) {
        sql.push_str(&format!(
            " AND CONVERT(DATE, {}) >= CONVERT(DATE, {})",
            qualify("p", &resolved.piece_date),
            sql_string(value)
        ));
    }
    if let Some(value) = date_to.as_ref().filter(|value| !value.trim().is_empty()) {
        sql.push_str(&format!(
            " AND CONVERT(DATE, {}) <= CONVERT(DATE, {})",
            qualify("p", &resolved.piece_date),
            sql_string(value)
        ));
    }
    if let Some(value) = nature.as_ref().filter(|value| !value.trim().is_empty()) {
        sql.push_str(&format!(
            " AND {} = {}",
            nature_sql_case(&resolved.edition, &qualify("p", &resolved.piece_nature)),
            sql_string(&nature_to_label(value))
        ));
    }
    if let Some(value) = statut.as_ref().filter(|value| !value.trim().is_empty()) {
        sql.push_str(&format!(
            " AND COALESCE(s.[statut], N'Brouillon') = {}",
            sql_string(&normalize_status(value))
        ));
    }
    if let Some(value) = tiers_id.as_ref().filter(|value| !value.trim().is_empty()) {
        sql.push_str(&format!(
            " AND CAST({} AS NVARCHAR(50)) = {}",
            qualify("p", &resolved.piece_tiers),
            sql_string(value)
        ));
    }
    if let Some(value) = search.as_ref().filter(|value| !value.trim().is_empty()) {
        let search_value = format!("%{}%", value.trim());
        sql.push_str(&format!(
            " AND (CAST({numero} AS NVARCHAR(100)) LIKE {search} OR CAST({reference} AS NVARCHAR(255)) LIKE {search} OR CAST({tiers_nom} AS NVARCHAR(255)) LIKE {search})",
            numero = qualify("p", &resolved.piece_numero),
            reference = qualify("p", &resolved.piece_reference),
            tiers_nom = qualify("t", &resolved.tiers_nom),
            search = sql_string(&search_value),
        ));
    }
    sql.push_str(&format!(
        " ORDER BY {} DESC, {} DESC",
        qualify("p", &resolved.piece_date),
        qualify("p", &resolved.piece_numero)
    ));

    let data = db::execute_logged_query(
        &mut client,
        &sql,
        invoice_query_context(
            "list_invoices",
            "invoice_list",
            &id,
            &database_name,
            resolved.piece.quoted_name(),
            db::DEFAULT_QUERY_TIMEOUT_SECS,
        ),
    )
    .await?;
    Ok(data.rows.iter().map(|row| map_invoice_row(row)).collect())
}

#[tauri::command]
pub async fn get_invoice(
    state: State<'_, AppState>,
    id: String,
    piece_id: String,
) -> Result<InvoiceHeader, String> {
    fetch_invoice_internal(&state, &id, &piece_id).await
}

#[tauri::command]
pub async fn create_invoice(
    state: State<'_, AppState>,
    id: String,
    invoice: InvoiceHeader,
) -> Result<InvoiceHeader, String> {
    let _connection = load_connection_config(&state, &id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    ensure_invoice_support_tables(&mut client).await?;
    let resolved = resolve_invoice_schema(&state, &id, &mut client).await?;
    let database_name = client.config.database.clone();

    let mut invoice = normalize_invoice(invoice);
    if invoice.numero.trim().is_empty() {
        invoice.numero = generate_invoice_numero(&mut client, &resolved, &invoice.nature).await?;
    }

    let piece_id_column = resolved
        .piece
        .column(&resolved.piece_oid)
        .ok_or_else(|| "Piece id column metadata missing".to_string())?;
    let piece_id_is_identity =
        is_identity_column(&mut client, &resolved.piece, &resolved.piece_oid).await?;
    if invoice.id.trim().is_empty() && !piece_id_is_identity {
        invoice.id = get_next_piece_id(&mut client, &resolved.piece, &resolved.piece_oid).await?;
    }

    let status = if invoice.statut.trim().is_empty() {
        nature_to_status(&invoice.nature)
    } else {
        normalize_status(&invoice.statut)
    };
    let mut piece_columns = Vec::new();
    let mut piece_values = Vec::new();

    if !piece_id_is_identity && !invoice.id.trim().is_empty() {
        piece_columns.push(br(&resolved.piece_oid));
        piece_values.push(sql_value_for_column(piece_id_column, &invoice.id));
    }

    let add_piece_value = |columns: &mut Vec<String>,
                           values: &mut Vec<String>,
                           column_name: &str,
                           table: &ResolvedTable,
                           value: String| {
        if table.column(column_name).is_some() {
            columns.push(br(column_name));
            values.push(value);
        }
    };

    add_piece_value(
        &mut piece_columns,
        &mut piece_values,
        &resolved.piece_numero,
        &resolved.piece,
        sql_value_for_column(
            resolved.piece.column(&resolved.piece_numero).unwrap(),
            &invoice.numero,
        ),
    );
    add_piece_value(
        &mut piece_columns,
        &mut piece_values,
        &resolved.piece_date,
        &resolved.piece,
        sql_value_for_column(
            resolved.piece.column(&resolved.piece_date).unwrap(),
            &invoice.date,
        ),
    );
    add_piece_value(
        &mut piece_columns,
        &mut piece_values,
        &resolved.piece_reference,
        &resolved.piece,
        sql_value_for_column(
            resolved.piece.column(&resolved.piece_reference).unwrap(),
            &invoice.reference,
        ),
    );
    add_piece_value(
        &mut piece_columns,
        &mut piece_values,
        &resolved.piece_tiers,
        &resolved.piece,
        sql_value_for_column(
            resolved.piece.column(&resolved.piece_tiers).unwrap(),
            &invoice.tiers_id,
        ),
    );
    add_piece_value(
        &mut piece_columns,
        &mut piece_values,
        &resolved.piece_nature,
        &resolved.piece,
        nature_to_sql_value(
            &invoice.nature,
            resolved.piece.column(&resolved.piece_nature).unwrap(),
        ),
    );
    if let Some(devise_column) = resolved.piece_devise.as_ref() {
        add_piece_value(
            &mut piece_columns,
            &mut piece_values,
            devise_column,
            &resolved.piece,
            sql_value_for_column(
                resolved.piece.column(devise_column).unwrap(),
                if invoice.devise.trim().is_empty() {
                    "XOF"
                } else {
                    &invoice.devise
                },
            ),
        );
    }

    let piece_id_text = if invoice.id.trim().is_empty() {
        String::new()
    } else {
        invoice.id.clone()
    };

    let mut line_sql_chunks = Vec::new();
    let line_piece_column = resolved
        .line
        .column(&resolved.line_piece)
        .ok_or_else(|| "Line piece column metadata missing".to_string())?;

    for line in &invoice.lignes {
        let mut columns = vec![br(&resolved.line_piece)];
        let mut values = vec![cast_text_to_column("@piece_id_text", line_piece_column)];

        if let Some(column) = resolved
            .line_article
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_value_for_column(column, &line.article_id));
        }
        if let Some(column) = resolved
            .line_libelle
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_value_for_column(column, &line.libelle));
        }
        if let Some(column) = resolved
            .line_qte
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_number(line.quantite));
        }
        if let Some(column) = resolved
            .line_pu_ht
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_number(line.prix_ht));
        }
        if let Some(column) = resolved
            .line_taux_tva
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_number(line.taux_tva));
        }
        if let Some(column) = resolved
            .line_montant_ht
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_number(line.montant_ht));
        }
        if let Some(column) = resolved
            .line_montant_ttc
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_number(line.montant_ttc));
        }
        if let Some(column) = resolved
            .line_remise
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_number(line.remise_pct));
        }
        if let Some(column) = resolved
            .line_ordre
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(line.ordre.to_string());
        }
        if let Some(column) = resolved
            .line_unite
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_value_for_column(column, &line.unite));
        }

        line_sql_chunks.push(format!(
            "INSERT INTO {} ({}) VALUES ({});",
            resolved.line.quoted_name(),
            columns.join(", "),
            values.join(", ")
        ));

        line_sql_chunks.push(format!(
            "MERGE [dbo].[{line_meta}] AS target
             USING (SELECT CONCAT(@piece_id_text, N':', {ordre}) AS [line_key]) AS src
             ON target.[line_key] = src.[line_key]
             WHEN MATCHED THEN
                 UPDATE SET [piece_id] = @piece_id_text, [ordre] = {ordre}, [article_code] = {article_code}, [unite] = {unite}, [updated_at] = GETDATE()
             WHEN NOT MATCHED THEN
                 INSERT ([line_key], [piece_id], [ordre], [article_code], [unite], [updated_at])
                 VALUES (src.[line_key], @piece_id_text, {ordre}, {article_code}, {unite}, GETDATE());",
            line_meta = LINE_META_TABLE,
            ordre = line.ordre,
            article_code = sql_opt_string(&line.article_code),
            unite = sql_opt_string(&line.unite),
        ));
    }

    let sql = format!(
        "
BEGIN TRY
    BEGIN TRANSACTION;
    DECLARE @piece_id_text NVARCHAR(50) = {piece_id_value};

    INSERT INTO {piece_table} ({piece_columns})
    VALUES ({piece_values});

    {set_identity_piece_id}

    {line_sql}

    MERGE [dbo].[{meta_table}] AS target
    USING (SELECT @piece_id_text AS [piece_id]) AS src
    ON target.[piece_id] = src.[piece_id]
    WHEN MATCHED THEN
        UPDATE SET
            [date_echeance] = {date_echeance},
            [tiers_code] = {tiers_code},
            [tiers_nom] = {tiers_nom},
            [tiers_adresse] = {tiers_adresse},
            [tiers_cp] = {tiers_cp},
            [tiers_ville] = {tiers_ville},
            [tiers_pays] = {tiers_pays},
            [tiers_siret] = {tiers_siret},
            [tiers_tva] = {tiers_tva},
            [notes] = {notes},
            [conditions] = {conditions},
            [remise_globale] = {remise_globale},
            [is_supplier] = {is_supplier},
            [updated_at] = GETDATE()
    WHEN NOT MATCHED THEN
        INSERT ([piece_id], [date_echeance], [tiers_code], [tiers_nom], [tiers_adresse], [tiers_cp], [tiers_ville], [tiers_pays], [tiers_siret], [tiers_tva], [notes], [conditions], [remise_globale], [is_supplier], [updated_at])
        VALUES (@piece_id_text, {date_echeance}, {tiers_code}, {tiers_nom}, {tiers_adresse}, {tiers_cp}, {tiers_ville}, {tiers_pays}, {tiers_siret}, {tiers_tva}, {notes}, {conditions}, {remise_globale}, {is_supplier}, GETDATE());

    IF EXISTS (SELECT 1 FROM [dbo].[{status_table}] WHERE [piece_id] = @piece_id_text)
    BEGIN
        UPDATE [dbo].[{status_table}]
        SET [statut] = {status},
            [updated_at] = GETDATE(),
            [updated_by] = {updated_by}
        WHERE [piece_id] = @piece_id_text;
    END
    ELSE
    BEGIN
        INSERT INTO [dbo].[{status_table}] ([piece_id], [statut], [updated_at], [updated_by])
        VALUES (@piece_id_text, {status}, GETDATE(), {updated_by});
    END;

    INSERT INTO [dbo].[{history_table}] ([id], [piece_id], [from_statut], [to_statut], [updated_at], [updated_by])
    VALUES ({history_id}, @piece_id_text, NULL, {status}, GETDATE(), {updated_by});

    COMMIT TRANSACTION;
    SELECT @piece_id_text AS piece_id, {numero} AS numero;
END TRY
BEGIN CATCH
    IF @@TRANCOUNT > 0
        ROLLBACK TRANSACTION;
    THROW;
END CATCH
",
        piece_id_value = if piece_id_is_identity {
            "NULL".to_string()
        } else {
            sql_opt_string(&piece_id_text)
        },
        piece_table = resolved.piece.quoted_name(),
        piece_columns = piece_columns.join(", "),
        piece_values = piece_values.join(", "),
        set_identity_piece_id = if piece_id_is_identity {
            "SET @piece_id_text = CAST(SCOPE_IDENTITY() AS NVARCHAR(50));".to_string()
        } else if !piece_id_text.is_empty() {
            format!("SET @piece_id_text = {};", sql_string(&piece_id_text))
        } else {
            String::new()
        },
        line_sql = line_sql_chunks.join("\n"),
        meta_table = META_TABLE,
        date_echeance = sql_opt_string(&invoice.date_echeance),
        tiers_code = sql_opt_string(&invoice.tiers_code),
        tiers_nom = sql_opt_string(&invoice.tiers_nom),
        tiers_adresse = sql_opt_string(&invoice.tiers_adresse),
        tiers_cp = sql_opt_string(&invoice.tiers_cp),
        tiers_ville = sql_opt_string(&invoice.tiers_ville),
        tiers_pays = sql_opt_string(&invoice.tiers_pays),
        tiers_siret = sql_opt_string(&invoice.tiers_siret),
        tiers_tva = sql_opt_string(&invoice.tiers_tva_intra),
        notes = sql_opt_string(&invoice.notes),
        conditions = sql_opt_string(&invoice.conditions),
        remise_globale = sql_number(invoice.remise_globale),
        is_supplier = if invoice.is_supplier { "1" } else { "0" },
        status_table = STATUS_TABLE,
        status = sql_string(&status),
        updated_by = sql_opt_string("SDB"),
        history_table = STATUS_HISTORY_TABLE,
        history_id = sql_string(&Uuid::new_v4().to_string()),
        numero = sql_string(&invoice.numero),
    );

    let created = db::execute_logged_query(
        &mut client,
        &sql,
        invoice_query_context(
            "create_invoice",
            "invoice_create",
            &id,
            &database_name,
            resolved.piece.quoted_name(),
            db::DEFAULT_QUERY_TIMEOUT_SECS,
        ),
    )
    .await?;
    if let Some(row) = created.rows.first() {
        invoice.id = parse_string(row.first());
        invoice.numero = parse_string(row.get(1));
    }
    fetch_invoice_internal(&state, &id, &invoice.id).await
}

#[tauri::command]
pub async fn update_invoice(
    state: State<'_, AppState>,
    id: String,
    invoice: InvoiceHeader,
) -> Result<InvoiceHeader, String> {
    let mut existing = fetch_invoice_internal(&state, &id, &invoice.id).await?;
    if !is_editable_status(&existing.statut) {
        return Err("Only Brouillon or Proforma invoices can be updated".to_string());
    }

    let _connection = load_connection_config(&state, &id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    ensure_invoice_support_tables(&mut client).await?;
    let resolved = resolve_invoice_schema(&state, &id, &mut client).await?;
    let database_name = client.config.database.clone();

    let invoice = normalize_invoice(invoice);
    let mut set_clauses = Vec::new();
    if let Some(column) = resolved.piece.column(&resolved.piece_numero) {
        set_clauses.push(format!(
            "{} = {}",
            br(&column.name),
            sql_value_for_column(column, &invoice.numero)
        ));
    }
    if let Some(column) = resolved.piece.column(&resolved.piece_date) {
        set_clauses.push(format!(
            "{} = {}",
            br(&column.name),
            sql_value_for_column(column, &invoice.date)
        ));
    }
    if let Some(column) = resolved.piece.column(&resolved.piece_reference) {
        set_clauses.push(format!(
            "{} = {}",
            br(&column.name),
            sql_value_for_column(column, &invoice.reference)
        ));
    }
    if let Some(column) = resolved.piece.column(&resolved.piece_tiers) {
        set_clauses.push(format!(
            "{} = {}",
            br(&column.name),
            sql_value_for_column(column, &invoice.tiers_id)
        ));
    }
    if let Some(column) = resolved.piece.column(&resolved.piece_nature) {
        set_clauses.push(format!(
            "{} = {}",
            br(&column.name),
            nature_to_sql_value(&invoice.nature, column)
        ));
    }
    if let Some(devise_column) = resolved
        .piece_devise
        .as_ref()
        .and_then(|name| resolved.piece.column(name))
    {
        set_clauses.push(format!(
            "{} = {}",
            br(&devise_column.name),
            sql_value_for_column(devise_column, &invoice.devise)
        ));
    }

    let line_piece_column = resolved
        .line
        .column(&resolved.line_piece)
        .ok_or_else(|| "Line piece column metadata missing".to_string())?;
    let mut line_sql_chunks = vec![format!(
        "DELETE FROM [dbo].[{line_meta}] WHERE [piece_id] = @piece_id_text;",
        line_meta = LINE_META_TABLE,
    )];
    line_sql_chunks.push(format!(
        "DELETE FROM {} WHERE {} = {};",
        resolved.line.quoted_name(),
        br(&resolved.line_piece),
        cast_text_to_column("@piece_id_text", line_piece_column)
    ));

    for line in &invoice.lignes {
        let mut columns = vec![br(&resolved.line_piece)];
        let mut values = vec![cast_text_to_column("@piece_id_text", line_piece_column)];

        if let Some(column) = resolved
            .line_article
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_value_for_column(column, &line.article_id));
        }
        if let Some(column) = resolved
            .line_libelle
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_value_for_column(column, &line.libelle));
        }
        if let Some(column) = resolved
            .line_qte
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_number(line.quantite));
        }
        if let Some(column) = resolved
            .line_pu_ht
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_number(line.prix_ht));
        }
        if let Some(column) = resolved
            .line_taux_tva
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_number(line.taux_tva));
        }
        if let Some(column) = resolved
            .line_montant_ht
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_number(line.montant_ht));
        }
        if let Some(column) = resolved
            .line_montant_ttc
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_number(line.montant_ttc));
        }
        if let Some(column) = resolved
            .line_remise
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_number(line.remise_pct));
        }
        if let Some(column) = resolved
            .line_ordre
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(line.ordre.to_string());
        }
        if let Some(column) = resolved
            .line_unite
            .as_ref()
            .and_then(|name| resolved.line.column(name))
        {
            columns.push(br(&column.name));
            values.push(sql_value_for_column(column, &line.unite));
        }

        line_sql_chunks.push(format!(
            "INSERT INTO {} ({}) VALUES ({});",
            resolved.line.quoted_name(),
            columns.join(", "),
            values.join(", ")
        ));
        line_sql_chunks.push(format!(
            "INSERT INTO [dbo].[{line_meta}] ([line_key], [piece_id], [ordre], [article_code], [unite], [updated_at])
             VALUES (CONCAT(@piece_id_text, N':', {ordre}), @piece_id_text, {ordre}, {article_code}, {unite}, GETDATE());",
            line_meta = LINE_META_TABLE,
            ordre = line.ordre,
            article_code = sql_opt_string(&line.article_code),
            unite = sql_opt_string(&line.unite),
        ));
    }

    let sql = format!(
        "
BEGIN TRY
    BEGIN TRANSACTION;
    DECLARE @piece_id_text NVARCHAR(50) = {piece_id};

    UPDATE {piece_table}
    SET {set_clauses}
    WHERE {piece_oid} = {piece_oid_value};

    {line_sql}

    MERGE [dbo].[{meta_table}] AS target
    USING (SELECT @piece_id_text AS [piece_id]) AS src
    ON target.[piece_id] = src.[piece_id]
    WHEN MATCHED THEN
        UPDATE SET
            [date_echeance] = {date_echeance},
            [tiers_code] = {tiers_code},
            [tiers_nom] = {tiers_nom},
            [tiers_adresse] = {tiers_adresse},
            [tiers_cp] = {tiers_cp},
            [tiers_ville] = {tiers_ville},
            [tiers_pays] = {tiers_pays},
            [tiers_siret] = {tiers_siret},
            [tiers_tva] = {tiers_tva},
            [notes] = {notes},
            [conditions] = {conditions},
            [remise_globale] = {remise_globale},
            [is_supplier] = {is_supplier},
            [updated_at] = GETDATE()
    WHEN NOT MATCHED THEN
        INSERT ([piece_id], [date_echeance], [tiers_code], [tiers_nom], [tiers_adresse], [tiers_cp], [tiers_ville], [tiers_pays], [tiers_siret], [tiers_tva], [notes], [conditions], [remise_globale], [is_supplier], [updated_at])
        VALUES (@piece_id_text, {date_echeance}, {tiers_code}, {tiers_nom}, {tiers_adresse}, {tiers_cp}, {tiers_ville}, {tiers_pays}, {tiers_siret}, {tiers_tva}, {notes}, {conditions}, {remise_globale}, {is_supplier}, GETDATE());

    COMMIT TRANSACTION;
END TRY
BEGIN CATCH
    IF @@TRANCOUNT > 0
        ROLLBACK TRANSACTION;
    THROW;
END CATCH
",
        piece_id = sql_string(&invoice.id),
        piece_table = resolved.piece.quoted_name(),
        set_clauses = set_clauses.join(", "),
        piece_oid = br(&resolved.piece_oid),
        piece_oid_value = cast_text_to_column(
            &sql_string(&invoice.id),
            resolved.piece.column(&resolved.piece_oid).unwrap(),
        ),
        line_sql = line_sql_chunks.join("\n"),
        meta_table = META_TABLE,
        date_echeance = sql_opt_string(&invoice.date_echeance),
        tiers_code = sql_opt_string(&invoice.tiers_code),
        tiers_nom = sql_opt_string(&invoice.tiers_nom),
        tiers_adresse = sql_opt_string(&invoice.tiers_adresse),
        tiers_cp = sql_opt_string(&invoice.tiers_cp),
        tiers_ville = sql_opt_string(&invoice.tiers_ville),
        tiers_pays = sql_opt_string(&invoice.tiers_pays),
        tiers_siret = sql_opt_string(&invoice.tiers_siret),
        tiers_tva = sql_opt_string(&invoice.tiers_tva_intra),
        notes = sql_opt_string(&invoice.notes),
        conditions = sql_opt_string(&invoice.conditions),
        remise_globale = sql_number(invoice.remise_globale),
        is_supplier = if invoice.is_supplier { "1" } else { "0" },
    );
    db::execute_logged_query(
        &mut client,
        &sql,
        invoice_query_context(
            "update_invoice",
            "invoice_update",
            &id,
            &database_name,
            resolved.piece.quoted_name(),
            db::DEFAULT_QUERY_TIMEOUT_SECS,
        ),
    )
    .await?;

    existing = fetch_invoice_internal(&state, &id, &invoice.id).await?;
    Ok(existing)
}

#[tauri::command]
pub async fn update_statut(
    state: State<'_, AppState>,
    id: String,
    piece_id: String,
    new_statut: String,
    updated_by: Option<String>,
) -> Result<StatutUpdateResult, String> {
    let _connection = load_connection_config(&state, &id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    ensure_invoice_support_tables(&mut client).await?;

    let current = get_current_status(&mut client, &piece_id)
        .await?
        .unwrap_or(StatutUpdateResult {
            piece_id: piece_id.clone(),
            statut: "Brouillon".to_string(),
            updated_at: String::new(),
            updated_by: String::new(),
        });
    let next = normalize_status(&new_statut);
    if current.statut == "Comptabilise" {
        return Err("Comptabilise invoices are locked".to_string());
    }
    if !validate_transition(&current.statut, &next) {
        return Err(format!(
            "Invalid status transition: {} -> {}",
            current.statut, next
        ));
    }

    upsert_status_batch(
        &mut client,
        &piece_id,
        &next,
        updated_by.as_deref().unwrap_or("SDB"),
        Some(&current.statut),
    )
    .await?;

    let updated = get_current_status(&mut client, &piece_id)
        .await?
        .ok_or_else(|| "Failed to read updated status".to_string())?;
    Ok(StatutUpdateResult {
        statut: status_display(&updated.statut),
        ..updated
    })
}

#[tauri::command]
pub async fn delete_invoice(
    state: State<'_, AppState>,
    id: String,
    piece_id: String,
) -> Result<(), String> {
    let invoice = fetch_invoice_internal(&state, &id, &piece_id).await?;
    if normalize_status(&invoice.statut) != "Brouillon" {
        return Err("Only Brouillon invoices can be deleted".to_string());
    }

    let _connection = load_connection_config(&state, &id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    ensure_invoice_support_tables(&mut client).await?;
    let resolved = resolve_invoice_schema(&state, &id, &mut client).await?;
    let database_name = client.config.database.clone();
    let line_piece_column = resolved
        .line
        .column(&resolved.line_piece)
        .ok_or_else(|| "Line piece column metadata missing".to_string())?;

    let sql = format!(
        "
BEGIN TRY
    BEGIN TRANSACTION;
    DELETE FROM [dbo].[{line_meta}] WHERE [piece_id] = {piece_id};
    DELETE FROM [dbo].[{history}] WHERE [piece_id] = {piece_id};
    DELETE FROM [dbo].[{meta}] WHERE [piece_id] = {piece_id};
    DELETE FROM [dbo].[{status}] WHERE [piece_id] = {piece_id};
    DELETE FROM {line_table} WHERE {line_piece} = {piece_fk};
    DELETE FROM {piece_table} WHERE {piece_oid} = {piece_oid_value};
    COMMIT TRANSACTION;
END TRY
BEGIN CATCH
    IF @@TRANCOUNT > 0
        ROLLBACK TRANSACTION;
    THROW;
END CATCH
",
        line_meta = LINE_META_TABLE,
        history = STATUS_HISTORY_TABLE,
        meta = META_TABLE,
        status = STATUS_TABLE,
        piece_id = sql_string(&piece_id),
        line_table = resolved.line.quoted_name(),
        line_piece = br(&resolved.line_piece),
        piece_fk = cast_text_to_column(&sql_string(&piece_id), line_piece_column),
        piece_table = resolved.piece.quoted_name(),
        piece_oid = br(&resolved.piece_oid),
        piece_oid_value = cast_text_to_column(
            &sql_string(&piece_id),
            resolved.piece.column(&resolved.piece_oid).unwrap(),
        ),
    );
    db::execute_logged_query(
        &mut client,
        &sql,
        invoice_query_context(
            "delete_invoice",
            "invoice_delete",
            &id,
            &database_name,
            resolved.piece.quoted_name(),
            db::DEFAULT_QUERY_TIMEOUT_SECS,
        ),
    )
    .await?;
    Ok(())
}

async fn resolve_entry_table(
    client: &mut db::DbClient,
    resolved: &ResolvedInvoiceSchema,
) -> Result<ResolvedTable, String> {
    let tables = db::get_tables(client).await?;
    sage_entity_service::load_table(
        client,
        &tables,
        &[resolved.sage_schema.table_ecritures.clone()],
    )
    .await
}

async fn resolve_account_table(
    client: &mut db::DbClient,
    resolved: &ResolvedInvoiceSchema,
) -> Result<ResolvedTable, String> {
    let tables = db::get_tables(client).await?;
    sage_entity_service::load_table(
        client,
        &tables,
        &[resolved.sage_schema.table_comptes.clone()],
    )
    .await
}

async fn lookup_account_value(
    client: &mut db::DbClient,
    account_table: &ResolvedTable,
    resolved: &ResolvedInvoiceSchema,
    connection_id: &str,
    database: &str,
    code: &str,
) -> Result<String, String> {
    if code.trim().is_empty() {
        return Err("Account code is empty".to_string());
    }

    let id_column =
        sage_entity_service::pick_optional(account_table, &["oid", "CG_Num", "CT_Num", "ACC"])
            .unwrap_or_else(|| resolved.sage_schema.col_compte_num.clone());
    let code_column = sage_entity_service::pick_optional(
        account_table,
        &[
            &resolved.sage_schema.col_compte_num,
            "codeCompte",
            "CG_Num",
            "CT_Num",
            "ACC",
        ],
    )
    .unwrap_or_else(|| resolved.sage_schema.col_compte_num.clone());
    let sql = format!(
        "SELECT TOP 1 CAST({id_col} AS NVARCHAR(50)) AS value FROM {table} WHERE CAST({code_col} AS NVARCHAR(100)) = {code}",
        id_col = br(&id_column),
        table = account_table.quoted_name(),
        code_col = br(&code_column),
        code = sql_string(code),
    );
    let data = db::execute_logged_query(
        client,
        &sql,
        invoice_query_context(
            "comptabiliser_invoice",
            "invoice_post_account_lookup",
            connection_id,
            database,
            account_table.quoted_name(),
            db::DEFAULT_QUERY_TIMEOUT_SECS,
        ),
    )
    .await?;
    let value = data
        .rows
        .first()
        .map(|row| parse_string(row.first()))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| code.to_string());
    Ok(value)
}

async fn resolve_tiers_role_for_entry(
    client: &mut db::DbClient,
    resolved: &ResolvedInvoiceSchema,
    connection_id: &str,
    database: &str,
    invoice: &InvoiceHeader,
) -> Result<Option<String>, String> {
    if resolved.edition != SageEdition::Sage1000 {
        return Ok(None);
    }
    let role_table = match resolved.role_tiers.as_ref() {
        Some(table) => table,
        None => return Ok(None),
    };
    let role_tiers_tiers = match resolved.role_tiers_tiers.as_ref() {
        Some(column) => column,
        None => return Ok(None),
    };

    let sql = format!(
        "SELECT TOP 1 CAST([oid] AS NVARCHAR(50)) AS role_id FROM {} WHERE {} = {} ORDER BY [oid] ASC",
        role_table.quoted_name(),
        br(role_tiers_tiers),
        cast_text_to_column(
            &sql_string(&invoice.tiers_id),
            role_table.column(role_tiers_tiers).unwrap(),
        )
    );
    let data = db::execute_logged_query(
        client,
        &sql,
        invoice_query_context(
            "comptabiliser_invoice",
            "invoice_post_role_lookup",
            connection_id,
            database,
            role_table.quoted_name(),
            db::DEFAULT_QUERY_TIMEOUT_SECS,
        ),
    )
    .await?;
    Ok(data
        .rows
        .first()
        .map(|row| parse_string(row.first()))
        .filter(|value| !value.is_empty()))
}

#[tauri::command]
pub async fn comptabiliser_invoice(
    state: State<'_, AppState>,
    id: String,
    piece_id: String,
) -> Result<ComptabilisationResult, String> {
    let invoice = fetch_invoice_internal(&state, &id, &piece_id).await?;
    if normalize_status(&invoice.statut) == "Comptabilise" {
        return Err("Invoice is already comptabilise".to_string());
    }
    if nature_to_label(&invoice.nature) != "Facture" {
        return Err("Only Facture invoices can be comptabilise".to_string());
    }

    let _connection = load_connection_config(&state, &id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    ensure_invoice_support_tables(&mut client).await?;
    let resolved = resolve_invoice_schema(&state, &id, &mut client).await?;
    let entry_table = resolve_entry_table(&mut client, &resolved).await?;
    let account_table = resolve_account_table(&mut client, &resolved).await?;
    let database_name = client.config.database.clone();

    let (cfg_ar, cfg_sales, cfg_vat) = {
        let config = state.config.lock().map_err(|e| e.to_string())?;
        (
            config.account_ar.clone(),
            config.account_sales.clone(),
            config.account_vat.clone(),
        )
    };

    let tiers_account_code = if invoice.is_supplier {
        "401000" // Still hardcoded for suppliers for now, but could be made configurable too
    } else {
        &cfg_ar
    };
    let sales_account_code = if invoice.is_supplier {
        "607000"
    } else {
        &cfg_sales
    };
    let vat_account_code = if invoice.is_supplier {
        "445620"
    } else {
        &cfg_vat
    };

    let tiers_account_value = lookup_account_value(
        &mut client,
        &account_table,
        &resolved,
        &id,
        &database_name,
        tiers_account_code,
    )
    .await?;
    let sales_account_value = lookup_account_value(
        &mut client,
        &account_table,
        &resolved,
        &id,
        &database_name,
        sales_account_code,
    )
    .await?;
    let vat_account_value = lookup_account_value(
        &mut client,
        &account_table,
        &resolved,
        &id,
        &database_name,
        vat_account_code,
    )
    .await?;
    let role_value =
        resolve_tiers_role_for_entry(&mut client, &resolved, &id, &database_name, &invoice).await?;

    let mut tva_breakdown = BTreeMap::<String, f64>::new();
    for line in &invoice.lignes {
        let key = format!("{:.2}", line.taux_tva);
        let amount = tva_breakdown.entry(key).or_insert(0.0);
        *amount = round2(*amount + line.montant_ht);
    }

    let entry_columns = {
        let mut columns = Vec::new();
        let mut push = |name: &str| {
            if entry_table.column(name).is_some() {
                columns.push(name.to_string());
            }
        };
        push(&resolved.sage_schema.col_date);
        push("JO_Num");
        push("CG_Num");
        push(&resolved.sage_schema.col_compte);
        push(&resolved.sage_schema.col_libelle);
        push(&resolved.sage_schema.col_piece);
        push("EC_RefPiece");
        push("EC_Montant");
        push("EC_Sens");
        push(&resolved.sage_schema.col_tiers);
        columns
    };

    let build_entry_values = |account_value: &str,
                              label: &str,
                              debit: f64,
                              credit: f64,
                              tiers_value: Option<&str>|
     -> Vec<String> {
        entry_columns
            .iter()
            .map(|column| {
                if column.eq_ignore_ascii_case(&resolved.sage_schema.col_date) {
                    sql_string(&invoice.date)
                } else if column.eq_ignore_ascii_case("JO_Num") {
                    sql_string("VT")
                } else if column.eq_ignore_ascii_case("CG_Num")
                    || column.eq_ignore_ascii_case(&resolved.sage_schema.col_compte)
                {
                    let col = entry_table.column(column).unwrap();
                    sql_value_for_column(col, account_value)
                } else if column.eq_ignore_ascii_case(&resolved.sage_schema.col_libelle) {
                    sql_string(label)
                } else if column.eq_ignore_ascii_case(&resolved.sage_schema.col_piece) {
                    let col = entry_table.column(column).unwrap();
                    sql_value_for_column(col, &invoice.id)
                } else if column.eq_ignore_ascii_case("EC_RefPiece") {
                    sql_string(&invoice.numero)
                } else if column.eq_ignore_ascii_case("EC_Montant") {
                    sql_number(if debit > 0.0 { debit } else { credit })
                } else if column.eq_ignore_ascii_case("EC_Sens") {
                    if debit > 0.0 {
                        "0".to_string()
                    } else {
                        "1".to_string()
                    }
                } else if column.eq_ignore_ascii_case(&resolved.sage_schema.col_tiers) {
                    match tiers_value {
                        Some(value) => sql_string(value),
                        None => "NULL".to_string(),
                    }
                } else if let Some(column_info) = entry_table.column(column) {
                    if column_info.is_nullable {
                        "NULL".to_string()
                    } else if is_numeric_type(&column_info.data_type) {
                        "0".to_string()
                    } else {
                        sql_string("")
                    }
                } else {
                    "NULL".to_string()
                }
            })
            .collect()
    };

    let mut inserts = Vec::new();
    let mut entry_ids = Vec::new();
    let tiers_tiers_value = if resolved.edition == SageEdition::Sage1000 {
        role_value.as_deref()
    } else {
        Some(invoice.tiers_code.as_str())
    };

    let add_entry = |inserts: &mut Vec<String>,
                     entry_ids: &mut Vec<String>,
                     entry_order: i32,
                     role: &str,
                     amount: f64,
                     values: Vec<String>| {
        let entry_id = Uuid::new_v4().to_string();
        inserts.push(format!(
            "INSERT INTO {entry_table} ({columns}) VALUES ({values});
             INSERT INTO [dbo].[{log_table}] ([id], [piece_id], [entry_order], [entry_role], [amount], [created_at])
             VALUES ({entry_id}, {piece_id}, {entry_order}, {entry_role}, {amount}, GETDATE());",
            entry_table = entry_table.quoted_name(),
            columns = entry_columns.iter().map(|name| br(name)).collect::<Vec<_>>().join(", "),
            values = values.join(", "),
            log_table = COMPTA_LOG_TABLE,
            entry_id = sql_string(&entry_id),
            piece_id = sql_string(&piece_id),
            entry_order = entry_order,
            entry_role = sql_string(role),
            amount = sql_number(amount),
        ));
        entry_ids.push(entry_id);
    };

    if invoice.is_supplier {
        add_entry(
            &mut inserts,
            &mut entry_ids,
            1,
            "achat",
            invoice.total_ht,
            build_entry_values(
                &sales_account_value,
                &format!("Achat {}", invoice.numero),
                invoice.total_ht,
                0.0,
                None,
            ),
        );
        add_entry(
            &mut inserts,
            &mut entry_ids,
            2,
            "tva_deductible",
            invoice.total_tva,
            build_entry_values(
                &vat_account_value,
                &format!("TVA {}", invoice.numero),
                invoice.total_tva,
                0.0,
                None,
            ),
        );
        add_entry(
            &mut inserts,
            &mut entry_ids,
            3,
            "fournisseur",
            invoice.total_ttc,
            build_entry_values(
                &tiers_account_value,
                &format!("Fournisseur {}", invoice.numero),
                0.0,
                invoice.total_ttc,
                tiers_tiers_value,
            ),
        );
    } else {
        add_entry(
            &mut inserts,
            &mut entry_ids,
            1,
            "client",
            invoice.total_ttc,
            build_entry_values(
                &tiers_account_value,
                &format!("Client {}", invoice.numero),
                invoice.total_ttc,
                0.0,
                tiers_tiers_value,
            ),
        );
        let mut order = 2;
        for amount in tva_breakdown.values() {
            add_entry(
                &mut inserts,
                &mut entry_ids,
                order,
                "vente",
                *amount,
                build_entry_values(
                    &sales_account_value,
                    &format!("Vente {}", invoice.numero),
                    0.0,
                    *amount,
                    None,
                ),
            );
            order += 1;
        }
        add_entry(
            &mut inserts,
            &mut entry_ids,
            order,
            "tva_collectee",
            invoice.total_tva,
            build_entry_values(
                &vat_account_value,
                &format!("TVA {}", invoice.numero),
                0.0,
                invoice.total_tva,
                None,
            ),
        );
    }

    let sql = format!(
        "
BEGIN TRY
    BEGIN TRANSACTION;
    {inserts}
    IF EXISTS (SELECT 1 FROM [dbo].[{status_table}] WHERE [piece_id] = {piece_id})
    BEGIN
        UPDATE [dbo].[{status_table}]
        SET [statut] = N'Comptabilise',
            [updated_at] = GETDATE(),
            [updated_by] = N'SDB'
        WHERE [piece_id] = {piece_id};
    END
    ELSE
    BEGIN
        INSERT INTO [dbo].[{status_table}] ([piece_id], [statut], [updated_at], [updated_by])
        VALUES ({piece_id}, N'Comptabilise', GETDATE(), N'SDB');
    END;

    INSERT INTO [dbo].[{history_table}] ([id], [piece_id], [from_statut], [to_statut], [updated_at], [updated_by])
    VALUES ({history_id}, {piece_id}, {from_statut}, N'Comptabilise', GETDATE(), N'SDB');

    COMMIT TRANSACTION;
END TRY
BEGIN CATCH
    IF @@TRANCOUNT > 0
        ROLLBACK TRANSACTION;
    THROW;
END CATCH
",
        inserts = inserts.join("\n"),
        status_table = STATUS_TABLE,
        history_table = STATUS_HISTORY_TABLE,
        piece_id = sql_string(&piece_id),
        history_id = sql_string(&Uuid::new_v4().to_string()),
        from_statut = sql_string(&normalize_status(&invoice.statut)),
    );
    db::execute_logged_query(
        &mut client,
        &sql,
        invoice_query_context(
            "comptabiliser_invoice",
            "invoice_post",
            &id,
            &database_name,
            entry_table.quoted_name(),
            db::DEFAULT_QUERY_TIMEOUT_SECS,
        ),
    )
    .await?;

    Ok(ComptabilisationResult {
        piece_id,
        entry_ids,
        statut: "Comptabilise".to_string(),
    })
}

#[tauri::command]
pub async fn list_tiers(
    state: State<'_, AppState>,
    id: String,
    type_tiers: Option<String>,
    search: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<TiersSummary>, String> {
    let connection = load_connection_config(&state, &id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    let support_tables = detect_invoice_support_table_availability(&mut client).await;
    let type_filter = type_tiers.unwrap_or_else(|| "all".to_string());
    let limit = limit.unwrap_or(40).clamp(1, 100);
    let normalized_search = search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let entity_ctx =
        sage_entity_service::resolve_entity_search_context(&state, &id, &mut client).await?;
    let resolved = entity_ctx.tiers;

    if resolved.is_none() {
        // Fall back to local-only when native schema isn't available.
        if !support_tables.local_tiers {
            return Ok(Vec::new());
        }
        let local_type_filter = match type_filter.trim().to_ascii_lowercase().as_str() {
            "clients" => Some(vec!["client", "les_deux"]),
            "fournisseurs" => Some(vec!["fournisseur", "les_deux"]),
            _ => None,
        };
        let local_sql =
            build_local_tiers_sql(&local_type_filter, normalized_search.as_deref(), limit);
        let local_data = db::execute_logged_query(
            &mut client,
            &local_sql,
            db::QueryExecutionContext::new("list_tiers", "invoice_search_tiers_local")
                .with_connection_id(id.clone())
                .with_database(connection.database.clone())
                .with_table(LOCAL_TIERS_TABLE)
                .with_timeout_secs(db::SEARCH_QUERY_TIMEOUT_SECS),
        )
        .await
        .unwrap_or_else(|_| empty_table_data());

        return Ok(local_data
            .rows
            .iter()
            .map(|row| TiersSummary {
                id: sage_entity_service::parse_string(row.first()),
                code: sage_entity_service::parse_string(row.get(1)),
                nom: sage_entity_service::parse_string(row.get(2)),
                adresse: sage_entity_service::parse_string(row.get(3)),
                cp: sage_entity_service::parse_string(row.get(4)),
                ville: sage_entity_service::parse_string(row.get(5)),
                pays: sage_entity_service::parse_string(row.get(6)),
                siret: sage_entity_service::parse_string(row.get(7)),
                email: sage_entity_service::parse_string(row.get(8)),
                telephone: sage_entity_service::parse_string(row.get(9)),
                tva_intra: sage_entity_service::parse_string(row.get(10)),
                type_tiers: sage_entity_service::parse_string(row.get(11)),
                encours: 0.0,
                nb_factures: 0,
                is_local: true,
            })
            .collect());
    }
    let resolved = resolved.unwrap();

    let type_expr = resolved
        .col_type
        .as_ref()
        .map(|column| {
            format!(
                "CASE
                WHEN TRY_CAST({} AS INT) = 1 THEN N'fournisseur'
                WHEN TRY_CAST({} AS INT) = 2 THEN N'les_deux'
                WHEN CAST({} AS NVARCHAR(50)) IN (N'0', N'1', N'2') THEN
                    CASE WHEN TRY_CAST({} AS INT) = 1 THEN N'fournisseur' WHEN TRY_CAST({} AS INT) = 2 THEN N'les_deux' ELSE N'client' END
                ELSE N'client'
            END",
                sage_entity_service::qualify("t", column),
                sage_entity_service::qualify("t", column),
                sage_entity_service::qualify("t", column),
                sage_entity_service::qualify("t", column),
                sage_entity_service::qualify("t", column),
            )
        })
        .unwrap_or_else(|| "N'client'".to_string());

    let mut final_sql = format!(
        "
SELECT TOP ({limit})
    CAST({tiers_id} AS NVARCHAR(50)) AS id,
    CAST({tiers_code} AS NVARCHAR(100)) AS code,
    CAST({tiers_nom} AS NVARCHAR(255)) AS nom,
    {tiers_adresse} AS adresse,
    {tiers_cp} AS cp,
    {tiers_ville} AS ville,
    {tiers_pays} AS pays,
    {tiers_siret} AS siret,
    {tiers_email} AS email,
    {tiers_telephone} AS telephone,
    {tiers_tva} AS tva_intra,
    {type_expr} AS type_tiers,
    CAST(0 AS DECIMAL(18, 2)) AS encours,
    CAST(0 AS BIGINT) AS nb_factures
FROM {tiers_table} t
WHERE 1=1
",
        limit = limit,
        tiers_id = sage_entity_service::qualify("t", &resolved.col_id),
        tiers_code = sage_entity_service::qualify("t", &resolved.col_code),
        tiers_nom = sage_entity_service::qualify("t", &resolved.col_nom),
        tiers_adresse = resolved
            .col_adresse
            .as_ref()
            .map(|column| format!(
                "COALESCE(CAST({} AS NVARCHAR(255)), N'')",
                sage_entity_service::qualify("t", column)
            ))
            .unwrap_or_else(|| "N''".to_string()),
        tiers_cp = resolved
            .col_cp
            .as_ref()
            .map(|column| format!(
                "COALESCE(CAST({} AS NVARCHAR(50)), N'')",
                sage_entity_service::qualify("t", column)
            ))
            .unwrap_or_else(|| "N''".to_string()),
        tiers_ville = resolved
            .col_ville
            .as_ref()
            .map(|column| format!(
                "COALESCE(CAST({} AS NVARCHAR(100)), N'')",
                sage_entity_service::qualify("t", column)
            ))
            .unwrap_or_else(|| "N''".to_string()),
        tiers_pays = resolved
            .col_pays
            .as_ref()
            .map(|column| format!(
                "COALESCE(CAST({} AS NVARCHAR(100)), N'')",
                sage_entity_service::qualify("t", column)
            ))
            .unwrap_or_else(|| "N''".to_string()),
        tiers_siret = resolved
            .col_siret
            .as_ref()
            .map(|column| format!(
                "COALESCE(CAST({} AS NVARCHAR(100)), N'')",
                sage_entity_service::qualify("t", column)
            ))
            .unwrap_or_else(|| "N''".to_string()),
        tiers_email = resolved
            .col_email
            .as_ref()
            .map(|column| format!(
                "COALESCE(CAST({} AS NVARCHAR(100)), N'')",
                sage_entity_service::qualify("t", column)
            ))
            .unwrap_or_else(|| "N''".to_string()),
        tiers_telephone = resolved
            .col_telephone
            .as_ref()
            .map(|column| format!(
                "COALESCE(CAST({} AS NVARCHAR(100)), N'')",
                sage_entity_service::qualify("t", column)
            ))
            .unwrap_or_else(|| "N''".to_string()),
        tiers_tva = resolved
            .col_tva
            .as_ref()
            .map(|column| format!(
                "COALESCE(CAST({} AS NVARCHAR(100)), N'')",
                sage_entity_service::qualify("t", column)
            ))
            .unwrap_or_else(|| "N''".to_string()),
        type_expr = type_expr,
        tiers_table = resolved.table.quoted_name(),
    );

    if type_filter.eq_ignore_ascii_case("clients") {
        final_sql.push_str(&format!(" AND ({} IN (N'client', N'les_deux'))", type_expr));
    } else if type_filter.eq_ignore_ascii_case("fournisseurs") {
        final_sql.push_str(&format!(
            " AND ({} IN (N'fournisseur', N'les_deux'))",
            type_expr
        ));
    }

    if let Some(value) = normalized_search.as_ref() {
        let search_value = format!("%{}%", value.replace('\'', "''"));
        final_sql.push_str(&format!(
            " AND (
                CAST({} AS NVARCHAR(100)) LIKE {}
                OR CAST({} AS NVARCHAR(255)) LIKE {}
                OR CAST({} AS NVARCHAR(255)) LIKE {}
                OR CAST({} AS NVARCHAR(100)) LIKE {}
                OR CAST({} AS NVARCHAR(100)) LIKE {}
                OR CAST({} AS NVARCHAR(100)) LIKE {}
                OR CAST({} AS NVARCHAR(100)) LIKE {}
            )",
            sage_entity_service::qualify("t", &resolved.col_code),
            sage_entity_service::sql_string(&search_value),
            sage_entity_service::qualify("t", &resolved.col_nom),
            sage_entity_service::sql_string(&search_value),
            resolved
                .col_adresse
                .as_ref()
                .map(|column| sage_entity_service::qualify("t", column))
                .unwrap_or_else(|| "N''".to_string()),
            sage_entity_service::sql_string(&search_value),
            resolved
                .col_ville
                .as_ref()
                .map(|column| sage_entity_service::qualify("t", column))
                .unwrap_or_else(|| "N''".to_string()),
            sage_entity_service::sql_string(&search_value),
            resolved
                .col_email
                .as_ref()
                .map(|column| sage_entity_service::qualify("t", column))
                .unwrap_or_else(|| "N''".to_string()),
            sage_entity_service::sql_string(&search_value),
            resolved
                .col_telephone
                .as_ref()
                .map(|column| sage_entity_service::qualify("t", column))
                .unwrap_or_else(|| "N''".to_string()),
            sage_entity_service::sql_string(&search_value),
            resolved
                .col_siret
                .as_ref()
                .map(|column| sage_entity_service::qualify("t", column))
                .unwrap_or_else(|| "N''".to_string()),
            sage_entity_service::sql_string(&search_value),
        ));
    }
    final_sql.push_str(&format!(
        " ORDER BY {} ASC",
        sage_entity_service::qualify("t", &resolved.col_nom)
    ));

    let data = db::execute_logged_query(
        &mut client,
        &final_sql,
        db::QueryExecutionContext::new("list_tiers", "invoice_search_tiers")
            .with_connection_id(id.clone())
            .with_database(connection.database.clone())
            .with_table(resolved.table.quoted_name())
            .with_timeout_secs(db::SEARCH_QUERY_TIMEOUT_SECS),
    )
    .await?;

    let mut result: Vec<TiersSummary> = data
        .rows
        .iter()
        .map(|row| TiersSummary {
            id: sage_entity_service::parse_string(row.first()),
            code: sage_entity_service::parse_string(row.get(1)),
            nom: sage_entity_service::parse_string(row.get(2)),
            adresse: sage_entity_service::parse_string(row.get(3)),
            cp: sage_entity_service::parse_string(row.get(4)),
            ville: sage_entity_service::parse_string(row.get(5)),
            pays: sage_entity_service::parse_string(row.get(6)),
            siret: sage_entity_service::parse_string(row.get(7)),
            email: sage_entity_service::parse_string(row.get(8)),
            telephone: sage_entity_service::parse_string(row.get(9)),
            tva_intra: sage_entity_service::parse_string(row.get(10)),
            type_tiers: sage_entity_service::parse_string(row.get(11)),
            encours: 0.0,
            nb_factures: 0,
            is_local: false,
        })
        .collect();

    // Merge locally-created tiers from SDB_TIERS
    if support_tables.local_tiers {
        let local_type_filter = match type_filter.trim().to_ascii_lowercase().as_str() {
            "clients" => Some(vec!["client", "les_deux"]),
            "fournisseurs" => Some(vec!["fournisseur", "les_deux"]),
            _ => None,
        };
        let local_sql =
            build_local_tiers_sql(&local_type_filter, normalized_search.as_deref(), limit);
        if let Ok(local_data) = db::execute_logged_query(
            &mut client,
            &local_sql,
            db::QueryExecutionContext::new("list_tiers", "invoice_search_tiers_local_merge")
                .with_connection_id(id.clone())
                .with_database(connection.database.clone())
                .with_table(LOCAL_TIERS_TABLE)
                .with_timeout_secs(db::SEARCH_QUERY_TIMEOUT_SECS),
        )
        .await
        {
            let native_ids: std::collections::HashSet<String> =
                result.iter().map(|t| t.code.to_lowercase()).collect();
            for row in &local_data.rows {
                let entry = TiersSummary {
                    id: sage_entity_service::parse_string(row.first()),
                    code: sage_entity_service::parse_string(row.get(1)),
                    nom: sage_entity_service::parse_string(row.get(2)),
                    adresse: sage_entity_service::parse_string(row.get(3)),
                    cp: sage_entity_service::parse_string(row.get(4)),
                    ville: sage_entity_service::parse_string(row.get(5)),
                    pays: sage_entity_service::parse_string(row.get(6)),
                    siret: sage_entity_service::parse_string(row.get(7)),
                    email: sage_entity_service::parse_string(row.get(8)),
                    telephone: sage_entity_service::parse_string(row.get(9)),
                    tva_intra: sage_entity_service::parse_string(row.get(10)),
                    type_tiers: sage_entity_service::parse_string(row.get(11)),
                    encours: 0.0,
                    nb_factures: 0,
                    is_local: true,
                };
                if !native_ids.contains(&entry.code.to_lowercase()) {
                    result.push(entry);
                }
            }
        }
    }
    result.sort_by(|a, b| a.nom.to_lowercase().cmp(&b.nom.to_lowercase()));
    result.truncate(limit as usize);
    Ok(result)
}

fn empty_table_data() -> crate::state::TableData {
    crate::state::TableData {
        columns: vec![],
        rows: vec![],
        total_count: 0,
        page: 0,
        page_size: 0,
    }
}

fn build_local_tiers_sql(
    type_filter: &Option<Vec<&str>>,
    search: Option<&str>,
    limit: u32,
) -> String {
    let mut sql = format!(
        "SELECT TOP ({limit}) [id],[code],[nom],COALESCE([adresse],N'') AS adresse,COALESCE([cp],N'') AS cp,COALESCE([ville],N'') AS ville,COALESCE([pays],N'') AS pays,COALESCE([siret],N'') AS siret,COALESCE([email],N'') AS email,COALESCE([telephone],N'') AS telephone,COALESCE([tva_intra],N'') AS tva_intra,[type_tiers] FROM [dbo].[{table}] WHERE 1=1",
        limit = limit,
        table = LOCAL_TIERS_TABLE
    );
    if let Some(types) = type_filter {
        let list: Vec<String> = types.iter().map(|t| format!("N'{}'", t)).collect();
        sql.push_str(&format!(" AND [type_tiers] IN ({})", list.join(",")));
    }
    if let Some(s) = search.filter(|s| !s.trim().is_empty()) {
        let val = format!("%{}%", s.trim().replace('\'', "''"));
        sql.push_str(&format!(
            " AND ([code] LIKE N'{}' OR [nom] LIKE N'{}' OR [adresse] LIKE N'{}' OR [ville] LIKE N'{}' OR [email] LIKE N'{}' OR [telephone] LIKE N'{}' OR [siret] LIKE N'{}')",
            val, val, val, val, val, val, val
        ));
    }
    sql.push_str(" ORDER BY [nom] ASC");
    sql
}

#[tauri::command]
pub async fn list_articles(
    state: State<'_, AppState>,
    id: String,
    search: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<ArticleSummary>, String> {
    let connection = load_connection_config(&state, &id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    let support_tables = detect_invoice_support_table_availability(&mut client).await;
    let limit = limit.unwrap_or(40).clamp(1, 100);
    let normalized_search = search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let entity_ctx =
        sage_entity_service::resolve_entity_search_context(&state, &id, &mut client).await?;
    let resolved = entity_ctx.article;

    if resolved.is_none() {
        if !support_tables.local_article {
            return Ok(Vec::new());
        }
        let local_sql = build_local_article_sql(normalized_search.as_deref(), limit);
        let local_data = db::execute_logged_query(
            &mut client,
            &local_sql,
            db::QueryExecutionContext::new("list_articles", "invoice_search_articles_local")
                .with_connection_id(id.clone())
                .with_database(connection.database.clone())
                .with_table(LOCAL_ARTICLE_TABLE)
                .with_timeout_secs(db::SEARCH_QUERY_TIMEOUT_SECS),
        )
        .await
        .unwrap_or_else(|_| empty_table_data());
        return Ok(local_data
            .rows
            .iter()
            .map(|row| ArticleSummary {
                id: sage_entity_service::parse_string(row.first()),
                code: sage_entity_service::parse_string(row.get(1)),
                libelle: sage_entity_service::parse_string(row.get(2)),
                prix_ht: sage_entity_service::parse_f64(row.get(3)),
                taux_tva: sage_entity_service::parse_f64(row.get(4)),
                unite: sage_entity_service::parse_string(row.get(5)),
                reference: sage_entity_service::parse_string(row.get(6)),
                en_activite: sage_entity_service::parse_bool(row.get(7)),
            })
            .collect());
    }
    let resolved = resolved.unwrap();

    let mut sql = format!(
        "
SELECT TOP ({limit})
    CAST({id_expr} AS NVARCHAR(50)) AS id,
    CAST({code_expr} AS NVARCHAR(100)) AS code,
    CAST({label_expr} AS NVARCHAR(255)) AS libelle,
    COALESCE(TRY_CAST({pu_expr} AS DECIMAL(18, 6)), 0) AS prix_ht,
    COALESCE(TRY_CAST({tva_expr} AS DECIMAL(18, 6)), 0) AS taux_tva,
    COALESCE(CAST({unite_expr} AS NVARCHAR(50)), N'') AS unite,
    COALESCE(CAST({ref_expr} AS NVARCHAR(100)), N'') AS reference,
    CASE
        WHEN {active_expr} IS NULL THEN CAST(1 AS BIT)
        WHEN CAST({active_expr} AS NVARCHAR(50)) IN (N'1', N'true', N'TRUE', N'oui') THEN CAST(1 AS BIT)
        WHEN CAST({active_expr} AS NVARCHAR(50)) IN (N'0', N'false', N'FALSE', N'non') THEN CAST(0 AS BIT)
        ELSE CASE WHEN TRY_CAST({active_expr} AS INT) = 0 THEN CAST(1 AS BIT) ELSE CAST(0 AS BIT) END
    END AS en_activite
FROM {article_table} a
WHERE 1=1
",
        limit = limit,
        id_expr = resolved
            .col_id
            .as_ref()
            .map(|column| sage_entity_service::qualify("a", column))
            .unwrap_or_else(|| sage_entity_service::qualify("a", &resolved.col_code)),
        code_expr = sage_entity_service::qualify("a", &resolved.col_code),
        label_expr = sage_entity_service::qualify("a", &resolved.col_libelle),
        pu_expr = resolved
            .col_pu
            .as_ref()
            .map(|column| sage_entity_service::qualify("a", column))
            .unwrap_or_else(|| "0".to_string()),
        tva_expr = resolved
            .col_tva
            .as_ref()
            .map(|column| sage_entity_service::qualify("a", column))
            .unwrap_or_else(|| "0".to_string()),
        unite_expr = resolved
            .col_unite
            .as_ref()
            .map(|column| sage_entity_service::qualify("a", column))
            .unwrap_or_else(|| "N''".to_string()),
        ref_expr = resolved
            .col_ref
            .as_ref()
            .map(|column| sage_entity_service::qualify("a", column))
            .unwrap_or_else(|| sage_entity_service::qualify("a", &resolved.col_code)),
        active_expr = resolved
            .col_active
            .as_ref()
            .map(|column| sage_entity_service::qualify("a", column))
            .unwrap_or_else(|| "NULL".to_string()),
        article_table = resolved.table.quoted_name(),
    );
    if let Some(value) = normalized_search.as_ref() {
        let search_value = format!("%{}%", value.replace('\'', "''"));
        sql.push_str(&format!(
            " AND (
                CAST({} AS NVARCHAR(100)) LIKE {}
                OR CAST({} AS NVARCHAR(255)) LIKE {}
                OR CAST({} AS NVARCHAR(100)) LIKE {}
            )",
            sage_entity_service::qualify("a", &resolved.col_code),
            sage_entity_service::sql_string(&search_value),
            sage_entity_service::qualify("a", &resolved.col_libelle),
            sage_entity_service::sql_string(&search_value),
            resolved
                .col_ref
                .as_ref()
                .map(|column| sage_entity_service::qualify("a", column))
                .unwrap_or_else(|| "N''".to_string()),
            sage_entity_service::sql_string(&search_value),
        ));
    }
    sql.push_str(&format!(
        " ORDER BY {} ASC",
        sage_entity_service::qualify("a", &resolved.col_code)
    ));

    let data = db::execute_logged_query(
        &mut client,
        &sql,
        db::QueryExecutionContext::new("list_articles", "invoice_search_articles")
            .with_connection_id(id.clone())
            .with_database(connection.database.clone())
            .with_table(resolved.table.quoted_name())
            .with_timeout_secs(db::SEARCH_QUERY_TIMEOUT_SECS),
    )
    .await?;
    let mut result: Vec<ArticleSummary> = data
        .rows
        .iter()
        .map(|row| ArticleSummary {
            id: sage_entity_service::parse_string(row.first()),
            code: sage_entity_service::parse_string(row.get(1)),
            libelle: sage_entity_service::parse_string(row.get(2)),
            prix_ht: round2(sage_entity_service::parse_f64(row.get(3))),
            taux_tva: round2(sage_entity_service::parse_f64(row.get(4))),
            unite: sage_entity_service::parse_string(row.get(5)),
            reference: sage_entity_service::parse_string(row.get(6)),
            en_activite: sage_entity_service::parse_bool(row.get(7)),
        })
        .collect();

    // Merge locally-created articles from SDB_ARTICLE
    if support_tables.local_article {
        let local_sql = build_local_article_sql(normalized_search.as_deref(), limit);
        if let Ok(local_data) = db::execute_logged_query(
            &mut client,
            &local_sql,
            db::QueryExecutionContext::new("list_articles", "invoice_search_articles_local_merge")
                .with_connection_id(id.clone())
                .with_database(connection.database.clone())
                .with_table(LOCAL_ARTICLE_TABLE)
                .with_timeout_secs(db::SEARCH_QUERY_TIMEOUT_SECS),
        )
        .await
        {
            let native_ids: std::collections::HashSet<String> =
                result.iter().map(|a| a.code.to_lowercase()).collect();
            for row in &local_data.rows {
                let entry = ArticleSummary {
                    id: sage_entity_service::parse_string(row.first()),
                    code: sage_entity_service::parse_string(row.get(1)),
                    libelle: sage_entity_service::parse_string(row.get(2)),
                    prix_ht: round2(sage_entity_service::parse_f64(row.get(3))),
                    taux_tva: round2(sage_entity_service::parse_f64(row.get(4))),
                    unite: sage_entity_service::parse_string(row.get(5)),
                    reference: sage_entity_service::parse_string(row.get(6)),
                    en_activite: sage_entity_service::parse_bool(row.get(7)),
                };
                if !native_ids.contains(&entry.code.to_lowercase()) {
                    result.push(entry);
                }
            }
        }
    }
    result.sort_by(|a, b| a.code.to_lowercase().cmp(&b.code.to_lowercase()));
    result.truncate(limit as usize);
    Ok(result)
}

fn build_local_article_sql(search: Option<&str>, limit: u32) -> String {
    let mut sql = format!(
        "SELECT TOP ({limit}) [id],[code],COALESCE([libelle],N'') AS libelle,COALESCE([prix_ht],0) AS prix_ht,COALESCE([taux_tva],0) AS taux_tva,COALESCE([unite],N'') AS unite,COALESCE([reference],N'') AS reference,[en_activite] FROM [dbo].[{table}] WHERE 1=1",
        limit = limit,
        table = LOCAL_ARTICLE_TABLE
    );
    if let Some(s) = search.filter(|s| !s.trim().is_empty()) {
        let val = format!("%{}%", s.trim().replace('\'', "''"));
        sql.push_str(&format!(
            " AND ([code] LIKE N'{}' OR [libelle] LIKE N'{}' OR [reference] LIKE N'{}')",
            val, val, val
        ));
    }
    sql.push_str(" ORDER BY [code] ASC");
    sql
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_invoice_sql_uses_empty_support_sources_when_tables_are_missing() {
        let status_join = status_join_sql(false, "[p].[oid]");
        let meta_join = meta_join_sql(false, "[p].[oid]");
        let line_meta_join = line_meta_join_sql(false, "N'piece-1'", "[l].[ordre]");

        assert!(status_join.contains("WHERE 1 = 0"));
        assert!(meta_join.contains("WHERE 1 = 0"));
        assert!(line_meta_join.contains("WHERE 1 = 0"));
        assert!(!status_join.contains(&format!("[dbo].[{}]", STATUS_TABLE)));
        assert!(!meta_join.contains(&format!("[dbo].[{}]", META_TABLE)));
        assert!(!line_meta_join.contains(&format!("[dbo].[{}]", LINE_META_TABLE)));
    }

    #[test]
    fn read_only_invoice_sql_uses_real_support_tables_when_available() {
        let status_join = status_join_sql(true, "[p].[oid]");
        let meta_join = meta_join_sql(true, "[p].[oid]");
        let line_meta_join = line_meta_join_sql(true, "N'piece-1'", "[l].[ordre]");

        assert!(status_join.contains(&format!("[dbo].[{}]", STATUS_TABLE)));
        assert!(meta_join.contains(&format!("[dbo].[{}]", META_TABLE)));
        assert!(line_meta_join.contains(&format!("[dbo].[{}]", LINE_META_TABLE)));
    }
}

#[tauri::command]
pub async fn create_tiers(
    state: State<'_, AppState>,
    id: String,
    mut tiers: TiersSummary,
) -> Result<TiersSummary, String> {
    let _connection = load_connection_config(&state, &id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    ensure_invoice_support_tables(&mut client).await?;
    tiers.id = format!("sdb-{}", Uuid::new_v4());
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let sql = format!(
        "INSERT INTO [dbo].[{table}] ([id],[code],[nom],[adresse],[cp],[ville],[pays],[siret],[email],[telephone],[tva_intra],[type_tiers],[created_at],[updated_at]) VALUES ({id},{code},{nom},{adresse},{cp},{ville},{pays},{siret},{email},{telephone},{tva_intra},{type_tiers},{now},{now})",
        table = LOCAL_TIERS_TABLE,
        id = sql_string(&tiers.id),
        code = sql_string(&tiers.code),
        nom = sql_opt_string(&tiers.nom),
        adresse = sql_opt_string(&tiers.adresse),
        cp = sql_opt_string(&tiers.cp),
        ville = sql_opt_string(&tiers.ville),
        pays = sql_opt_string(&tiers.pays),
        siret = sql_opt_string(&tiers.siret),
        email = sql_opt_string(&tiers.email),
        telephone = sql_opt_string(&tiers.telephone),
        tva_intra = sql_opt_string(&tiers.tva_intra),
        type_tiers = sql_string(if tiers.type_tiers.is_empty() { "client" } else { &tiers.type_tiers }),
        now = sql_string(&now),
    );
    db::execute_raw_query(&mut client, &sql).await?;
    tiers.is_local = true;
    Ok(tiers)
}

#[tauri::command]
pub async fn update_tiers(
    state: State<'_, AppState>,
    id: String,
    tiers: TiersSummary,
) -> Result<TiersSummary, String> {
    let _connection = load_connection_config(&state, &id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    ensure_invoice_support_tables(&mut client).await?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let sql = format!(
        "UPDATE [dbo].[{table}] SET [code]={code},[nom]={nom},[adresse]={adresse},[cp]={cp},[ville]={ville},[pays]={pays},[siret]={siret},[email]={email},[telephone]={telephone},[tva_intra]={tva_intra},[type_tiers]={type_tiers},[updated_at]={now} WHERE [id]={id}",
        table = LOCAL_TIERS_TABLE,
        code = sql_string(&tiers.code),
        nom = sql_opt_string(&tiers.nom),
        adresse = sql_opt_string(&tiers.adresse),
        cp = sql_opt_string(&tiers.cp),
        ville = sql_opt_string(&tiers.ville),
        pays = sql_opt_string(&tiers.pays),
        siret = sql_opt_string(&tiers.siret),
        email = sql_opt_string(&tiers.email),
        telephone = sql_opt_string(&tiers.telephone),
        tva_intra = sql_opt_string(&tiers.tva_intra),
        type_tiers = sql_string(if tiers.type_tiers.is_empty() { "client" } else { &tiers.type_tiers }),
        now = sql_string(&now),
        id = sql_string(&tiers.id),
    );
    db::execute_raw_query(&mut client, &sql).await?;
    Ok(tiers)
}

#[tauri::command]
pub async fn delete_tiers(
    state: State<'_, AppState>,
    id: String,
    tiers_id: String,
) -> Result<(), String> {
    let _connection = load_connection_config(&state, &id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    ensure_invoice_support_tables(&mut client).await?;
    let database_name = client.config.database.clone();
    let sql = format!(
        "DELETE FROM [dbo].[{table}] WHERE [id]={id}",
        table = LOCAL_TIERS_TABLE,
        id = sql_string(&tiers_id),
    );
    db::execute_logged_query(
        &mut client,
        &sql,
        invoice_query_context(
            "delete_tiers",
            "tiers_delete",
            &id,
            &database_name,
            LOCAL_TIERS_TABLE,
            db::DEFAULT_QUERY_TIMEOUT_SECS,
        ),
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn create_article(
    state: State<'_, AppState>,
    id: String,
    mut article: ArticleSummary,
) -> Result<ArticleSummary, String> {
    let _connection = load_connection_config(&state, &id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    ensure_invoice_support_tables(&mut client).await?;
    article.id = format!("sdb-{}", Uuid::new_v4());
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let sql = format!(
        "INSERT INTO [dbo].[{table}] ([id],[code],[libelle],[prix_ht],[taux_tva],[unite],[reference],[en_activite],[created_at],[updated_at]) VALUES ({id},{code},{libelle},{prix_ht},{taux_tva},{unite},{reference},{active},{now},{now})",
        table = LOCAL_ARTICLE_TABLE,
        id = sql_string(&article.id),
        code = sql_string(&article.code),
        libelle = sql_opt_string(&article.libelle),
        prix_ht = article.prix_ht,
        taux_tva = article.taux_tva,
        unite = sql_opt_string(&article.unite),
        reference = sql_opt_string(&article.reference),
        active = if article.en_activite { 1 } else { 0 },
        now = sql_string(&now),
    );
    db::execute_raw_query(&mut client, &sql).await?;
    Ok(article)
}

#[tauri::command]
pub async fn update_article(
    state: State<'_, AppState>,
    id: String,
    article: ArticleSummary,
) -> Result<ArticleSummary, String> {
    let _connection = load_connection_config(&state, &id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    ensure_invoice_support_tables(&mut client).await?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let sql = format!(
        "UPDATE [dbo].[{table}] SET [code]={code},[libelle]={libelle},[prix_ht]={prix_ht},[taux_tva]={taux_tva},[unite]={unite},[reference]={reference},[en_activite]={active},[updated_at]={now} WHERE [id]={id}",
        table = LOCAL_ARTICLE_TABLE,
        code = sql_string(&article.code),
        libelle = sql_opt_string(&article.libelle),
        prix_ht = article.prix_ht,
        taux_tva = article.taux_tva,
        unite = sql_opt_string(&article.unite),
        reference = sql_opt_string(&article.reference),
        active = if article.en_activite { 1 } else { 0 },
        now = sql_string(&now),
        id = sql_string(&article.id),
    );
    db::execute_raw_query(&mut client, &sql).await?;
    Ok(article)
}

#[tauri::command]
pub async fn delete_article(
    state: State<'_, AppState>,
    id: String,
    article_id: String,
) -> Result<(), String> {
    let _connection = load_connection_config(&state, &id)?;
    let mut client = get_active_client(state.inner(), &id).await?;
    ensure_invoice_support_tables(&mut client).await?;
    let sql = format!(
        "DELETE FROM [dbo].[{table}] WHERE [id]={id}",
        table = LOCAL_ARTICLE_TABLE,
        id = sql_string(&article_id),
    );
    db::execute_raw_query(&mut client, &sql).await?;
    Ok(())
}
