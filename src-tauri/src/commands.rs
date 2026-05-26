use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tauri::State;

use crate::db;
use crate::sage_compat::{DetectionResult, SageEdition, SageSchema};
use crate::state::{
    ActiveConnection, AppState, ColumnInfo, ConnectionConfig, ConnectionStatus, Filter,
    QueryHistoryEntry, RelationshipInfo, SavedQuery, TableData, TableInfo,
};

const MAX_QUERY_HISTORY: usize = 100;
const MAX_FIELD_HISTORY: usize = 10;

fn load_connection_config(
    state: &State<'_, AppState>,
    id: &str,
) -> Result<ConnectionConfig, String> {
    state.get_active_connection(id)
}

fn append_query_history(
    state: &State<'_, AppState>,
    entry: QueryHistoryEntry,
) -> Result<(), String> {
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    config.query_history.insert(0, entry);
    config.query_history.truncate(MAX_QUERY_HISTORY);
    drop(config);
    state.save_config()
}

fn hash_secret(secret: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(secret.as_bytes(), &salt)
        .map_err(|e| e.to_string())
        .map(|hash| hash.to_string())
}

fn verify_secret(secret: &str, hash_str: &str) -> Result<bool, String> {
    let parsed = PasswordHash::new(hash_str).map_err(|e| e.to_string())?;
    Ok(Argon2::default()
        .verify_password(secret.as_bytes(), &parsed)
        .is_ok())
}

// ─── Auth / Security ─────────────────────────────────────────────────────────

/// Check whether a password has been configured
#[tauri::command]
pub fn has_password(state: State<AppState>) -> bool {
    let config = state.config.lock().unwrap();
    config.password_hash.is_some()
}

/// Set (or change) the application password
#[tauri::command]
pub fn set_password(state: State<AppState>, password: String) -> Result<(), String> {
    let hash = hash_secret(&password)?;
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    config.password_hash = Some(hash);
    drop(config);
    state.save_config()
}

/// Verify the application password
#[tauri::command]
pub fn verify_password(state: State<AppState>, password: String) -> Result<bool, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    if let Some(hash_str) = &config.password_hash {
        verify_secret(&password, hash_str)
    } else {
        // No password configured — allow access
        Ok(true)
    }
}

/// Remove the application password
#[tauri::command]
pub fn remove_password(state: State<AppState>) -> Result<(), String> {
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    config.password_hash = None;
    drop(config);
    state.save_config()
}

#[tauri::command]
pub fn has_pin(state: State<AppState>) -> bool {
    let config = state.config.lock().unwrap();
    config.password_hash.is_some()
}

#[tauri::command]
pub fn set_pin(state: State<AppState>, pin: String) -> Result<(), String> {
    set_password(state, pin)
}

#[tauri::command]
pub fn verify_pin(state: State<AppState>, pin: String) -> Result<bool, String> {
    verify_password(state, pin)
}

#[tauri::command]
pub fn remove_pin(state: State<AppState>) -> Result<(), String> {
    remove_password(state)
}

#[tauri::command]
pub fn has_admin_password(state: State<AppState>) -> bool {
    let config = state.config.lock().unwrap();
    config.admin_password_hash.is_some()
}

#[tauri::command]
pub fn set_admin_password(state: State<AppState>, password: String) -> Result<(), String> {
    let hash = hash_secret(&password)?;
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    config.admin_password_hash = Some(hash);
    drop(config);
    state.save_config()
}

#[tauri::command]
pub fn verify_admin_password(state: State<AppState>, password: String) -> Result<bool, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    if let Some(hash_str) = &config.admin_password_hash {
        verify_secret(&password, hash_str)
    } else {
        Ok(true)
    }
}

#[tauri::command]
pub fn remove_admin_password(state: State<AppState>) -> Result<(), String> {
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    config.admin_password_hash = None;
    drop(config);
    state.save_config()
}

// ─── Settings ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct AppSettings {
    pub query_timeout_secs: u64,
    pub dashboard_timeout_secs: u64,
    pub login_timeout_secs: u64,
    pub account_ar: String,
    pub account_sales: String,
    pub account_vat: String,
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<AppSettings, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    Ok(AppSettings {
        query_timeout_secs: config.query_timeout_secs,
        dashboard_timeout_secs: config.dashboard_timeout_secs,
        login_timeout_secs: config.login_timeout_secs,
        account_ar: config.account_ar.clone(),
        account_sales: config.account_sales.clone(),
        account_vat: config.account_vat.clone(),
    })
}

#[tauri::command]
pub fn save_settings(state: State<AppState>, settings: AppSettings) -> Result<(), String> {
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    config.query_timeout_secs = settings.query_timeout_secs;
    config.dashboard_timeout_secs = settings.dashboard_timeout_secs;
    config.login_timeout_secs = settings.login_timeout_secs;
    config.account_ar = settings.account_ar;
    config.account_sales = settings.account_sales;
    config.account_vat = settings.account_vat;
    drop(config);
    state.save_config()
}

// ─── Field History ────────────────────────────────────────────────────────────

