// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analytics;
mod bridge;
mod commands;
mod db;
mod export_jobs;
mod invoice_commands;
mod invoice_compat;
mod network_scan;
mod pdf_engine;
mod sage_compat;
mod sage_entity_service;
mod state;

use state::AppState;

#[cfg(target_os = "windows")]
fn configure_windows_webview2() {
    use std::path::PathBuf;

    if std::env::var_os("WEBVIEW2_USER_DATA_FOLDER").is_some() {
        return;
    }

    let base_dir = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    let webview_dir: PathBuf = base_dir.join("SageDataBridge").join("WebView2");

    if let Err(err) = std::fs::create_dir_all(&webview_dir) {
        eprintln!(
            "failed to prepare WebView2 user data folder at {}: {}",
            webview_dir.display(),
            err
        );
        return;
    }

    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &webview_dir);
}

fn main() {
    #[cfg(target_os = "windows")]
    configure_windows_webview2();

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
            // Settings
            commands::get_settings,
            commands::save_settings,
            commands::get_exploitation_report,
            commands::save_exploitation_report,
            commands::delete_exploitation_report,
            commands::get_exploitation_mappings,
            commands::save_exploitation_mappings,
            commands::reset_exploitation_mappings,
            // Field history
            commands::get_field_history,
            commands::save_field_history,
            commands::remove_field_history_entry,
            // Connections
            commands::list_connections,
            commands::save_connection,
            commands::delete_connection,
            commands::reveal_connection_password,
            commands::test_connection,
            commands::discover_databases,
            commands::connect_db,
            commands::get_databases,
            commands::switch_database,
            commands::open_database_workspace,
            commands::close_database_workspace,
            commands::run_connection_diagnostics,
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
            export_jobs::start_table_export,
            export_jobs::list_export_jobs,
            export_jobs::cancel_export_job,
            // Network scan
            network_scan::scan_network_for_sql_servers,
            // Analytics
            analytics::get_grand_livre,
            analytics::get_grand_livre_page,
            analytics::get_balance,
            analytics::get_compte_exploitation_accounts,
            analytics::get_grand_livre_auxiliaire,
            analytics::stream_grand_livre,
            analytics::export_grand_livre_xlsx,
            analytics::stream_balance,
            analytics::stream_grand_livre_auxiliaire,
            analytics::get_dashboard_kpis,
            analytics::get_dashboard_tiers,
            // Invoicing
            invoice_commands::list_invoices,
            invoice_commands::get_invoice,
            invoice_commands::create_invoice,
            invoice_commands::update_invoice,
            invoice_commands::update_statut,
            invoice_commands::delete_invoice,
            invoice_commands::comptabiliser_invoice,
            invoice_commands::mark_invoice_comptabilise_from_bridge,
            invoice_commands::list_tiers,
            invoice_commands::create_tiers,
            invoice_commands::update_tiers,
            invoice_commands::delete_tiers,
            invoice_commands::list_articles,
            invoice_commands::create_article,
            invoice_commands::update_article,
            invoice_commands::delete_article,
            // Normalized commercial-to-accounting bridge
            bridge::initialize_bridge,
            bridge::list_bridge_documents,
            bridge::create_bridge_document,
            bridge::validate_bridge_document,
            bridge::transform_bridge_quote,
            bridge::generate_bridge_invoice_posting,
            bridge::register_bridge_payment,
            bridge::generate_bridge_payment_posting,
            bridge::get_bridge_accounting_configuration,
            bridge::save_bridge_accounting_configuration,
            bridge::get_bridge_master_data,
            bridge::save_bridge_product_profile,
            bridge::get_bridge_management_data,
            bridge::save_bridge_management_record,
            bridge::deactivate_bridge_management_record,
            bridge::preview_bridge_accounting,
            bridge::list_bridge_posting_batches,
            bridge::prepare_bridge_export,
            pdf_engine::render_invoice_html,
            pdf_engine::render_invoice_html_preview,
            pdf_engine::export_invoice_pdf,
            pdf_engine::save_invoice_template,
            pdf_engine::get_invoice_templates,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
