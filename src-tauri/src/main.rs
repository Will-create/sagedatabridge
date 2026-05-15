// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analytics;
mod commands;
mod db;
mod network_scan;
mod sage_compat;
mod state;

use state::AppState;

fn main() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            // Auth
            commands::has_password,
            commands::set_password,
            commands::verify_password,
            commands::remove_password,
            commands::has_pin,
            commands::set_pin,
            commands::verify_pin,
            commands::remove_pin,
            commands::has_admin_password,
            commands::set_admin_password,
            commands::verify_admin_password,
            commands::remove_admin_password,
            // Field history
            commands::get_field_history,
            commands::save_field_history,
            commands::remove_field_history_entry,
            // Connections
            commands::list_connections,
            commands::save_connection,
            commands::delete_connection,
            commands::test_connection,
            commands::discover_databases,
            commands::connect_db,
            commands::get_databases,
            commands::switch_database,
            commands::disconnect_db,
            commands::get_connection_statuses,
            // Sage edition detection
            commands::detect_sage_edition,
            commands::get_active_schema,
            commands::get_active_schemas,
            // Schema
            commands::get_tables,
            commands::get_columns,
            commands::get_relationships,
            // Data
            commands::get_table_data,
            commands::execute_query,
            // Query library
            commands::list_saved_queries,
            commands::save_saved_query,
            commands::delete_saved_query,
            commands::list_query_history,
            commands::clear_query_history,
            // Export
            commands::export_csv,
            commands::export_json,
            commands::export_sql,
            // Network scan
            network_scan::scan_network_for_sql_servers,
            // Analytics
            analytics::stream_grand_livre,
            analytics::stream_balance,
            analytics::stream_grand_livre_auxiliaire,
            analytics::get_dashboard_kpis,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