/// Get the input history for a field key (most recent first, max 10)
#[tauri::command]
pub fn get_field_history(state: State<AppState>, field: String) -> Result<Vec<String>, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    let history = config
        .input_history
        .get(&field)
        .cloned()
        .unwrap_or_default();
    Ok(history.into_iter().take(MAX_FIELD_HISTORY).collect())
}

/// Save a value to the input history for a field key
#[tauri::command]
pub fn save_field_history(
    state: State<AppState>,
    field: String,
    value: String,
) -> Result<(), String> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Ok(());
    }
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    let list = config.input_history.entry(field).or_default();
    list.retain(|v| v != &trimmed);
    list.insert(0, trimmed);
    list.truncate(MAX_FIELD_HISTORY);
    drop(config);
    state.save_config()
}

/// Remove a single entry from a field's history
#[tauri::command]
pub fn remove_field_history_entry(
    state: State<AppState>,
    field: String,
    value: String,
) -> Result<(), String> {
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    if let Some(list) = config.input_history.get_mut(&field) {
        list.retain(|v| v != &value);
    }
    drop(config);
    state.save_config()
}

// ─── Connection Management ────────────────────────────────────────────────────

/// List all saved connections
#[tauri::command]
pub fn list_connections(state: State<AppState>) -> Result<Vec<ConnectionConfig>, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    // Return configs with masked passwords
    let masked: Vec<ConnectionConfig> = config
        .connections
        .iter()
        .map(|c| {
            let mut mc = c.clone();
            if !mc.password.is_empty() {
                mc.password = "••••••••".to_string();
            }
            mc
        })
        .collect();
    Ok(masked)
}

/// Save a new or updated connection
#[tauri::command]
pub fn save_connection(
    state: State<AppState>,
    connection: ConnectionConfig,
) -> Result<ConnectionConfig, String> {
    let mut config = state.config.lock().map_err(|e| e.to_string())?;

    let existing = config
        .connections
        .iter_mut()
        .find(|c| c.id == connection.id);

    if let Some(existing) = existing {
        // Update — preserve password if masked
        let password = if connection.password == "••••••••" {
            existing.password.clone()
        } else {
            connection.password.clone()
        };
        *existing = connection.clone();
        existing.password = password;
        let updated = existing.clone();
        drop(config);
        state.save_config()?;

        // Persist history for tracked fields
        let _ = save_field_history(
            state.clone(),
            "conn_host".to_string(),
            connection.host.clone(),
        );
        let _ = save_field_history(
            state.clone(),
            "conn_instance".to_string(),
            connection.instance_name.clone(),
        );
        let _ = save_field_history(
            state.clone(),
            "conn_database".to_string(),
            connection.database.clone(),
        );
        let _ = save_field_history(
            state.clone(),
            "conn_username".to_string(),
            connection.username.clone(),
        );
        {
            let mut active = state.active_connections.lock().map_err(|e| e.to_string())?;
            if let Some(active_connection) = active.get_mut(&updated.id) {
                active_connection.config = updated.clone();
            }
        }
        state.invalidate_client_cache(&updated.id)?;
        sync_active_schema_for_connection(&state, &updated.id, &updated)?;

        Ok(updated)
    } else {
        // New connection
        let mut new_conn = connection;
        new_conn.id = uuid::Uuid::new_v4().to_string();
        config.connections.push(new_conn.clone());
        drop(config);
        state.save_config()?;

        // Persist history for tracked fields
        let _ = save_field_history(
            state.clone(),
            "conn_host".to_string(),
            new_conn.host.clone(),
        );
        let _ = save_field_history(
            state.clone(),
            "conn_instance".to_string(),
            new_conn.instance_name.clone(),
        );
        let _ = save_field_history(
            state.clone(),
            "conn_database".to_string(),
            new_conn.database.clone(),
        );
        let _ = save_field_history(
            state.clone(),
            "conn_username".to_string(),
            new_conn.username.clone(),
        );
        state.invalidate_client_cache(&new_conn.id)?;

        Ok(new_conn)
    }
}

/// Delete a saved connection
#[tauri::command]
pub fn delete_connection(state: State<AppState>, id: String) -> Result<(), String> {
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    config.connections.retain(|c| c.id != id);
    config.saved_queries.retain(|q| q.connection_id != id);
    config.query_history.retain(|q| q.connection_id != id);
    drop(config);

    // Also disconnect if active
    let mut active = state.active_connections.lock().map_err(|e| e.to_string())?;
    active.remove(&id);
    drop(active);

    let mut schemas = state.active_schemas.lock().map_err(|e| e.to_string())?;
    schemas.remove(&id);
    drop(schemas);
    state.invalidate_client_cache(&id)?;

    state.save_config()
}

/// Test a connection without saving it
#[tauri::command]
pub async fn test_connection(connection: ConnectionConfig) -> Result<String, String> {
    db::test_connection(&connection).await
}

#[tauri::command]
pub async fn discover_databases(connection: ConnectionConfig) -> Result<Vec<String>, String> {
    let mut client = db::connect(&connection).await?;
    db::get_databases(&mut client).await
}

