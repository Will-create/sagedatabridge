use serde_json::{json, Value};
use tiberius::{AuthMethod, Client, ColumnType, Config, SqlBrowser};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

#[cfg(windows)]
use odbc_api::{
    buffers::TextRowSet,
    environment,
    ConnectionOptions,
    Cursor,
    ResultSetMetadata,
};

use crate::state::{ColumnInfo, ConnectionConfig, Filter, RelationshipInfo, TableData, TableInfo};

pub type TdsClient = Client<Compat<TcpStream>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbBackend {
    Tiberius,
    #[cfg(windows)]
    Odbc,
}

#[derive(Debug, Clone)]
pub struct DbClient {
    pub config: ConnectionConfig,
    pub backend: DbBackend,
}

/// A cached database connection entry (held in AppState.client_cache)
pub struct CachedClient {
    pub client: tokio::sync::Mutex<DbClient>,
    pub connected_at: std::time::Instant,
}

impl DbClient {
    /// Lightweight ping: creates a temp TDS connection to verify server is reachable.
    pub async fn ping(&self) -> bool {
        match self.backend {
            #[cfg(windows)]
            DbBackend::Odbc => query_odbc(&self.config, "SELECT 1").is_ok(),
            DbBackend::Tiberius => {
                let Ok(mut tds) = connect_tds(&self.config).await else { return false; };
                let Ok(stream) = tds.simple_query("SELECT 1").await else { return false; };
                stream.into_results().await.is_ok()
            }
        }
    }
}

/// Build a tiberius Config from our ConnectionConfig
fn build_config(conn: &ConnectionConfig) -> Result<Config, String> {
    let mut config = Config::new();
    let raw_host = conn.host.trim();
    let raw_instance = conn.instance_name.trim();

    let (host, instance) = if !raw_instance.is_empty() {
        (raw_host.to_string(), Some(raw_instance.to_string()))
    } else if raw_host.contains('\\') {
        let mut parts = raw_host.splitn(2, '\\');
        let h = parts.next().unwrap_or("localhost");
        let h = if h == "." { "localhost" } else { h };
        let inst = parts.next().unwrap_or("");
        (h.to_string(), Some(inst.to_string()))
    } else if raw_host.eq_ignore_ascii_case("SQLEXPRESS") {
        ("localhost".to_string(), Some("SQLEXPRESS".to_string()))
    } else if raw_host == "." {
        ("localhost".to_string(), None)
    } else {
        (raw_host.to_string(), None)
    };

    config.host(&host);
    if let Some(inst) = instance.filter(|s| !s.is_empty()) {
        config.instance_name(&inst);
    } else {
        config.port(conn.port);
    }
    if !conn.database.trim().is_empty() {
        config.database(&conn.database);
    }

    if conn.use_windows_auth {
        #[cfg(windows)]
        config.authentication(AuthMethod::Integrated);
        #[cfg(not(windows))]
        return Err("Windows authentication is only supported on Windows".to_string());
    } else {
        config.authentication(AuthMethod::sql_server(&conn.username, &conn.password));
    }

    if conn.trust_cert {
        config.trust_cert();
    }

    if conn.encrypt {
        config.encryption(tiberius::EncryptionLevel::Required);
    } else {
        config.encryption(tiberius::EncryptionLevel::NotSupported);
    }

    Ok(config)
}

async fn connect_tds(conn: &ConnectionConfig) -> Result<TdsClient, String> {
    let config = build_config(conn)?;

    let tcp = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        TcpStream::connect_named(&config),
    )
    .await
    .map_err(|_| "Connexion timeout après 10 secondes — vérifiez le réseau".to_string())?
    .map_err(|e| format!("Connexion TCP échouée: {}", e))?;

    tcp.set_nodelay(true)
        .map_err(|e| format!("Failed to set TCP_NODELAY: {}", e))?;

    let client = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        Client::connect(config, tcp.compat_write()),
    )
    .await
    .map_err(|_| "Timeout d'authentification SQL Server".to_string())?
    .map_err(|e| format!("SQL Server login failed: {}", e))?;

    Ok(client)
}

async fn test_connection_tds(conn: &ConnectionConfig) -> Result<String, String> {
    let mut client = connect_tds(conn).await?;

    let stream = client
        .simple_query("SELECT @@VERSION AS version, @@SERVERNAME AS server_name")
        .await
        .map_err(|e| format!("Test query failed: {}", e))?;

    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| format!("Failed to read results: {}", e))?;

    if let Some(row) = rows.first() {
        let version: Option<&str> = row.get(0);
        let server: Option<&str> = row.get(1);
        Ok(format!(
            "Connected to: {} ({})",
            server.unwrap_or("unknown"),
            version.unwrap_or("unknown").lines().next().unwrap_or("")
        ))
    } else {
        Ok("Connected successfully".to_string())
    }
}

#[cfg(windows)]
const ODBC_SQL_SERVER_DRIVERS: &[&str] = &[
    "ODBC Driver 18 for SQL Server",
    "ODBC Driver 17 for SQL Server",
    "SQL Server Native Client 11.0",
    "SQL Server",
];

#[cfg(windows)]
const ODBC_BATCH_SIZE: usize = 256;

#[cfg(windows)]
struct OdbcQueryResult {
    columns: Vec<String>,
    rows: Vec<Vec<Option<String>>>,
}

#[cfg(windows)]
fn odbc_escape(value: &str) -> String {
    format!("{{{}}}", value.replace('}', "}}"))
}

#[cfg(windows)]
fn decode_windows_1252(bytes: &[u8]) -> String {
    const EXTENDED: [char; 32] = [
        '\u{20AC}', '\u{0081}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{008D}',
        '\u{017D}', '\u{008F}', '\u{0090}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}',
        '\u{2022}', '\u{2013}', '\u{2014}', '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}',
        '\u{0153}', '\u{009D}', '\u{017E}', '\u{0178}',
    ];

    bytes
        .iter()
        .map(|byte| match byte {
            0x80..=0x9F => EXTENDED[(byte - 0x80) as usize],
            _ => char::from_u32(*byte as u32).unwrap_or('\u{FFFD}'),
        })
        .collect()
}

