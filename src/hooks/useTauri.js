import { invoke } from "@tauri-apps/api/tauri";
import { save } from "@tauri-apps/api/dialog";
import { writeBinaryFile } from "@tauri-apps/api/fs";
import * as XLSX from "xlsx";

// Re-export tauri invoke for easy mocking/testing
export { invoke };

// ─── Auth ────────────────────────────────────────────────────────────────────

export const hasPassword = () => invoke("has_password");
export const setPassword = (password) => invoke("set_password", { password });
export const verifyPassword = (password) => invoke("verify_password", { password });
export const removePassword = () => invoke("remove_password");
export const hasPin = () => invoke("has_pin");
export const setPin = (pin) => invoke("set_pin", { pin });
export const verifyPin = (pin) => invoke("verify_pin", { pin });
export const removePin = () => invoke("remove_pin");
export const hasAdminPassword = () => invoke("has_admin_password");
export const setAdminPassword = (password) => invoke("set_admin_password", { password });
export const verifyAdminPassword = (password) => invoke("verify_admin_password", { password });
export const removeAdminPassword = () => invoke("remove_admin_password");

// ─── Field History ────────────────────────────────────────────────────────────

export const getFieldHistory = (field) => invoke("get_field_history", { field });
export const saveFieldHistory = (field, value) => invoke("save_field_history", { field, value });
export const removeFieldHistoryEntry = (field, value) => invoke("remove_field_history_entry", { field, value });

// ─── Connections ─────────────────────────────────────────────────────────────

export const listConnections = () => invoke("list_connections");
export const saveConnection = (connection) => invoke("save_connection", { connection });
export const deleteConnection = (id) => invoke("delete_connection", { id });
export const testConnection = (connection) => invoke("test_connection", { connection });
export const discoverDatabases = (connection) => invoke("discover_databases", { connection });
export const connectDb = (id) => invoke("connect_db", { id });
export const getDatabases = (connectionId) => invoke("get_databases", { connectionId });
export const switchDatabase = (connectionId, database) =>
  invoke("switch_database", { connectionId, database });
export const disconnectDb = (id) => invoke("disconnect_db", { id });
export const getConnectionStatuses = () => invoke("get_connection_statuses");
export const detectSageEdition = (connectionId) => invoke("detect_sage_edition", { connectionId });
export const scanNetworkForSqlServers = () => invoke("scan_network_for_sql_servers");
export const testMapping = (connection, schema) => invoke("test_mapping", { connection, schema });
export const getActiveSchema = (connectionId) => invoke("get_active_schema", { connectionId });
export const getActiveSchemas = () => invoke("get_active_schemas");

// ─── Schema ───────────────────────────────────────────────────────────────────

export const getTables = (id) => invoke("get_tables", { id });
export const getColumns = (id, schema, table) =>
  invoke("get_columns", { id, schema, table });
export const getRelationships = (id) => invoke("get_relationships", { id });

// ─── Data ─────────────────────────────────────────────────────────────────────

export const getTableData = (id, schema, table, page, pageSize, filters) =>
  invoke("get_table_data", { id, schema, table, page, pageSize, filters });

export const executeQuery = (id, sql) => invoke("execute_query", { id, sql });
export const listSavedQueries = (connectionId = null) =>
  invoke("list_saved_queries", { connectionId });
export const saveSavedQuery = (query) => invoke("save_saved_query", { query });
export const deleteSavedQuery = (id) => invoke("delete_saved_query", { id });
export const listQueryHistory = (connectionId = null) =>
  invoke("list_query_history", { connectionId });
export const clearQueryHistory = (connectionId = null) =>
  invoke("clear_query_history", { connectionId });

// ─── Analytics / Dashboard ───────────────────────────────────────────────────

export const getGrandLivre = (id, dateFrom, dateTo, accountPrefix = null) =>
  invoke("get_grand_livre", { id, dateFrom, dateTo, accountPrefix });

export const getBalance = (id, dateFrom, dateTo, accountPrefix = null) =>
  invoke("get_balance", { id, dateFrom, dateTo, accountPrefix });