// ─── Schema Detection ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct SchemaResolutionOutcome {
    schema: SageSchema,
    source: &'static str,
    evidence: Vec<String>,
    confidence: u8,
}

fn explicit_schema_for_connection(connection: &ConnectionConfig) -> Option<SageSchema> {
    match connection.sage_edition.as_str() {
        "sage100" => Some(SageSchema::for_edition(&SageEdition::Sage100)),
        "sage1000" => Some(SageSchema::for_edition(&SageEdition::Sage1000)),
        "sagex3" => Some(SageSchema::for_edition(&SageEdition::SageX3)),
        "custom" => Some(
            connection
                .custom_schema
                .clone()
                .unwrap_or_else(|| SageSchema::for_edition(&SageEdition::Generic)),
        ),
        _ => None,
    }
}

fn sync_active_schema_for_connection(
    state: &State<'_, AppState>,
    id: &str,
    connection: &ConnectionConfig,
) -> Result<(), String> {
    let mut schemas = state.active_schemas.lock().map_err(|e| e.to_string())?;
    if let Some(schema) = explicit_schema_for_connection(connection) {
        schemas.insert(id.to_string(), schema);
    } else {
        schemas.remove(id);
    }
    Ok(())
}

fn connection_profile_summary(connection: &ConnectionConfig) -> String {
    let host = connection.host.trim();
    let instance = connection.instance_name.trim();
    let server = if instance.is_empty() {
        host.to_string()
    } else {
        format!("{}\\{}", host, instance)
    };
    format!(
        "server={} database={} auth={} encrypt={} trust_cert={}",
        server,
        connection.database.trim(),
        if connection.use_windows_auth {
            "windows"
        } else {
            "sql"
        },
        connection.encrypt,
        connection.trust_cert,
    )
}

fn detect_sage_schema_from_probes(
    found100: Vec<String>,
    found_x3: Vec<String>,
    found1000: Vec<String>,
    has_tecriture_edate: bool,
) -> DetectionResult {
    if found100.len() >= 3 {
        let schema = SageSchema::for_edition(&SageEdition::Sage100);
        return DetectionResult {
            edition: "sage100".into(),
            confidence: 95,
            evidence: found100,
            schema,
        };
    }

    if found_x3.len() >= 3 {
        let schema = SageSchema::for_edition(&SageEdition::SageX3);
        return DetectionResult {
            edition: "sagex3".into(),
            confidence: 92,
            evidence: found_x3,
            schema,
        };
    }

    if found1000.len() >= 4 {
        let mut confidence = 95;
        let mut evidence = found1000;
        if evidence
            .iter()
            .any(|name| name.eq_ignore_ascii_case("TECRITURE"))
            && has_tecriture_edate
        {
            confidence = 99;
            evidence.push("TECRITURE.eDate".into());
        }

        let mut schema = SageSchema::for_edition(&SageEdition::Sage1000);
        schema.confidence = confidence;
        return DetectionResult {
            edition: "sage1000".into(),
            confidence,
            evidence,
            schema,
        };
    }

    let mut evidence = Vec::new();
    evidence.extend(found100.into_iter().map(|name| format!("sage100:{}", name)));
    evidence.extend(found_x3.into_iter().map(|name| format!("sagex3:{}", name)));
    evidence.extend(
        found1000
            .into_iter()
            .map(|name| format!("sage1000:{}", name)),
    );
    if has_tecriture_edate {
        evidence.push("sage1000:TECRITURE.eDate".into());
    }

    let schema = SageSchema::for_edition(&SageEdition::Generic);
    DetectionResult {
        edition: "generic".into(),
        confidence: 0,
        evidence,
        schema,
    }
}

fn has_custom_mapping(schema: &SageSchema) -> bool {
    !schema.table_ecritures.trim().is_empty()
        || !schema.table_comptes.trim().is_empty()
        || !schema.table_tiers.trim().is_empty()
        || !schema.table_journaux.trim().is_empty()
}

fn resolve_schema_from_selection(
    requested_edition: &str,
    custom_schema: Option<SageSchema>,
    detection: Option<DetectionResult>,
) -> SchemaResolutionOutcome {
    let requested = requested_edition.trim().to_ascii_lowercase();
    let custom_schema = custom_schema.filter(has_custom_mapping);

    match requested.as_str() {
        "sage100" => {
            let schema = SageSchema::for_edition(&SageEdition::Sage100);
            SchemaResolutionOutcome {
                confidence: schema.confidence,
                schema,
                source: "manual",
                evidence: vec!["manual:sage100".into()],
            }
        }
        "sage1000" => {
            let schema = SageSchema::for_edition(&SageEdition::Sage1000);
            SchemaResolutionOutcome {
                confidence: schema.confidence,
                schema,
                source: "manual",
                evidence: vec!["manual:sage1000".into()],
            }
        }
        "sagex3" => {
            let schema = SageSchema::for_edition(&SageEdition::SageX3);
            SchemaResolutionOutcome {
                confidence: schema.confidence,
                schema,
                source: "manual",
                evidence: vec!["manual:sagex3".into()],
            }
        }
        "custom" => {
            let schema =
                custom_schema.unwrap_or_else(|| SageSchema::for_edition(&SageEdition::Generic));
            let confidence = schema.confidence;
            SchemaResolutionOutcome {
                schema,
                confidence,
                source: "custom",
                evidence: vec!["manual:custom".into()],
            }
        }
        _ => {
            if let Some(detection) = detection
                .as_ref()
                .filter(|result| !result.schema.edition.is_generic())
            {
                return SchemaResolutionOutcome {
                    schema: detection.schema.clone(),
                    confidence: detection.confidence,
                    source: "auto-detect",
                    evidence: detection.evidence.clone(),
                };
            }

            if let Some(schema) = custom_schema {
                let confidence = schema.confidence;
                return SchemaResolutionOutcome {
                    schema,
                    confidence,
                    source: "custom-fallback",
                    evidence: vec!["fallback:custom".into()],
                };
            }

            let detection = detection.unwrap_or_else(|| DetectionResult {
                edition: "generic".into(),
                confidence: 0,
                evidence: vec![],
                schema: SageSchema::for_edition(&SageEdition::Generic),
            });
            SchemaResolutionOutcome {
                schema: detection.schema.clone(),
                confidence: detection.confidence,
                source: "generic-fallback",
                evidence: detection.evidence,
            }
        }
    }
}