#[cfg(windows)]
fn decode_odbc_text(bytes: &[u8]) -> String {
    std::str::from_utf8(bytes)
        .map(|text| text.to_string())
        .unwrap_or_else(|_| decode_windows_1252(bytes))
}

#[cfg(windows)]
fn odbc_server(conn: &ConnectionConfig) -> String {
    let raw_host = conn.host.trim();
    let raw_instance = conn.instance_name.trim();

    if !raw_instance.is_empty() {
        return format!("{}\\{}", raw_host, raw_instance);
    }

    if raw_host.eq_ignore_ascii_case("SQLEXPRESS") {
        return ".\\SQLEXPRESS".to_string();
    }

    if raw_host.contains('\\') {
        return raw_host.to_string();
    }

    let host = if raw_host.is_empty() || raw_host == "." || raw_host.eq_ignore_ascii_case("(local)") {
        "localhost"
    } else {
        raw_host
    };

    format!("{},{}", host, conn.port)
}

#[cfg(windows)]
fn open_odbc_connection(conn: &ConnectionConfig) -> Result<odbc_api::Connection<'static>, String> {
    let env = environment().map_err(|e| format!("Failed to initialize ODBC: {}", e))?;
    let server = odbc_server(conn);
    let mut errors = Vec::new();

    for driver in ODBC_SQL_SERVER_DRIVERS {
        let options = ConnectionOptions {
            login_timeout_sec: Some(10),
            packet_size: None,
        };
        let mut connection_string = format!(
            "Driver={{{}}};Server={};App={};Encrypt={};TrustServerCertificate={};",
            driver,
            odbc_escape(&server),
            odbc_escape("Sage Data Bridge"),
            if conn.encrypt { "Yes" } else { "No" },
            if conn.trust_cert { "Yes" } else { "No" },
        );

        if !conn.database.trim().is_empty() {
            connection_string.push_str(&format!("Database={};", odbc_escape(&conn.database)));
        }

        if conn.use_windows_auth {
            connection_string.push_str("Trusted_Connection=Yes;");
        } else {
            connection_string.push_str(&format!(
                "UID={};PWD={};",
                odbc_escape(&conn.username),
                odbc_escape(&conn.password)
            ));
        }

        match env.connect_with_connection_string(&connection_string, options) {
            Ok(connection) => return Ok(connection),
            Err(error) => errors.push(format!("{}: {}", driver, error)),
        }
    }

    Err(format!(
        "ODBC connection failed: {}",
        errors.join(" | ")
    ))
}

#[cfg(windows)]
fn query_odbc(conn: &ConnectionConfig, sql: &str) -> Result<OdbcQueryResult, String> {
    let connection = open_odbc_connection(conn)?;
    let maybe_cursor = connection
        .execute(sql, (), None)
        .map_err(|e| format!("Query failed: {}", e))?;

    let Some(mut cursor) = maybe_cursor else {
        return Ok(OdbcQueryResult {
            columns: vec![],
            rows: vec![],
        });
    };

    let columns = cursor
        .column_names()
        .map_err(|e| format!("Failed to read column names: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to read column names: {}", e))?;

    let mut buffers = TextRowSet::for_cursor(ODBC_BATCH_SIZE, &mut cursor, Some(65_535))
        .map_err(|e| format!("Failed to allocate row buffer: {}", e))?;
    let mut row_set_cursor = cursor
        .bind_buffer(&mut buffers)
        .map_err(|e| format!("Failed to bind row buffer: {}", e))?;

    let mut rows = Vec::new();

    while let Some(batch) = row_set_cursor
        .fetch()
        .map_err(|e| format!("Failed to fetch rows: {}", e))?
    {
        for row_index in 0..batch.num_rows() {
            let mut row = Vec::with_capacity(batch.num_cols());
            for col_index in 0..batch.num_cols() {
                let value = batch
                    .at(col_index, row_index)
                    .map(decode_odbc_text);
                row.push(value);
            }
            rows.push(row);
        }
    }

    Ok(OdbcQueryResult { columns, rows })
}

#[cfg(windows)]
fn test_connection_odbc(conn: &ConnectionConfig) -> Result<String, String> {
    let result = query_odbc(conn, "SELECT @@VERSION AS version, @@SERVERNAME AS server_name")?;

    if let Some(row) = result.rows.first() {
        let version = row.get(0).and_then(|v| v.as_deref()).unwrap_or("unknown");
        let server = row.get(1).and_then(|v| v.as_deref()).unwrap_or("unknown");
        Ok(format!(
            "Connected to: {} ({})",
            server,
            version.lines().next().unwrap_or("")
        ))
    } else {
        Ok("Connected successfully".to_string())
    }
}

#[cfg(windows)]
fn odbc_cell_to_json(value: Option<&str>, column: &ColumnInfo) -> Value {
    let Some(raw) = value else {
        return Value::Null;
    };

    let ty = column.data_type.to_ascii_lowercase();

    match ty.as_str() {
        "bit" => Value::Bool(matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )),
        "tinyint" | "smallint" | "int" | "bigint" => raw
            .trim()
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(raw.to_string())),
        "float" | "real" | "decimal" | "numeric" | "money" | "smallmoney" => raw
            .trim()
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(raw.to_string())),
        _ => Value::String(raw.to_string()),
    }
}

async fn probe_connection(conn: &ConnectionConfig) -> Result<(DbBackend, String), String> {
    #[cfg(windows)]
    {
        let conn_clone = conn.clone();
        let odbc_result = tokio::task::spawn_blocking(move || test_connection_odbc(&conn_clone))
            .await
            .map_err(|e| e.to_string())?;
        match odbc_result {
            Ok(message) => return Ok((DbBackend::Odbc, message)),
            Err(odbc_error) => match test_connection_tds(conn).await {
                Ok(message) => Ok((DbBackend::Tiberius, message)),
                Err(tds_error) => Err(format!(
                "Windows-native driver failed: {}; TCP driver failed: {}",
                odbc_error, tds_error
                )),
            },
        }
    }

    #[cfg(not(windows))]
    {
        let message = test_connection_tds(conn).await?;
        Ok((DbBackend::Tiberius, message))
    }
}

