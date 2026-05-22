use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn default_sage_edition() -> String {
    "auto".to_string()
}

fn default_active_template_id() -> String {
    "builtin-modern".to_string()
}

/// A saved database connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default)]
    pub instance_name: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
    pub use_windows_auth: bool,
    pub trust_cert: bool,
    pub encrypt: bool,
    #[serde(default = "default_sage_edition")]
    pub sage_edition: String,
    #[serde(default)]
    pub custom_schema: Option<crate::sage_compat::SageSchema>,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            host: String::from("localhost"),
            instance_name: String::new(),
            port: 1433,
            database: String::new(),
            username: String::new(),
            password: String::new(),
            use_windows_auth: false,
            trust_cert: true,
            encrypt: false,
            sage_edition: "auto".to_string(),
            custom_schema: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceTemplate {
    pub id: String,
    #[serde(default)]
    pub connection_id: String,
    pub name: String,
    pub primary_color: String,
    pub secondary_color: String,
    pub font_family: String,
    #[serde(default = "default_font_size_px")]
    pub font_size_px: u8,
    pub show_logo: bool,
    #[serde(default)]
    pub logo_base64: String,
    #[serde(default)]
    pub company_name: String,
    #[serde(default)]
    pub company_address: String,
    #[serde(default)]
    pub company_siret: String,
    #[serde(default)]
    pub company_tva: String,
    #[serde(default)]
    pub company_phone: String,
    #[serde(default)]
    pub company_email: String,
    #[serde(default)]
    pub company_website: String,
    #[serde(default)]
    pub footer_text: String,
    #[serde(default)]
    pub legal_mentions: String,
    #[serde(default)]
    pub show_bank_details: bool,
    #[serde(default)]
    pub bank_iban: String,
    #[serde(default)]
    pub bank_bic: String,
    #[serde(default)]
    pub conditions_paiement: String,
    #[serde(default)]
    pub mention_tva: String,
    pub layout: String,
}

fn default_font_size_px() -> u8 {
    10
}

/// Info about a database table
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub full_name: String,
    pub row_count: Option<i64>,
}

/// Info about a table column
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_primary_key: bool,
    pub ordinal: i32,
    pub max_length: Option<i32>,
    pub precision: Option<i32>,
    pub scale: Option<i32>,
}

/// A foreign-key relationship between two tables
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipInfo {
    pub constraint_name: String,
    pub source_schema: String,
    pub source_table: String,
    pub source_column: String,
    pub target_schema: String,
    pub target_table: String,
    pub target_column: String,
    pub ordinal: i32,
}

/// A filter condition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    pub column: String,
    pub operator: String, // =, !=, >, <, >=, <=, LIKE, NOT LIKE, IS NULL, IS NOT NULL, BETWEEN, IN
    pub value: Option<String>,
    pub value2: Option<String>,    // for BETWEEN
    pub data_type: Option<String>, // hint for value quoting
}

/// Result of a table data query
#[derive(Debug, Serialize, Deserialize)]
pub struct TableData {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub total_count: i64,
    pub page: u32,
    pub page_size: u32,
}

/// A user-saved SQL query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedQuery {
    pub id: String,
    pub connection_id: String,
    pub name: String,
    pub sql: String,
    pub updated_at: String,
}

impl Default for SavedQuery {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            connection_id: String::new(),
            name: String::new(),
            sql: String::new(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// A query execution log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryHistoryEntry {
    pub id: String,
    pub connection_id: String,
    pub connection_name: String,
    pub sql: String,
    pub ran_at: String,
    pub ok: bool,
    pub row_count: Option<i64>,
    pub error: Option<String>,
}

impl Default for QueryHistoryEntry {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            connection_id: String::new(),
            connection_name: String::new(),
            sql: String::new(),
            ran_at: chrono::Utc::now().to_rfc3339(),
            ok: true,
            row_count: None,
            error: None,
        }
    }
}

/// App config stored on disk
#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub password_hash: Option<String>,
    pub admin_password_hash: Option<String>,
    pub connections: Vec<ConnectionConfig>,
    pub saved_queries: Vec<SavedQuery>,
    pub query_history: Vec<QueryHistoryEntry>,
    pub input_history: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub invoice_templates: Vec<InvoiceTemplate>,
    #[serde(default = "default_active_template_id")]
    pub active_template_id: String,
}

/// Connection status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

/// Runtime state for a connection
#[derive(Debug, Clone)]
pub struct ActiveConnection {
    pub config: ConnectionConfig,
    pub status: ConnectionStatus,
}

/// Global application state
pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub active_connections: Mutex<HashMap<String, ActiveConnection>>,
    pub active_schemas: Mutex<HashMap<String, crate::sage_compat::SageSchema>>,
    pub client_cache: Mutex<HashMap<String, Arc<crate::db::CachedClient>>>,
    pub data_dir: PathBuf,
    pub config_path: PathBuf,
}

impl AppState {
    pub fn new() -> Self {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("SageDataBridge");

        let config_path = data_dir.join("config.json");

        // Load config from disk if it exists
        let config = if config_path.exists() {
            std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str::<AppConfig>(&s).ok())
                .unwrap_or_default()
        } else {
            AppConfig::default()
        };

        Self {
            config: Mutex::new(config),
            active_connections: Mutex::new(HashMap::new()),
            active_schemas: Mutex::new(HashMap::new()),
            client_cache: Mutex::new(HashMap::new()),
            data_dir,
            config_path,
        }
    }

    pub fn save_config(&self) -> Result<(), String> {
        let config = self.config.lock().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&self.data_dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(&*config).map_err(|e| e.to_string())?;
        std::fs::write(&self.config_path, json).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn resolve_connection_config(&self, id: &str) -> Result<ConnectionConfig, String> {
        {
            let active = self.active_connections.lock().map_err(|e| e.to_string())?;
            if let Some(connection) = active.get(id) {
                return Ok(connection.config.clone());
            }
        }

        let config = self.config.lock().map_err(|e| e.to_string())?;
        config
            .connections
            .iter()
            .find(|connection| connection.id == id)
            .cloned()
            .ok_or_else(|| format!("Connection '{}' not found", id))
    }

    pub fn get_active_connection(&self, id: &str) -> Result<ConnectionConfig, String> {
        self.resolve_connection_config(id)
    }

    pub fn invalidate_client_cache(&self, id: &str) -> Result<(), String> {
        let mut cache = self.client_cache.lock().map_err(|e| e.to_string())?;
        cache.remove(id);
        Ok(())
    }

    pub fn get_connection_schema(&self, id: &str) -> Option<crate::sage_compat::SageSchema> {
        let schemas = self.active_schemas.lock().ok()?;
        schemas.get(id).cloned()
    }
}