async fn resolve_schema_for_connection(
    connection_id: &str,
    connection: &ConnectionConfig,
) -> SchemaResolutionOutcome {
    let detection = if connection.sage_edition == "auto" {
        match db::connect(connection).await {
            Ok(mut client) => match detect_sage_schema(&mut client).await {
                Ok(result) => Some(result),
                Err(error) => {
                    eprintln!(
                        "[sage] schema_detect_error connection_id={} profile=\"{}\" error={}",
                        connection_id,
                        connection_profile_summary(connection),
                        error,
                    );
                    None
                }
            },
            Err(error) => {
                eprintln!(
                    "[sage] schema_probe_connect_error connection_id={} profile=\"{}\" error={}",
                    connection_id,
                    connection_profile_summary(connection),
                    error,
                );
                None
            }
        }
    } else {
        None
    };

    let outcome = resolve_schema_from_selection(
        &connection.sage_edition,
        connection.custom_schema.clone(),
        detection,
    );

    eprintln!(
        "[sage] schema_resolved connection_id={} profile=\"{}\" requested={} resolved={} source={} confidence={} evidence={:?}",
        connection_id,
        connection_profile_summary(connection),
        connection.sage_edition,
        outcome.schema.edition.as_str(),
        outcome.source,
        outcome.confidence,
        outcome.evidence,
    );

    outcome
}