/// Create a new database connection
pub async fn connect(conn: &ConnectionConfig) -> Result<DbClient, String> {
    let (backend, _) = probe_connection(conn).await?;
    Ok(DbClient {
        config: conn.clone(),
        backend,
    })
}

/// Test a connection without storing it
pub async fn test_connection(conn: &ConnectionConfig) -> Result<String, String> {
    let (_, message) = probe_connection(conn).await?;
    Ok(message)
}

pub async fn get_databases(client: &mut DbClient) -> Result<Vec<String>, String> {
    match client.backend {
        #[cfg(windows)]
        DbBackend::Odbc => {
            let config = client.config.clone();
            tokio::task::spawn_blocking(move || get_databases_odbc(&config))
                .await
                .map_err(|e| e.to_string())?
        }
        DbBackend::Tiberius => {
            let mut tds = connect_tds(&client.config).await?;
            get_databases_tds(&mut tds).await
        }
    }
}

async fn get_databases_tds(client: &mut TdsClient) -> Result<Vec<String>, String> {
    let sql = r#"
        SELECT name
        FROM sys.databases
        WHERE name NOT IN ('master', 'tempdb', 'model', 'msdb')
        ORDER BY name
    "#;

    let stream = client
        .simple_query(sql)
        .await
        .map_err(|e| format!("Failed to list databases: {}", e))?;

    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| format!("Failed to read database list: {}", e))?;

    Ok(rows
        .iter()
        .filter_map(|row| row.get::<&str, _>(0).map(|name| name.to_string()))
        .collect())
}

#[cfg(windows)]
fn get_databases_odbc(conn: &ConnectionConfig) -> Result<Vec<String>, String> {
    let sql = r#"
        SELECT name
        FROM sys.databases
        WHERE name NOT IN ('master', 'tempdb', 'model', 'msdb')
        ORDER BY name
    "#;

    let result = query_odbc(conn, sql)?;

    Ok(result
        .rows
        .into_iter()
        .filter_map(|row| row.first().and_then(|value| value.clone()))
        .collect())
}

/// Get all user tables in the database
pub async fn get_tables(client: &mut DbClient) -> Result<Vec<TableInfo>, String> {
    match client.backend {
        #[cfg(windows)]
        DbBackend::Odbc => {
            let config = client.config.clone();
            tokio::task::spawn_blocking(move || get_tables_odbc(&config))
                .await
                .map_err(|e| e.to_string())?
        }
        DbBackend::Tiberius => {
            let mut tds = connect_tds(&client.config).await?;
            get_tables_tds(&mut tds).await
        }
    }
}

async fn get_tables_tds(client: &mut TdsClient) -> Result<Vec<TableInfo>, String> {
    let sql = r#"
        SELECT
            s.name AS schema_name,
            t.name AS table_name,
            CAST(p.rows AS BIGINT) AS row_count
        FROM sys.tables t
        INNER JOIN sys.schemas s ON t.schema_id = s.schema_id
        INNER JOIN sys.indexes i ON t.object_id = i.object_id AND i.index_id IN (0, 1)
        INNER JOIN sys.partitions p ON i.object_id = p.object_id AND i.index_id = p.index_id
        WHERE t.type = 'U'
          AND t.is_ms_shipped = 0
        ORDER BY s.name, t.name
    "#;

    let stream = client
        .simple_query(sql)
        .await
        .map_err(|e| format!("Failed to list tables: {}", e))?;

    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| format!("Failed to read table list: {}", e))?;

    let tables = rows
        .iter()
        .map(|row| {
            let schema: &str = row.get(0).unwrap_or("dbo");
            let name: &str = row.get(1).unwrap_or("");
            let row_count: Option<i64> = row.get(2);

            TableInfo {
                schema: schema.to_string(),
                name: name.to_string(),
                full_name: format!("[{}].[{}]", schema, name),
                row_count,
            }
        })
        .collect();

    Ok(tables)
}

#[cfg(windows)]
fn get_tables_odbc(conn: &ConnectionConfig) -> Result<Vec<TableInfo>, String> {
    let sql = r#"
        SELECT
            s.name AS schema_name,
            t.name AS table_name,
            CAST(p.rows AS BIGINT) AS row_count
        FROM sys.tables t
        INNER JOIN sys.schemas s ON t.schema_id = s.schema_id
        INNER JOIN sys.indexes i ON t.object_id = i.object_id AND i.index_id IN (0, 1)
        INNER JOIN sys.partitions p ON i.object_id = p.object_id AND i.index_id = p.index_id
        WHERE t.type = 'U'
          AND t.is_ms_shipped = 0
        ORDER BY s.name, t.name
    "#;

    let result = query_odbc(conn, sql)?;

    Ok(result
        .rows
        .into_iter()
        .map(|row| {
            let schema = row.get(0).and_then(|v| v.clone()).unwrap_or_else(|| "dbo".to_string());
            let name = row.get(1).and_then(|v| v.clone()).unwrap_or_default();
            let row_count = row
                .get(2)
                .and_then(|v| v.as_deref())
                .and_then(|v| v.parse::<i64>().ok());

            TableInfo {
                full_name: format!("[{}].[{}]", schema, name),
                schema,
                name,
                row_count,
            }
        })
        .collect())
}

/// Get column metadata for a specific table
pub async fn get_columns(
    client: &mut DbClient,
    schema: &str,
    table: &str,
) -> Result<Vec<ColumnInfo>, String> {
    match client.backend {
        #[cfg(windows)]
        DbBackend::Odbc => {
            let config = client.config.clone();
            let schema = schema.to_string();
            let table = table.to_string();
            tokio::task::spawn_blocking(move || get_columns_odbc(&config, &schema, &table))
                .await
                .map_err(|e| e.to_string())?
        }
        DbBackend::Tiberius => {
            let mut tds = connect_tds(&client.config).await?;
            get_columns_tds(&mut tds, schema, table).await
        }
    }
}

