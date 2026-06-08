use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tauri::State;

use crate::db;
use crate::invoice_compat::InvoiceSchema;
use crate::sage_compat::SageEdition;
use crate::state::{AppState, ColumnInfo, TableInfo};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TiersSummary {
    pub id: String,
    pub code: String,
    pub nom: String,
    pub adresse: String,
    pub cp: String,
    pub ville: String,
    pub pays: String,
    pub siret: String,
    pub email: String,
    pub telephone: String,
    pub tva_intra: String,
    pub type_tiers: String,
    #[serde(default)]
    pub encours: f64,
    #[serde(default)]
    pub nb_factures: i64,
    #[serde(default)]
    pub is_local: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArticleSummary {
    pub id: String,
    pub code: String,
    pub libelle: String,
    #[serde(default)]
    pub description: String,
    pub prix_ht: f64,
    #[serde(default)]
    pub currency: String,
    pub taux_tva: f64,
    #[serde(default)]
    pub taux_bic: f64,
    pub unite: String,
    pub reference: String,
    #[serde(default)]
    pub category: String,
    pub en_activite: bool,
    #[serde(default)]
    pub revenue_account: String,
    #[serde(default)]
    pub expense_account: String,
    #[serde(default)]
    pub vat_account: String,
    #[serde(default)]
    pub bic_account: String,
    #[serde(default)]
    pub tax_exempt: bool,
    #[serde(default)]
    pub custom_tax_rules: String,
    #[serde(default)]
    pub product_family_code: String,
    #[serde(default)]
    pub accounting_category_code: String,
    #[serde(default)]
    pub tax_code: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedTable {
    pub schema: String,
    pub name: String,
    pub columns: HashMap<String, ColumnInfo>,
}

impl ResolvedTable {
    pub fn quoted_name(&self) -> String {
        format!(
            "[{}].[{}]",
            self.schema.replace(']', "]]"),
            self.name.replace(']', "]]")
        )
    }

    pub fn column(&self, name: &str) -> Option<&ColumnInfo> {
        self.columns.get(&name.to_ascii_lowercase())
    }
}

pub struct EntitySearchContext {
    pub tiers: Option<ResolvedTiersMapping>,
    pub article: Option<ResolvedArticleMapping>,
}

pub struct ResolvedTiersMapping {
    pub table: ResolvedTable,
    pub col_id: String,
    pub col_code: String,
    pub col_nom: String,
    pub col_adresse: Option<String>,
    pub col_cp: Option<String>,
    pub col_ville: Option<String>,
    pub col_pays: Option<String>,
    pub col_siret: Option<String>,
    pub col_email: Option<String>,
    pub col_telephone: Option<String>,
    pub col_tva: Option<String>,
    pub col_type: Option<String>,
}

pub struct ResolvedArticleMapping {
    pub table: ResolvedTable,
    pub col_id: Option<String>,
    pub col_code: String,
    pub col_libelle: String,
    pub col_pu: Option<String>,
    pub col_tva: Option<String>,
    pub col_ref: Option<String>,
    pub col_unite: Option<String>,
    pub col_active: Option<String>,
}

pub async fn resolve_entity_search_context(
    state: &State<'_, AppState>,
    connection_id: &str,
    client: &mut db::DbClient,
) -> Result<EntitySearchContext, String> {
    let connection = state.resolve_connection_config(connection_id)?;
    let edition = state
        .get_connection_schema(connection_id)
        .map(|schema| schema.edition)
        .unwrap_or_else(|| SageEdition::from_edition_str(&connection.sage_edition));

    let native = InvoiceSchema::for_edition(&edition);
    let tables = db::get_tables(client).await?;

    println!(
        "[SageEntityService] Resolving search context for connection: {}, DB: {}, Edition: {:?}",
        connection_id, connection.database, edition
    );

    let tiers_table = load_table_optional(
        client,
        &tables,
        &[
            native.table_tiers.clone(),
            "F_COMPTET".to_string(),
            "COMPTET".to_string(),
            "F_TIERS".to_string(),
            "TIERS".to_string(),
            "TTIERS".to_string(),
            "CUSTOMERS".to_string(),
            "SUPPLIERS".to_string(),
        ],
    )
    .await;

    if let Some(ref t) = tiers_table {
        println!(
            "[SageEntityService] Detected tiers table: {}.{}",
            t.schema, t.name
        );
    } else {
        println!("[SageEntityService] No tiers table detected");
    }

    let article_table = load_table_optional(
        client,
        &tables,
        &[
            native.table_article.clone(),
            "F_ARTICLE".to_string(),
            "ARTICLE".to_string(),
            "TARTICLE".to_string(),
            "ITEMS".to_string(),
            "PRODUCTS".to_string(),
        ],
    )
    .await;

    if let Some(ref a) = article_table {
        println!(
            "[SageEntityService] Detected article table: {}.{}",
            a.schema, a.name
        );
    } else {
        println!("[SageEntityService] No article table detected");
    }

    let tiers_mapping = if let Some(table) = tiers_table {
        Some(ResolvedTiersMapping {
            col_id: pick_required(&table, &["oid", "CT_Num", "id", "TIERS_ID"])
                .unwrap_or_else(|_| "CT_Num".to_string()),
            col_code: pick_required(
                &table,
                &[
                    &native.table_tiers,
                    "code",
                    "CT_Num",
                    "numero",
                    "TIERS_CODE",
                ],
            )
            .unwrap_or_else(|_| "CT_Num".to_string()),
            col_nom: pick_required(
                &table,
                &[
                    "raisonSociale",
                    "CT_Intitule",
                    "Caption",
                    "nom",
                    "TIERS_NAME",
                    "INTITULE",
                ],
            )
            .unwrap_or_else(|_| "CT_Intitule".to_string()),
            col_adresse: pick_optional(&table, &["adresse", "adresse1", "CT_Adresse", "voie"]),
            col_cp: pick_optional(
                &table,
                &["codePostal", "CT_CodePostal", "cp", "CODE_POSTAL"],
            ),
            col_ville: pick_optional(&table, &["ville", "CT_Ville"]),
            col_pays: pick_optional(&table, &["pays", "CT_Pays"]),
            col_siret: pick_optional(&table, &["siret", "CT_Siret", "siren"]),
            col_email: pick_optional(&table, &["email", "EMail", "CT_EMail"]),
            col_telephone: pick_optional(&table, &["telephone", "CT_Telephone"]),
            col_tva: pick_optional(&table, &["numTvaIntracom", "CT_Identifiant", "tva"]),
            col_type: pick_optional(&table, &["CT_Type", "typePersonne", "type"]),
            table,
        })
    } else {
        None
    };

    let article_mapping = if let Some(table) = article_table {
        Some(ResolvedArticleMapping {
            col_id: pick_optional(&table, &["oid", "AR_Ref", "id"]),
            col_code: pick_required(
                &table,
                &[&native.col_article_code, "AR_Ref", "code", "ITEM_CODE"],
            )
            .unwrap_or_else(|_| "AR_Ref".to_string()),
            col_libelle: pick_required(
                &table,
                &[
                    &native.col_article_libelle,
                    "AR_Design",
                    "Caption",
                    "libelle",
                    "ITEM_NAME",
                    "DESIGNATION",
                ],
            )
            .unwrap_or_else(|_| "AR_Design".to_string()),
            col_pu: pick_optional(
                &table,
                &[
                    &native.col_article_pu,
                    "AR_PrixVen",
                    "prix_ht",
                    "UNIT_PRICE",
                ],
            ),
            col_tva: pick_optional(
                &table,
                &[
                    &native.col_article_tva,
                    "AR_TauxTva",
                    "taux_tva",
                    "VAT_RATE",
                ],
            ),
            col_ref: pick_optional(&table, &[&native.col_article_ref, "AR_Ref", "reference"]),
            col_unite: pick_optional(&table, &["unite", "AR_UniteVen", "UV_Code"]),
            col_active: pick_optional(&table, &["actif", "AR_Sommeil", "en_activite"]),
            table,
        })
    } else {
        None
    };

    Ok(EntitySearchContext {
        tiers: tiers_mapping,
        article: article_mapping,
    })
}

pub async fn load_table(
    client: &mut db::DbClient,
    tables: &[TableInfo],
    candidates: &[String],
) -> Result<ResolvedTable, String> {
    let info = candidates
        .iter()
        .find_map(|candidate| {
            tables
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(candidate))
        })
        .ok_or_else(|| format!("Required table not found. Tried: {}", candidates.join(", ")))?;

    let columns = db::get_columns(client, &info.schema, &info.name).await?;
    let column_map = columns
        .into_iter()
        .map(|column| (column.name.to_ascii_lowercase(), column))
        .collect::<HashMap<_, _>>();

    Ok(ResolvedTable {
        schema: info.schema.clone(),
        name: info.name.clone(),
        columns: column_map,
    })
}

async fn load_table_optional(
    client: &mut db::DbClient,
    tables: &[TableInfo],
    candidates: &[String],
) -> Option<ResolvedTable> {
    let info = candidates.iter().find_map(|candidate| {
        tables
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(candidate))
    })?;

    let columns = db::get_columns(client, &info.schema, &info.name)
        .await
        .ok()?;
    let column_map = columns
        .into_iter()
        .map(|column| (column.name.to_ascii_lowercase(), column))
        .collect::<HashMap<_, _>>();

    Some(ResolvedTable {
        schema: info.schema.clone(),
        name: info.name.clone(),
        columns: column_map,
    })
}