async fn probe_tables(
    client: &mut db::DbClient,
    candidates: &[&str],
) -> Result<Vec<String>, String> {
    let quoted: Vec<String> = candidates.iter().map(|t| format!("'{}'", t)).collect();
    let sql = format!(
        "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES \
         WHERE TABLE_TYPE = 'BASE TABLE' AND TABLE_NAME IN ({})",
        quoted.join(", ")
    );
    let data = db::execute_raw_query(client, &sql).await?;
    let found: Vec<String> = data
        .rows
        .iter()
        .filter_map(|row| {
            row.first().and_then(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
        })
        .collect();
    Ok(found)
}

async fn probe_column(
    client: &mut db::DbClient,
    table_name: &str,
    column_name: &str,
) -> Result<bool, String> {
    let sql = format!(
        "SELECT 1 FROM INFORMATION_SCHEMA.COLUMNS \
         WHERE TABLE_NAME = '{}' AND COLUMN_NAME = '{}'",
        table_name.replace('\'', "''"),
        column_name.replace('\'', "''")
    );
    let data = db::execute_raw_query(client, &sql).await?;
    Ok(!data.rows.is_empty())
}

async fn detect_sage_schema(client: &mut db::DbClient) -> Result<DetectionResult, String> {
    let sage100_candidates = ["F_ECRITUREC", "F_COMPTET", "F_TIERS", "F_JOURNAL"];
    let found100 = probe_tables(client, &sage100_candidates).await?;

    let sagex3_candidates = ["GACCENTRY", "GACCOUNT", "BPARTNER", "GJOURNAL"];
    let found_x3 = probe_tables(client, &sagex3_candidates).await?;
    let sage1000_candidates = [
        "TECRITURE",
        "TCOMPTEGENERAL",
        "TTIERS",
        "TROLETIERS",
        "TPIECE",
    ];
    let found1000 = probe_tables(client, &sage1000_candidates).await?;
    let has_tecriture_edate = found1000
        .iter()
        .any(|name| name.eq_ignore_ascii_case("TECRITURE"))
        && probe_column(client, "TECRITURE", "eDate").await?;

    Ok(detect_sage_schema_from_probes(
        found100,
        found_x3,
        found1000,
        has_tecriture_edate,
    ))
}

fn store_schema_for_connection(
    state: &State<'_, AppState>,
    id: &str,
    schema: SageSchema,
) -> Result<(), String> {
    let mut schemas = state.active_schemas.lock().map_err(|e| e.to_string())?;
    schemas.insert(id.to_string(), schema);
    Ok(())
}

#[tauri::command]
pub async fn test_mapping(
    connection: ConnectionConfig,
    schema: SageSchema,
) -> Result<TableData, String> {
    let mut client = db::connect(&connection).await?;

    if schema.table_ecritures.is_empty() {
        return Err("Table des écritures non spécifiée".into());
    }

    let wrap = |col: &str| {
        if col.trim_start().to_uppercase().starts_with("CASE") {
            col.to_string()
        } else {
            format!("[{}]", col.replace(']', "]]"))
        }
    };

    let mut select_parts = Vec::new();
    if !schema.col_date.is_empty() {
        select_parts.push(format!("{} AS [Date]", wrap(&schema.col_date)));
    }
    if !schema.col_journal.is_empty() {
        select_parts.push(format!("{} AS [Journal]", wrap(&schema.col_journal)));
    }
    if !schema.col_compte.is_empty() {
        select_parts.push(format!("{} AS [Compte]", wrap(&schema.col_compte)));
    }
    if !schema.col_libelle.is_empty() {
        select_parts.push(format!("{} AS [Libelle]", wrap(&schema.col_libelle)));
    }
    if !schema.col_debit.is_empty() {
        select_parts.push(format!("{} AS [Debit]", wrap(&schema.col_debit)));
    }
    if !schema.col_credit.is_empty() {
        select_parts.push(format!("{} AS [Credit]", wrap(&schema.col_credit)));
    }

    let select_clause = if select_parts.is_empty() {
        "*".to_string()
    } else {
        select_parts.join(", ")
    };

    let sql = format!(
        "SELECT TOP 5 {} FROM [{}]",
        select_clause,
        schema.table_ecritures.replace(']', "]]")
    );

    db::execute_raw_query(&mut client, &sql).await
}

/// Auto-detect the Sage edition by probing INFORMATION_SCHEMA
#[tauri::command]
pub async fn detect_sage_edition(
    state: State<'_, AppState>,
    connection_id: String,
) -> Result<DetectionResult, String> {
    let config = load_connection_config(&state, &connection_id)?;
    let mut client = db::connect(&config).await?;
    let result = detect_sage_schema(&mut client).await?;
    store_schema_for_connection(&state, &connection_id, result.schema.clone())?;
    eprintln!(
        "[sage] schema_detected connection_id={} profile=\"{}\" detected={} confidence={} evidence={:?}",
        connection_id,
        connection_profile_summary(&config),
        result.schema.edition.as_str(),
        result.confidence,
        result.evidence,
    );
    Ok(result)
}

/// Get the active resolved schema for a connection
#[tauri::command]
pub fn get_active_schema(
    state: State<AppState>,
    connection_id: String,
) -> Result<Option<SageSchema>, String> {
    let schemas = state.active_schemas.lock().map_err(|e| e.to_string())?;
    Ok(schemas.get(&connection_id).cloned())
}

/// Get all active schemas as a map of connection_id → edition string
#[tauri::command]
pub fn get_active_schemas(state: State<AppState>) -> Result<HashMap<String, SageSchema>, String> {
    let schemas = state.active_schemas.lock().map_err(|e| e.to_string())?;
    Ok(schemas.clone())
}

/// Connect to a saved database
#[tauri::command]
pub async fn connect_db(state: State<'_, AppState>, id: String) -> Result<String, String> {
    // Get the config with real password
    let conn_config = load_connection_config(&state, &id)?;

    // Mark as connecting
    {
        let mut active = state.active_connections.lock().map_err(|e| e.to_string())?;
        active.insert(
            id.clone(),
            ActiveConnection {
                config: conn_config.clone(),
                status: ConnectionStatus::Connecting,
            },
        );
    }

    // Try to connect
    match db::test_connection(&conn_config).await {
        Ok(msg) => {
            {
                let mut active = state.active_connections.lock().map_err(|e| e.to_string())?;
                if let Some(ac) = active.get_mut(&id) {
                    ac.status = ConnectionStatus::Connected;
                }
            }

            let schema = resolve_schema_for_connection(&id, &conn_config)
                .await
                .schema;
            {
                let mut schemas = state.active_schemas.lock().map_err(|e| e.to_string())?;
                schemas.insert(id.clone(), schema);
            }

            Ok(msg)
        }
        Err(e) => {
            let mut active = state.active_connections.lock().map_err(|e| e.to_string())?;
            if let Some(ac) = active.get_mut(&id) {
                ac.status = ConnectionStatus::Error(e.clone());
            }
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn get_databases(
    state: State<'_, AppState>,
    connection_id: String,
) -> Result<Vec<String>, String> {
    let conn_config = load_connection_config(&state, &connection_id)?;
    let mut client = db::connect(&conn_config).await?;
    db::get_databases(&mut client).await
}

#[tauri::command]
pub async fn switch_database(
    state: State<'_, AppState>,
    connection_id: String,
    database: String,
) -> Result<(), String> {
    let next_database = database.trim();
    if next_database.is_empty() {
        return Err("Database name is required".to_string());
    }

    let current_config = load_connection_config(&state, &connection_id)?;
    let previous_active = {
        let active = state.active_connections.lock().map_err(|e| e.to_string())?;
        active.get(&connection_id).cloned()
    };

    let mut next_config = current_config.clone();
    next_config.database = next_database.to_string();

    {
        let mut active = state.active_connections.lock().map_err(|e| e.to_string())?;
        active.insert(
            connection_id.clone(),
            ActiveConnection {
                config: next_config.clone(),
                status: ConnectionStatus::Connecting,
            },
        );
    }
    state.invalidate_client_cache(&connection_id)?;

    let switch_result = db::test_connection(&next_config).await;

    match switch_result {
        Ok(_) => {
            {
                let mut active = state.active_connections.lock().map_err(|e| e.to_string())?;
                active.insert(
                    connection_id.clone(),
                    ActiveConnection {
                        config: next_config.clone(),
                        status: ConnectionStatus::Connected,
                    },
                );
            }
            let resolved_schema = resolve_schema_for_connection(&connection_id, &next_config).await;
            let resolved_edition = resolved_schema.schema.edition.as_str().to_string();
            let resolved_confidence = resolved_schema.confidence;
            store_schema_for_connection(&state, &connection_id, resolved_schema.schema)?;
            eprintln!(
                "[sage] database_switched connection_id={} from_database={} to_database={} resolved={} confidence={}",
                connection_id,
                current_config.database,
                next_database,
                resolved_edition,
                resolved_confidence,
            );
            Ok(())
        }
        Err(error) => {
            let mut active = state.active_connections.lock().map_err(|e| e.to_string())?;
            let fallback_config = previous_active
                .as_ref()
                .map(|connection| connection.config.clone())
                .unwrap_or(current_config);

            active.insert(
                connection_id,
                ActiveConnection {
                    config: fallback_config,
                    status: ConnectionStatus::Error(error.clone()),
                },
            );
            Err(error)
        }
    }
}

/// Disconnect from a database
#[tauri::command]
pub fn disconnect_db(state: State<AppState>, id: String) -> Result<(), String> {
    let mut active = state.active_connections.lock().map_err(|e| e.to_string())?;
    active.remove(&id);
    drop(active);
    let mut schemas = state.active_schemas.lock().map_err(|e| e.to_string())?;
    schemas.remove(&id);
    drop(schemas);
    state.invalidate_client_cache(&id)
}

/// Get connection status map
#[tauri::command]
pub fn get_connection_statuses(
    state: State<AppState>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let active = state.active_connections.lock().map_err(|e| e.to_string())?;
    let statuses = active
        .iter()
        .map(|(id, ac)| {
            let status = match &ac.status {
                ConnectionStatus::Disconnected => "disconnected",
                ConnectionStatus::Connecting => "connecting",
                ConnectionStatus::Connected => "connected",
                ConnectionStatus::Error(_) => "error",
            };
            (id.clone(), status.to_string())
        })
        .collect();
    Ok(statuses)
}

// ─── Schema Exploration ───────────────────────────────────────────────────────

/// Get all tables in a connected database
#[tauri::command]
pub async fn get_tables(state: State<'_, AppState>, id: String) -> Result<Vec<TableInfo>, String> {
    let conn_config = load_connection_config(&state, &id)?;

    let mut client = db::connect(&conn_config).await?;
    db::get_tables(&mut client).await
}

/// Get columns for a table
#[tauri::command]
pub async fn get_columns(
    state: State<'_, AppState>,
    id: String,
    schema: String,
    table: String,
) -> Result<Vec<ColumnInfo>, String> {
    let conn_config = load_connection_config(&state, &id)?;

    let mut client = db::connect(&conn_config).await?;
    db::get_columns(&mut client, &schema, &table).await
}

/// Get all foreign-key relationships in a connected database
#[tauri::command]
pub async fn get_relationships(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<RelationshipInfo>, String> {
    let conn_config = load_connection_config(&state, &id)?;

    let mut client = db::connect(&conn_config).await?;
    db::get_relationships(&mut client).await
}

// ─── Data ─────────────────────────────────────────────────────────────────────

/// Fetch paginated data from a table
#[tauri::command]
pub async fn get_table_data(
    state: State<'_, AppState>,
    id: String,
    schema: String,
    table: String,
    page: u32,
    page_size: u32,
    filters: Vec<Filter>,
) -> Result<TableData, String> {
    let conn_config = load_connection_config(&state, &id)?;

    let mut client = db::connect(&conn_config).await?;
    let columns = db::get_columns(&mut client, &schema, &table).await?;
    db::get_table_data(
        &mut client,
        &schema,
        &table,
        page,
        page_size,
        &filters,
        &columns,
    )
    .await
}

/// Execute a raw SQL query
#[tauri::command]
pub async fn execute_query(
    state: State<'_, AppState>,
    id: String,
    sql: String,
) -> Result<TableData, String> {
    let conn_config = load_connection_config(&state, &id)?;

    let mut client = db::connect(&conn_config).await?;
    match db::execute_raw_query(&mut client, &sql).await {
        Ok(data) => {
            let _ = append_query_history(
                &state,
                QueryHistoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    connection_id: id,
                    connection_name: conn_config.name.clone(),
                    sql,
                    ran_at: chrono::Utc::now().to_rfc3339(),
                    ok: true,
                    row_count: Some(data.total_count),
                    error: None,
                },
            );
            Ok(data)
        }
        Err(error) => {
            let _ = append_query_history(
                &state,
                QueryHistoryEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    connection_id: id,
                    connection_name: conn_config.name.clone(),
                    sql,
                    ran_at: chrono::Utc::now().to_rfc3339(),
                    ok: false,
                    row_count: None,
                    error: Some(error.clone()),
                },
            );
            Err(error)
        }
    }
}

// ─── Query Library ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_saved_queries(
    state: State<AppState>,
    connection_id: Option<String>,
) -> Result<Vec<SavedQuery>, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    let mut queries: Vec<SavedQuery> = config
        .saved_queries
        .iter()
        .filter(|q| {
            connection_id
                .as_ref()
                .map(|id| q.connection_id == *id)
                .unwrap_or(true)
        })
        .cloned()
        .collect();
    queries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(queries)
}

#[tauri::command]
pub fn save_saved_query(state: State<AppState>, query: SavedQuery) -> Result<SavedQuery, String> {
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();

    let saved = if let Some(existing) = config.saved_queries.iter_mut().find(|q| q.id == query.id) {
        existing.connection_id = query.connection_id.clone();
        existing.name = query.name.clone();
        existing.sql = query.sql.clone();
        existing.updated_at = now;
        existing.clone()
    } else {
        let mut new_query = query;
        if new_query.id.is_empty() {
            new_query.id = uuid::Uuid::new_v4().to_string();
        }
        new_query.updated_at = now;
        config.saved_queries.push(new_query.clone());
        new_query
    };

    drop(config);
    state.save_config()?;
    Ok(saved)
}

#[tauri::command]
pub fn delete_saved_query(state: State<AppState>, id: String) -> Result<(), String> {
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    config.saved_queries.retain(|q| q.id != id);
    drop(config);
    state.save_config()
}

#[tauri::command]
pub fn list_query_history(
    state: State<AppState>,
    connection_id: Option<String>,
) -> Result<Vec<QueryHistoryEntry>, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?;
    let mut history: Vec<QueryHistoryEntry> = config
        .query_history
        .iter()
        .filter(|q| {
            connection_id
                .as_ref()
                .map(|id| q.connection_id == *id)
                .unwrap_or(true)
        })
        .cloned()
        .collect();
    history.sort_by(|a, b| b.ran_at.cmp(&a.ran_at));
    Ok(history)
}