async fn get_columns_tds(
    client: &mut TdsClient,
    schema: &str,
    table: &str,
) -> Result<Vec<ColumnInfo>, String> {
    let sql = format!(
        r#"
        SELECT
            c.name AS column_name,
            tp.name AS data_type,
            c.is_nullable,
            CAST(CASE WHEN pk.column_id IS NOT NULL THEN 1 ELSE 0 END AS BIT) AS is_primary_key,
            c.column_id AS ordinal,
            c.max_length,
            c.precision,
            c.scale
        FROM sys.columns c
        INNER JOIN sys.tables t ON c.object_id = t.object_id
        INNER JOIN sys.schemas s ON t.schema_id = s.schema_id
        INNER JOIN sys.types tp ON c.user_type_id = tp.user_type_id
        LEFT JOIN (
            SELECT ic.object_id, ic.column_id
            FROM sys.index_columns ic
            INNER JOIN sys.indexes i ON ic.object_id = i.object_id AND ic.index_id = i.index_id
            WHERE i.is_primary_key = 1
        ) pk ON c.object_id = pk.object_id AND c.column_id = pk.column_id
        WHERE s.name = '{}'
          AND t.name = '{}'
        ORDER BY c.column_id
        "#,
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let stream = client
        .simple_query(&sql)
        .await
        .map_err(|e| format!("Failed to get columns: {}", e))?;

    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| format!("Failed to read column info: {}", e))?;

    let columns = rows
        .iter()
        .map(|row| {
            let name: &str = row.get(0).unwrap_or("");
            let data_type: &str = row.get(1).unwrap_or("");
            let is_nullable: bool = row.get(2).unwrap_or(true);
            let is_primary_key: bool = row.get(3).unwrap_or(false);
            let ordinal: i32 = row.get(4).unwrap_or(0);
            let max_length: Option<i16> = row.get(5);
            let precision: Option<u8> = row.get(6);
            let scale: Option<u8> = row.get(7);

            ColumnInfo {
                name: name.to_string(),
                data_type: data_type.to_string(),
                is_nullable,
                is_primary_key,
                ordinal,
                max_length: max_length.map(|v| v as i32),
                precision: precision.map(|v| v as i32),
                scale: scale.map(|v| v as i32),
            }
        })
        .collect();

    Ok(columns)
}

#[cfg(windows)]
fn get_columns_odbc(
    conn: &ConnectionConfig,
    schema: &str,
    table: &str,
) -> Result<Vec<ColumnInfo>, String> {
    let sql = format!(
        r#"
        SELECT
            c.name AS column_name,
            tp.name AS data_type,
            c.is_nullable,
            CAST(CASE WHEN pk.column_id IS NOT NULL THEN 1 ELSE 0 END AS BIT) AS is_primary_key,
            c.column_id AS ordinal,
            c.max_length,
            c.precision,
            c.scale
        FROM sys.columns c
        INNER JOIN sys.tables t ON c.object_id = t.object_id
        INNER JOIN sys.schemas s ON t.schema_id = s.schema_id
        INNER JOIN sys.types tp ON c.user_type_id = tp.user_type_id
        LEFT JOIN (
            SELECT ic.object_id, ic.column_id
            FROM sys.index_columns ic
            INNER JOIN sys.indexes i ON ic.object_id = i.object_id AND ic.index_id = i.index_id
            WHERE i.is_primary_key = 1
        ) pk ON c.object_id = pk.object_id AND c.column_id = pk.column_id
        WHERE s.name = '{}'
          AND t.name = '{}'
        ORDER BY c.column_id
        "#,
        schema.replace("'", "''"),
        table.replace("'", "''")
    );

    let result = query_odbc(conn, &sql)?;

    Ok(result
        .rows
        .into_iter()
        .map(|row| ColumnInfo {
            name: row.get(0).and_then(|v| v.clone()).unwrap_or_default(),
            data_type: row.get(1).and_then(|v| v.clone()).unwrap_or_default(),
            is_nullable: matches!(row.get(2).and_then(|v| v.as_deref()), Some("1") | Some("true") | Some("TRUE")),
            is_primary_key: matches!(row.get(3).and_then(|v| v.as_deref()), Some("1") | Some("true") | Some("TRUE")),
            ordinal: row
                .get(4)
                .and_then(|v| v.as_deref())
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(0),
            max_length: row
                .get(5)
                .and_then(|v| v.as_deref())
                .and_then(|v| v.parse::<i32>().ok()),
            precision: row
                .get(6)
                .and_then(|v| v.as_deref())
                .and_then(|v| v.parse::<i32>().ok()),
            scale: row
                .get(7)
                .and_then(|v| v.as_deref())
                .and_then(|v| v.parse::<i32>().ok()),
        })
        .collect())
}

/// Get foreign-key relationships for the current database
pub async fn get_relationships(client: &mut DbClient) -> Result<Vec<RelationshipInfo>, String> {
    match client.backend {
        #[cfg(windows)]
        DbBackend::Odbc => {
            let config = client.config.clone();
            tokio::task::spawn_blocking(move || get_relationships_odbc(&config))
                .await
                .map_err(|e| e.to_string())?
        }
        DbBackend::Tiberius => {
            let mut tds = connect_tds(&client.config).await?;
            get_relationships_tds(&mut tds).await
        }
    }
}