pub fn pick_required(table: &ResolvedTable, candidates: &[&str]) -> Result<String, String> {
    candidates
        .iter()
        .find_map(|candidate| table.column(candidate).map(|col| col.name.clone()))
        .ok_or_else(|| {
            format!(
                "Required column missing on {}.{} ({})",
                table.schema,
                table.name,
                candidates.join(", ")
            )
        })
}

pub fn pick_optional(table: &ResolvedTable, candidates: &[&str]) -> Option<String> {
    candidates
        .iter()
        .find_map(|candidate| table.column(candidate).map(|col| col.name.clone()))
}

pub fn br(value: &str) -> String {
    format!("[{}]", value.replace(']', "]]"))
}

pub fn qualify(alias: &str, column: &str) -> String {
    format!("{}.[{}]", alias, column.replace(']', "]]"))
}

pub fn sql_string(value: &str) -> String {
    format!("N'{}'", value.replace('\'', "''"))
}

pub fn sql_opt_string(value: &str) -> String {
    if value.trim().is_empty() {
        "NULL".to_string()
    } else {
        sql_string(value.trim())
    }
}

pub fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

pub fn parse_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => {
            if *flag {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        _ => String::new(),
    }
}

pub fn parse_f64(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(number)) => number.as_f64().unwrap_or(0.0),
        Some(Value::String(text)) => text.trim().replace(',', ".").parse::<f64>().unwrap_or(0.0),
        Some(Value::Bool(flag)) => {
            if *flag {
                1.0
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}

pub fn parse_i32(value: Option<&Value>) -> i32 {
    round2(parse_f64(value)) as i32
}

pub fn parse_i64(value: Option<&Value>) -> i64 {
    round2(parse_f64(value)) as i64
}

pub fn parse_bool(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_i64().unwrap_or(0) != 0,
        Some(Value::String(text)) => matches!(
            text.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "y" | "oui"
        ),
        _ => false,
    }
}