#[tauri::command]
pub fn clear_query_history(
    state: State<AppState>,
    connection_id: Option<String>,
) -> Result<(), String> {
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    match connection_id {
        Some(id) => config.query_history.retain(|q| q.connection_id != id),
        None => config.query_history.clear(),
    }
    drop(config);
    state.save_config()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_sage1000_from_probe_fixture() {
        let result = detect_sage_schema_from_probes(
            vec![],
            vec![],
            vec![
                "TECRITURE".into(),
                "TCOMPTEGENERAL".into(),
                "TTIERS".into(),
                "TROLETIERS".into(),
                "TPIECE".into(),
            ],
            true,
        );

        assert_eq!(result.edition, "sage1000");
        assert_eq!(result.schema.edition, SageEdition::Sage1000);
        assert_eq!(result.confidence, 99);
        assert!(result.evidence.iter().any(|item| item == "TECRITURE.eDate"));
    }

    #[test]
    fn detect_sage100_from_probe_fixture() {
        let result = detect_sage_schema_from_probes(
            vec!["F_ECRITUREC".into(), "F_COMPTET".into(), "F_TIERS".into()],
            vec![],
            vec![],
            false,
        );

        assert_eq!(result.edition, "sage100");
        assert_eq!(result.schema.edition, SageEdition::Sage100);
        assert_eq!(result.confidence, 95);
    }

    #[test]
    fn auto_selection_uses_detected_sage1000_schema() {
        let detection = DetectionResult {
            edition: "sage1000".into(),
            confidence: 99,
            evidence: vec!["TECRITURE".into(), "TCOMPTEGENERAL".into()],
            schema: SageSchema::for_edition(&SageEdition::Sage1000),
        };

        let resolved = resolve_schema_from_selection("auto", None, Some(detection));

        assert_eq!(resolved.schema.edition, SageEdition::Sage1000);
        assert_eq!(resolved.source, "auto-detect");
        assert_eq!(resolved.confidence, 99);
    }

    #[test]
    fn manual_sage1000_override_wins_over_detection() {
        let detection = DetectionResult {
            edition: "sage100".into(),
            confidence: 95,
            evidence: vec!["F_ECRITUREC".into()],
            schema: SageSchema::for_edition(&SageEdition::Sage100),
        };

        let resolved = resolve_schema_from_selection("sage1000", None, Some(detection));

        assert_eq!(resolved.schema.edition, SageEdition::Sage1000);
        assert_eq!(resolved.source, "manual");
        assert!(resolved
            .evidence
            .iter()
            .any(|item| item == "manual:sage1000"));
    }

    #[test]
    fn auto_selection_uses_custom_mapping_when_probe_is_generic() {
        let mut custom = SageSchema::for_edition(&SageEdition::Generic);
        custom.table_ecritures = "CUSTOM_ECRITURES".into();
        custom.table_comptes = "CUSTOM_COMPTES".into();
        custom.table_tiers = "CUSTOM_TIERS".into();
        custom.table_journaux = "CUSTOM_JOURNAUX".into();
        custom.confidence = 61;

        let detection = DetectionResult {
            edition: "generic".into(),
            confidence: 0,
            evidence: vec!["sage1000:TECRITURE".into()],
            schema: SageSchema::for_edition(&SageEdition::Generic),
        };

        let resolved = resolve_schema_from_selection("auto", Some(custom.clone()), Some(detection));

        assert_eq!(resolved.schema.table_ecritures, custom.table_ecritures);
        assert_eq!(resolved.source, "custom-fallback");
        assert!(resolved
            .evidence
            .iter()
            .any(|item| item == "fallback:custom"));
    }

    #[test]
    fn auto_selection_refreshes_when_detection_changes_after_database_switch() {
        let sage100 = DetectionResult {
            edition: "sage100".into(),
            confidence: 95,
            evidence: vec!["F_ECRITUREC".into()],
            schema: SageSchema::for_edition(&SageEdition::Sage100),
        };
        let sage1000 = DetectionResult {
            edition: "sage1000".into(),
            confidence: 99,
            evidence: vec!["TECRITURE".into()],
            schema: SageSchema::for_edition(&SageEdition::Sage1000),
        };

        let initial = resolve_schema_from_selection("auto", None, Some(sage100));
        let after_switch = resolve_schema_from_selection("auto", None, Some(sage1000));

        assert_eq!(initial.schema.edition, SageEdition::Sage100);
        assert_eq!(after_switch.schema.edition, SageEdition::Sage1000);
        assert_eq!(after_switch.source, "auto-detect");
    }
}