async fn get_relationships_tds(client: &mut TdsClient) -> Result<Vec<RelationshipInfo>, String> {
    let sql = r#"
        SELECT
            fk.name AS constraint_name,
            src_schema.name AS source_schema,
            src_table.name AS source_table,
            src_col.name AS source_column,
            dst_schema.name AS target_schema,
            dst_table.name AS target_table,
            dst_col.name AS target_column,
            fkc.constraint_column_id AS ordinal
        FROM sys.foreign_keys fk
        INNER JOIN sys.foreign_key_columns fkc
            ON fk.object_id = fkc.constraint_object_id
        INNER JOIN sys.tables src_table
            ON fk.parent_object_id = src_table.object_id
        INNER JOIN sys.schemas src_schema
            ON src_table.schema_id = src_schema.schema_id
        INNER JOIN sys.columns src_col
            ON src_col.object_id = src_table.object_id
           AND src_col.column_id = fkc.parent_column_id
        INNER JOIN sys.tables dst_table
            ON fk.referenced_object_id = dst_table.object_id
        INNER JOIN sys.schemas dst_schema
            ON dst_table.schema_id = dst_schema.schema_id
        INNER JOIN sys.columns dst_col
            ON dst_col.object_id = dst_table.object_id
           AND dst_col.column_id = fkc.referenced_column_id
        ORDER BY
            src_schema.name,
            src_table.name,
            fk.name,
            fkc.constraint_column_id
    "#;

    let stream = client
        .simple_query(sql)
        .await
        .map_err(|e| format!("Failed to read relationships: {}", e))?;

    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| format!("Failed to read relationship data: {}", e))?;

    Ok(rows
        .iter()
        .map(|row| RelationshipInfo {
            constraint_name: row.get::<&str, _>(0).unwrap_or("").to_string(),
            source_schema: row.get::<&str, _>(1).unwrap_or("dbo").to_string(),
            source_table: row.get::<&str, _>(2).unwrap_or("").to_string(),
            source_column: row.get::<&str, _>(3).unwrap_or("").to_string(),
            target_schema: row.get::<&str, _>(4).unwrap_or("dbo").to_string(),
            target_table: row.get::<&str, _>(5).unwrap_or("").to_string(),
            target_column: row.get::<&str, _>(6).unwrap_or("").to_string(),
            ordinal: row.get::<i32, _>(7).unwrap_or(0),
        })
        .collect())
}

#[cfg(windows)]
fn get_relationships_odbc(conn: &ConnectionConfig) -> Result<Vec<RelationshipInfo>, String> {
    let sql = r#"
        SELECT
            fk.name AS constraint_name,
            src_schema.name AS source_schema,
            src_table.name AS source_table,
            src_col.name AS source_column,
            dst_schema.name AS target_schema,
            dst_table.name AS target_table,
            dst_col.name AS target_column,
            fkc.constraint_column_id AS ordinal
        FROM sys.foreign_keys fk
        INNER JOIN sys.foreign_key_columns fkc
            ON fk.object_id = fkc.constraint_object_id
        INNER JOIN sys.tables src_table
            ON fk.parent_object_id = src_table.object_id
        INNER JOIN sys.schemas src_schema
            ON src_table.schema_id = src_schema.schema_id
        INNER JOIN sys.columns src_col
            ON src_col.object_id = src_table.object_id
           AND src_col.column_id = fkc.parent_column_id
        INNER JOIN sys.tables dst_table
            ON fk.referenced_object_id = dst_table.object_id
        INNER JOIN sys.schemas dst_schema
            ON dst_table.schema_id = dst_schema.schema_id
        INNER JOIN sys.columns dst_col
            ON dst_col.object_id = dst_table.object_id
           AND dst_col.column_id = fkc.referenced_column_id
        ORDER BY
            src_schema.name,
            src_table.name,
            fk.name,
            fkc.constraint_column_id
    "#;

    let result = query_odbc(conn, sql)?;

    Ok(result
        .rows
        .into_iter()
        .map(|row| RelationshipInfo {
            constraint_name: row.get(0).and_then(|v| v.clone()).unwrap_or_default(),
            source_schema: row.get(1).and_then(|v| v.clone()).unwrap_or_else(|| "dbo".to_string()),
            source_table: row.get(2).and_then(|v| v.clone()).unwrap_or_default(),
            source_column: row.get(3).and_then(|v| v.clone()).unwrap_or_default(),
            target_schema: row.get(4).and_then(|v| v.clone()).unwrap_or_else(|| "dbo".to_string()),
            target_table: row.get(5).and_then(|v| v.clone()).unwrap_or_default(),
            target_column: row.get(6).and_then(|v| v.clone()).unwrap_or_default(),
            ordinal: row
                .get(7)
                .and_then(|v| v.as_deref())
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(0),
        })
        .collect())
}

/// Build a safe WHERE clause from filters
pub fn build_where_clause(filters: &[Filter]) -> String {
    if filters.is_empty() {
        return String::new();
    }

    let conditions: Vec<String> = filters
        .iter()
        .filter(|f| !f.column.is_empty())
        .map(|f| {
            let col = format!("[{}]", f.column.replace(']', "]]"));
            let is_numeric = matches!(
                f.data_type.as_deref().unwrap_or(""),
                "int"
                    | "bigint"
                    | "smallint"
                    | "tinyint"
                    | "float"
                    | "real"
                    | "decimal"
                    | "numeric"
                    | "money"
                    | "smallmoney"
                    | "bit"
            );

            let quote_val = |v: &str| -> String {
                if is_numeric {
                    v.to_string()
                } else {
                    format!("'{}'", v.replace("'", "''"))
                }
            };

            match f.operator.as_str() {
                "IS NULL" => format!("{} IS NULL", col),
                "IS NOT NULL" => format!("{} IS NOT NULL", col),
                "BETWEEN" => {
                    let v1 = f.value.as_deref().unwrap_or("");
                    let v2 = f.value2.as_deref().unwrap_or("");
                    format!("{} BETWEEN {} AND {}", col, quote_val(v1), quote_val(v2))
                }
                "LIKE" => {
                    let v = f.value.as_deref().unwrap_or("");
                    format!("{} LIKE '{}'", col, v.replace("'", "''"))
                }
                "NOT LIKE" => {
                    let v = f.value.as_deref().unwrap_or("");
                    format!("{} NOT LIKE '{}'", col, v.replace("'", "''"))
                }
                "IN" => {
                    let v = f.value.as_deref().unwrap_or("");
                    let parts: Vec<String> = v.split(',').map(|s| quote_val(s.trim())).collect();
                    format!("{} IN ({})", col, parts.join(", "))
                }
                op => {
                    let v = f.value.as_deref().unwrap_or("");
                    format!("{} {} {}", col, op, quote_val(v))
                }
            }
        })
        .collect();

    if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    }
}