export const getGrandLivreAuxiliaire = (
  id,
  dateFrom,
  dateTo,
  tiersType,
  accountPrefix = null,
) => invoke("get_grand_livre_auxiliaire", {
  id,
  dateFrom,
  dateTo,
  tiersType,
  accountPrefix,
});

export const getDashboardKpis = (id, dateFrom, dateTo, accountPrefix = null) =>
  invoke("get_dashboard_kpis", { id, dateFrom, dateTo, accountPrefix });

export const fetchTableRows = async (
  id,
  schema,
  table,
  filters,
  { pageSize = 5000, maxRows = Number.POSITIVE_INFINITY } = {},
) => {
  const firstPage = await getTableData(id, schema, table, 0, pageSize, filters);
  const targetRows = Math.min(firstPage.total_count, maxRows);
  const rows = [...firstPage.rows];

  for (let page = 1; rows.length < targetRows; page += 1) {
    const batch = await getTableData(id, schema, table, page, pageSize, filters);
    if (!batch.rows.length) break;
    rows.push(...batch.rows);
  }

  return {
    columns: firstPage.columns,
    rows: rows.slice(0, targetRows),
    totalCount: firstPage.total_count,
  };
};

// ─── Export ───────────────────────────────────────────────────────────────────

export const exportToCsv = async (id, schema, table, filters, selectedColumns) => {
  const filePath = await save({
    defaultPath: `${table}.csv`,
    filters: [{ name: "CSV", extensions: ["csv"] }],
  });
  if (!filePath) return null;
  return invoke("export_csv", { id, schema, table, filters, selectedColumns, filePath });
};

export const exportToJson = async (id, schema, table, filters, selectedColumns) => {
  const filePath = await save({
    defaultPath: `${table}.json`,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (!filePath) return null;
  return invoke("export_json", { id, schema, table, filters, selectedColumns, filePath });
};

export const exportToSql = async (id, schema, table, filters) => {
  const filePath = await save({
    defaultPath: `${table}.sql`,
    filters: [{ name: "SQL", extensions: ["sql"] }],
  });
  if (!filePath) return null;
  return invoke("export_sql", { id, schema, table, filters, filePath });
};

export const exportToExcel = async (id, schema, table, filters, selectedColumns) => {
  const filePath = await save({
    defaultPath: `${table}.xlsx`,
    filters: [{ name: "Excel Workbook", extensions: ["xlsx"] }],
  });
  if (!filePath) return null;

  const { columns, rows } = await fetchTableRows(id, schema, table, filters);
  const selectedSet = new Set(selectedColumns);
  const exportIndexes = columns
    .map((column, index) => ({ column, index }))
    .filter(({ column }) => selectedSet.size === 0 || selectedSet.has(column.name));

  const headers = exportIndexes.map(({ column }) => column.name);
  const body = rows.map((row) => exportIndexes.map(({ index }) => row[index]));

  const worksheet = XLSX.utils.aoa_to_sheet([headers, ...body]);
  const workbook = XLSX.utils.book_new();
  const sheetName = `${schema}_${table}`.replace(/[\\/?*\[\]:]/g, "_").slice(0, 31) || "Export";
  const range = XLSX.utils.decode_range(worksheet["!ref"] || "A1");

  worksheet["!cols"] = headers.map((header, index) => {
    const sampleWidth = body.reduce((max, row) => {
      const value = row[index];
      const length = value == null ? 4 : String(value).length;
      return Math.max(max, length);
    }, header.length);
    return { wch: Math.min(Math.max(sampleWidth + 2, 10), 40) };
  });
  worksheet["!autofilter"] = {
    ref: XLSX.utils.encode_range({
      s: { r: 0, c: 0 },
      e: { r: range.e.r, c: range.e.c },
    }),
  };

  XLSX.utils.book_append_sheet(workbook, worksheet, sheetName);

  const contents = new Uint8Array(XLSX.write(workbook, { bookType: "xlsx", type: "array" }));
  await writeBinaryFile(filePath, contents);

  return filePath;
};