// ─── Export ───────────────────────────────────────────────────────────────────

/// Export table data to CSV — returns file path written
#[tauri::command]
pub async fn export_csv(
    state: State<'_, AppState>,
    id: String,
    schema: String,
    table: String,
    filters: Vec<Filter>,
    selected_columns: Vec<String>,
    file_path: String,
) -> Result<String, String> {
    let conn_config = load_connection_config(&state, &id)?;

    let mut client = db::connect(&conn_config).await?;
    let csv = db::export_csv(&mut client, &schema, &table, &filters, &selected_columns).await?;

    std::fs::write(&file_path, csv).map_err(|e| format!("Failed to write file: {}", e))?;
    Ok(file_path)
}

/// Export table data to JSON — returns file path written
#[tauri::command]
pub async fn export_json(
    state: State<'_, AppState>,
    id: String,
    schema: String,
    table: String,
    filters: Vec<Filter>,
    selected_columns: Vec<String>,
    file_path: String,
) -> Result<String, String> {
    let conn_config = load_connection_config(&state, &id)?;

    let mut client = db::connect(&conn_config).await?;
    let json = db::export_json(&mut client, &schema, &table, &filters, &selected_columns).await?;

    std::fs::write(&file_path, json).map_err(|e| format!("Failed to write file: {}", e))?;
    Ok(file_path)
}