/// Convert a tiberius row value to serde_json::Value
fn cell_to_json_tds(row: &tiberius::Row, idx: usize) -> Value {
    let col_type = row.columns()[idx].column_type();

    match col_type {
        ColumnType::Bit => {
            let v: Option<bool> = row.get(idx);
            json!(v)
        }
        ColumnType::Int1 => {
            let v: Option<u8> = row.get(idx);
            json!(v)
        }
        ColumnType::Int2 => {
            let v: Option<i16> = row.get(idx);
            json!(v)
        }
        ColumnType::Int4 => {
            let v: Option<i32> = row.get(idx);
            json!(v)
        }
        ColumnType::Int8 => {
            let v: Option<i64> = row.get(idx);
            json!(v)
        }
        ColumnType::Intn => {
            let v: Option<i64> = row.get(idx);
            json!(v)
        }
        ColumnType::Float4 => {
            let v: Option<f32> = row.get(idx);
            json!(v)
        }
        ColumnType::Float8 => {
            let v: Option<f64> = row.get(idx);
            json!(v)
        }
        ColumnType::Floatn => {
            let v: Option<f64> = row.get(idx);
            json!(v)
        }
        ColumnType::Numericn | ColumnType::Decimaln => {
            let v: Option<rust_decimal::Decimal> = row.get(idx);
            json!(v.map(|d| d.to_string()))
        }
        ColumnType::Money | ColumnType::Money4 => {
            let v: Option<f64> = row.get(idx);
            json!(v)
        }
        ColumnType::Datetime | ColumnType::Datetime2 | ColumnType::Datetimen => {
            let v: Option<chrono::NaiveDateTime> = row.get(idx);
            json!(v.map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string()))
        }
        ColumnType::DatetimeOffsetn => {
            let v: Option<chrono::DateTime<chrono::FixedOffset>> = row.get(idx);
            json!(v.map(|d| d.to_rfc3339()))
        }
        ColumnType::Daten => {
            let v: Option<chrono::NaiveDate> = row.get(idx);
            json!(v.map(|d| d.to_string()))
        }
        ColumnType::Timen => {
            let v: Option<chrono::NaiveTime> = row.get(idx);
            json!(v.map(|d| d.to_string()))
        }
        ColumnType::Guid => {
            let v: Option<uuid::Uuid> = row.get(idx);
            json!(v.map(|u| u.to_string()))
        }
        ColumnType::BigBinary | ColumnType::BigVarBin | ColumnType::Image => {
            let v: Option<&[u8]> = row.get(idx);
            json!(v.map(|b| format!("0x{}", hex::encode(b))))
        }
        _ => {
            let v: Option<&str> = row.get(idx);
            json!(v)
        }
    }
}

/// Fetch paginated table data with optional filters
pub async fn get_table_data(
    client: &mut DbClient,
    schema: &str,
    table: &str,
    page: u32,
    page_size: u32,
    filters: &[Filter],
    columns: &[ColumnInfo],
) -> Result<TableData, String> {
    match client.backend {
        #[cfg(windows)]
        DbBackend::Odbc => {
            let config = client.config.clone();
            let schema = schema.to_string();
            let table = table.to_string();
            let filters = filters.to_vec();
            let columns = columns.to_vec();
            tokio::task::spawn_blocking(move || {
                get_table_data_odbc(&config, &schema, &table, page, page_size, &filters, &columns)
            })
            .await
            .map_err(|e| e.to_string())?
        }
        DbBackend::Tiberius => {
            let mut tds = connect_tds(&client.config).await?;
            get_table_data_tds(&mut tds, schema, table, page, page_size, filters, columns).await
        }
    }
}

async fn get_table_data_tds(
    client: &mut TdsClient,
    schema: &str,
    table: &str,
    page: u32,
    page_size: u32,
    filters: &[Filter],
    columns: &[ColumnInfo],
) -> Result<TableData, String> {
    let full_table = format!("[{}].[{}]", schema.replace(']', "]]"), table.replace(']', "]]"));
    let where_clause = build_where_clause(filters);
    let offset = page * page_size;

    let count_sql = format!("SELECT COUNT_BIG(*) FROM {} {}", full_table, where_clause);

    let count_stream = client
        .simple_query(&count_sql)
        .await
        .map_err(|e| format!("Count query failed: {}", e))?;

    let count_rows = count_stream
        .into_first_result()
        .await
        .map_err(|e| format!("Failed to read count: {}", e))?;

    let total_count: i64 = count_rows.first().and_then(|r| r.get(0)).unwrap_or(0);

    let data_sql = format!(
        r#"
        SELECT *
        FROM {}
        {}
        ORDER BY (SELECT NULL)
        OFFSET {} ROWS
        FETCH NEXT {} ROWS ONLY
        "#,
        full_table, where_clause, offset, page_size
    );

    let data_stream = client
        .simple_query(&data_sql)
        .await
        .map_err(|e| format!("Data query failed: {}", e))?;

    let rows = data_stream
        .into_first_result()
        .await
        .map_err(|e| format!("Failed to read data: {}", e))?;

    let row_data: Vec<Vec<Value>> = rows
        .iter()
        .map(|row| (0..row.columns().len()).map(|i| cell_to_json_tds(row, i)).collect())
        .collect();

    Ok(TableData {
        columns: columns.to_vec(),
        rows: row_data,
        total_count,
        page,
        page_size,
    })
}

#[cfg(windows)]
fn get_table_data_odbc(
    conn: &ConnectionConfig,
    schema: &str,
    table: &str,
    page: u32,
    page_size: u32,
    filters: &[Filter],
    columns: &[ColumnInfo],
) -> Result<TableData, String> {
    let full_table = format!("[{}].[{}]", schema.replace(']', "]]"), table.replace(']', "]]"));
    let where_clause = build_where_clause(filters);
    let offset = page * page_size;

    let count_sql = format!("SELECT COUNT_BIG(*) FROM {} {}", full_table, where_clause);
    let count_result = query_odbc(conn, &count_sql)?;
    let total_count = count_result
        .rows
        .first()
        .and_then(|row| row.first())
        .and_then(|value| value.as_deref())
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);

    let data_sql = format!(
        r#"
        SELECT *
        FROM {}
        {}
        ORDER BY (SELECT NULL)
        OFFSET {} ROWS
        FETCH NEXT {} ROWS ONLY
        "#,
        full_table, where_clause, offset, page_size
    );

    let data_result = query_odbc(conn, &data_sql)?;
    let rows = data_result
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(idx, value)| {
                    if let Some(column) = columns.get(idx) {
                        odbc_cell_to_json(value.as_deref(), column)
                    } else {
                        value
                            .clone()
                            .map(Value::String)
                            .unwrap_or(Value::Null)
                    }
                })
                .collect()
        })
        .collect();

    Ok(TableData {
        columns: columns.to_vec(),
        rows,
        total_count,
        page,
        page_size,
    })
}

/// Export table data to CSV string
pub async fn export_csv(
    client: &mut DbClient,
    schema: &str,
    table: &str,
    filters: &[Filter],
    selected_columns: &[String],
) -> Result<String, String> {
    match client.backend {
        #[cfg(windows)]
        DbBackend::Odbc => {
            let config = client.config.clone();
            let schema = schema.to_string();
            let table = table.to_string();
            let filters = filters.to_vec();
            let selected_columns = selected_columns.to_vec();
            tokio::task::spawn_blocking(move || {
                export_csv_odbc(&config, &schema, &table, &filters, &selected_columns)
            })
            .await
            .map_err(|e| e.to_string())?
        }
        DbBackend::Tiberius => {
            let mut tds = connect_tds(&client.config).await?;
            export_csv_tds(&mut tds, schema, table, filters, selected_columns).await
        }
    }
}

async fn export_csv_tds(
    client: &mut TdsClient,
    schema: &str,
    table: &str,
    filters: &[Filter],
    selected_columns: &[String],
) -> Result<String, String> {
    let full_table = format!("[{}].[{}]", schema.replace(']', "]]"), table.replace(']', "]]"));
    let where_clause = build_where_clause(filters);

    let col_select = if selected_columns.is_empty() {
        "*".to_string()
    } else {
        selected_columns
            .iter()
            .map(|c| format!("[{}]", c.replace(']', "]]")))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let sql = format!("SELECT {} FROM {} {}", col_select, full_table, where_clause);

    let stream = client
        .simple_query(&sql)
        .await
        .map_err(|e| format!("Export query failed: {}", e))?;

    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| format!("Failed to read export data: {}", e))?;

    let mut csv = String::new();

    if let Some(first_row) = rows.first() {
        let headers: Vec<String> = first_row
            .columns()
            .iter()
            .map(|c| {
                let name = c.name();
                if name.contains(',') || name.contains('"') || name.contains('\n') {
                    format!("\"{}\"", name.replace('"', "\"\""))
                } else {
                    name.to_string()
                }
            })
            .collect();
        csv.push_str(&headers.join(","));
        csv.push('\n');
    }

    for row in &rows {
        let cells: Vec<String> = (0..row.columns().len())
            .map(|i| {
                let val = cell_to_json_tds(row, i);
                match &val {
                    Value::Null => String::new(),
                    Value::Bool(b) => b.to_string(),
                    Value::Number(n) => n.to_string(),
                    Value::String(s) => {
                        if s.contains(',') || s.contains('"') || s.contains('\n') {
                            format!("\"{}\"", s.replace('"', "\"\""))
                        } else {
                            s.clone()
                        }
                    }
                    _ => val.to_string(),
                }
            })
            .collect();
        csv.push_str(&cells.join(","));
        csv.push('\n');
    }

    Ok(csv)
}