/// Export table as SQL INSERT statements
#[tauri::command]
pub async fn export_sql(
    state: State<'_, AppState>,
    id: String,
    schema: String,
    table: String,
    filters: Vec<Filter>,
    file_path: String,
) -> Result<String, String> {
    let conn_config = load_connection_config(&state, &id)?;

    let mut client = db::connect(&conn_config).await?;
    let columns = db::get_columns(&mut client, &schema, &table).await?;
    let data =
        db::get_table_data(&mut client, &schema, &table, 0, 50_000, &filters, &columns).await?;

    let full_table = format!("[{}].[{}]", schema, table);
    let col_names: Vec<String> = data
        .columns
        .iter()
        .map(|c| format!("[{}]", c.name))
        .collect();

    let mut sql_output = format!(
        "-- Export of {} ({})\n-- Generated by Sage Data Bridge\n-- {} rows\n\n",
        full_table,
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        data.total_count
    );

    for row in &data.rows {
        let values: Vec<String> = row
            .iter()
            .map(|v| match v {
                Value::Null => "NULL".to_string(),
                Value::Bool(b) => {
                    if *b {
                        "1".to_string()
                    } else {
                        "0".to_string()
                    }
                }
                Value::Number(n) => n.to_string(),
                Value::String(s) => format!("N'{}'", s.replace("'", "''")),
                _ => format!("N'{}'", v.to_string().replace("'", "''")),
            })
            .collect();

        sql_output.push_str(&format!(
            "INSERT INTO {} ({}) VALUES ({});\n",
            full_table,
            col_names.join(", "),
            values.join(", ")
        ));
    }

    std::fs::write(&file_path, sql_output).map_err(|e| format!("Failed to write file: {}", e))?;
    Ok(file_path)
}