#[cfg(windows)]
fn export_csv_odbc(
    conn: &ConnectionConfig,
    schema: &str,
    table: &str,
    filters: &[Filter],
    selected_columns: &[String],
) -> Result<String, String> {
    let full_table = format!("[{}].[{}]", schema.replace(']', "]]"), table.replace(']', "]]"));
    let where_clause = build_where_clause(filters);

    let col_select = if selected_columns.is_empty() {
        "*".to_string()
    } else {
        selected_columns
            .iter()
            .map(|c| format!("[{}]", c.replace(']', "]]")))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let sql = format!("SELECT {} FROM {} {}", col_select, full_table, where_clause);
    let result = query_odbc(conn, &sql)?;
    let mut csv = String::new();

    if !result.columns.is_empty() {
        let headers: Vec<String> = result
            .columns
            .iter()
            .map(|name| {
                if name.contains(',') || name.contains('"') || name.contains('\n') {
                    format!("\"{}\"", name.replace('"', "\"\""))
                } else {
                    name.clone()
                }
            })
            .collect();
        csv.push_str(&headers.join(","));
        csv.push('\n');
    }

    for row in result.rows {
        let cells: Vec<String> = row
            .into_iter()
            .map(|value| {
                let value = value.unwrap_or_default();
                if value.contains(',') || value.contains('"') || value.contains('\n') {
                    format!("\"{}\"", value.replace('"', "\"\""))
                } else {
                    value
                }
            })
            .collect();
        csv.push_str(&cells.join(","));
        csv.push('\n');
    }

    Ok(csv)
}

/// Export table data to JSON string
pub async fn export_json(
    client: &mut DbClient,
    schema: &str,
    table: &str,
    filters: &[Filter],
    selected_columns: &[String],
) -> Result<String, String> {
    match client.backend {
        #[cfg(windows)]
        DbBackend::Odbc => {
            let config = client.config.clone();
            let schema = schema.to_string();
            let table = table.to_string();
            let filters = filters.to_vec();
            let selected_columns = selected_columns.to_vec();
            tokio::task::spawn_blocking(move || {
                export_json_odbc(&config, &schema, &table, &filters, &selected_columns)
            })
            .await
            .map_err(|e| e.to_string())?
        }
        DbBackend::Tiberius => {
            let mut tds = connect_tds(&client.config).await?;
            export_json_tds(&mut tds, schema, table, filters, selected_columns).await
        }
    }
}

async fn export_json_tds(
    client: &mut TdsClient,
    schema: &str,
    table: &str,
    filters: &[Filter],
    selected_columns: &[String],
) -> Result<String, String> {
    let full_table = format!("[{}].[{}]", schema.replace(']', "]]"), table.replace(']', "]]"));
    let where_clause = build_where_clause(filters);

    let col_select = if selected_columns.is_empty() {
        "*".to_string()
    } else {
        selected_columns
            .iter()
            .map(|c| format!("[{}]", c.replace(']', "]]")))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let sql = format!("SELECT {} FROM {} {}", col_select, full_table, where_clause);

    let stream = client
        .simple_query(&sql)
        .await
        .map_err(|e| format!("Export query failed: {}", e))?;

    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| format!("Failed to read export data: {}", e))?;

    let mut records: Vec<serde_json::Map<String, Value>> = Vec::new();

    for row in &rows {
        let mut record = serde_json::Map::new();
        for (i, col) in row.columns().iter().enumerate() {
            record.insert(col.name().to_string(), cell_to_json_tds(row, i));
        }
        records.push(record);
    }

    serde_json::to_string_pretty(&records).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn export_json_odbc(
    conn: &ConnectionConfig,
    schema: &str,
    table: &str,
    filters: &[Filter],
    selected_columns: &[String],
) -> Result<String, String> {
    let full_table = format!("[{}].[{}]", schema.replace(']', "]]"), table.replace(']', "]]"));
    let where_clause = build_where_clause(filters);

    let col_select = if selected_columns.is_empty() {
        "*".to_string()
    } else {
        selected_columns
            .iter()
            .map(|c| format!("[{}]", c.replace(']', "]]")))
            .collect::<Vec<_>>()
            .join(", ")
    };

    let sql = format!("SELECT {} FROM {} {}", col_select, full_table, where_clause);
    let result = query_odbc(conn, &sql)?;
    let column_meta = get_columns_odbc(conn, schema, table)?;
    let OdbcQueryResult { columns, rows } = result;

    let mut records = Vec::new();

    for row in rows {
        let mut record = serde_json::Map::new();
        for (idx, name) in columns.iter().enumerate() {
            let meta = column_meta.iter().find(|column| column.name.eq_ignore_ascii_case(name));
            let value = row.get(idx).and_then(|value| value.as_deref());
            let json_value = match meta {
                Some(column) => odbc_cell_to_json(value, column),
                None => value
                    .map(|value| Value::String(value.to_string()))
                    .unwrap_or(Value::Null),
            };
            record.insert(name.clone(), json_value);
        }
        records.push(record);
    }

    serde_json::to_string_pretty(&records).map_err(|e| e.to_string())
}

/// Execute a raw SQL query and return results
pub async fn execute_raw_query(client: &mut DbClient, sql: &str) -> Result<TableData, String> {
    match client.backend {
        #[cfg(windows)]
        DbBackend::Odbc => {
            let config = client.config.clone();
            let sql = sql.to_string();
            tokio::task::spawn_blocking(move || execute_raw_query_odbc(&config, &sql))
                .await
                .map_err(|e| e.to_string())?
        }
        DbBackend::Tiberius => {
            let mut tds = connect_tds(&client.config).await?;
            execute_raw_query_tds(&mut tds, sql).await
        }
    }
}

async fn execute_raw_query_tds(client: &mut TdsClient, sql: &str) -> Result<TableData, String> {
    let stream = client
        .simple_query(sql)
        .await
        .map_err(|e| format!("Query failed: {}", e))?;

    let rows = stream
        .into_first_result()
        .await
        .map_err(|e| format!("Failed to read results: {}", e))?;

    let columns: Vec<ColumnInfo> = if let Some(first) = rows.first() {
        first
            .columns()
            .iter()
            .enumerate()
            .map(|(i, col)| ColumnInfo {
                name: col.name().to_string(),
                data_type: format!("{:?}", col.column_type()),
                is_nullable: true,
                is_primary_key: false,
                ordinal: i as i32,
                max_length: None,
                precision: None,
                scale: None,
            })
            .collect()
    } else {
        vec![]
    };

    let row_data: Vec<Vec<Value>> = rows
        .iter()
        .map(|row| (0..row.columns().len()).map(|i| cell_to_json_tds(row, i)).collect())
        .collect();

    let total = row_data.len() as i64;

    Ok(TableData {
        columns,
        rows: row_data,
        total_count: total,
        page: 0,
        page_size: total as u32,
    })
}

#[cfg(windows)]
fn execute_raw_query_odbc(conn: &ConnectionConfig, sql: &str) -> Result<TableData, String> {
    let result = query_odbc(conn, sql)?;
    let columns: Vec<ColumnInfo> = result
        .columns
        .iter()
        .enumerate()
        .map(|(idx, name)| ColumnInfo {
            name: name.clone(),
            data_type: "nvarchar".to_string(),
            is_nullable: true,
            is_primary_key: false,
            ordinal: idx as i32,
            max_length: None,
            precision: None,
            scale: None,
        })
        .collect();

    let rows = result
        .rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|value| value.map(Value::String).unwrap_or(Value::Null))
                .collect()
        })
        .collect::<Vec<Vec<Value>>>();

    let total = rows.len() as i64;

    Ok(TableData {
        columns,
        rows,
        total_count: total,
        page: 0,
        page_size: total as u32,
    })
}

/// Open a raw TDS connection for streaming operations (one connection shared across all chunks)
pub async fn open_tds_connection(conn: &ConnectionConfig) -> Result<TdsClient, String> {
    connect_tds(conn).await
}

/// Execute a raw SQL query on an EXISTING TDS connection (used for streaming chunks)
pub async fn run_tds_query(tds: &mut TdsClient, sql: &str) -> Result<TableData, String> {
    execute_raw_query_tds(tds, sql).await
}

/// Get or create a cached DbClient for a connection (avoids re-probing backend on every command)
pub async fn get_or_connect(
    state: &crate::state::AppState,
    id: &str,
) -> Result<std::sync::Arc<CachedClient>, String> {
    let cached_opt = {
        let cache = state.client_cache.lock().map_err(|e| e.to_string())?;
        cache.get(id).cloned()
    };
    if let Some(cached) = cached_opt {
        return Ok(cached);
    }
    let conn_config = state.resolve_connection_config(id)?;
    let new_client = connect(&conn_config).await?;
    let new_cached = std::sync::Arc::new(CachedClient {
        client: tokio::sync::Mutex::new(new_client),
        connected_at: std::time::Instant::now(),
    });
    {
        let mut cache = state.client_cache.lock().map_err(|e| e.to_string())?;
        cache.insert(id.to_string(), new_cached.clone());
    }
    Ok(new_cached)
}
