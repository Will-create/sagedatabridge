import { invoke } from "@tauri-apps/api/tauri";
import { save } from "@tauri-apps/api/dialog";
import { writeBinaryFile } from "@tauri-apps/api/fs";
import * as XLSX from "xlsx";

// Re-export tauri invoke for easy mocking/testing
export { invoke };

const DASHBOARD_TIMEOUT_MS = 40_000;
const INVOICE_TIMEOUT_MS = 30_000;
const SEARCH_TIMEOUT_MS = 15_000;

function withTimeout(promise, timeoutMs, label) {
  let timeoutId;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timeoutId = globalThis.setTimeout(() => {
        reject(new Error(`${label} timed out after ${Math.round(timeoutMs / 1000)}s`));
      }, timeoutMs);
    }),
  ]).finally(() => {
    if (timeoutId) globalThis.clearTimeout(timeoutId);
  });
}

function invokeWithTimeout(command, payload, { timeoutMs, label }) {
  return withTimeout(invoke(command, payload), timeoutMs, label);
}

function logInvoiceRequest(scope, payload) {
  console.debug("[invoice]", scope, "request", payload);
}

function logInvoiceResponse(scope, meta) {
  console.debug("[invoice]", scope, "response", meta);
}

function logInvoiceError(scope, error, payload) {
  console.error("[invoice]", scope, "error", { error: String(error), payload });
}

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
  invokeWithTimeout(
    "get_grand_livre",
    { id, dateFrom, dateTo, accountPrefix },
    { timeoutMs: DASHBOARD_TIMEOUT_MS, label: "Grand livre" },
  );

export const getBalance = (id, dateFrom, dateTo, accountPrefix = null) =>
  invokeWithTimeout(
    "get_balance",
    { id, dateFrom, dateTo, accountPrefix },
    { timeoutMs: DASHBOARD_TIMEOUT_MS, label: "Balance" },
  );

export const getGrandLivreAuxiliaire = (
  id,
  dateFrom,
  dateTo,
  tiersType,
  accountPrefix = null,
) => invokeWithTimeout(
  "get_grand_livre_auxiliaire",
  {
    id,
    dateFrom,
    dateTo,
    tiersType,
    accountPrefix,
  },
  { timeoutMs: DASHBOARD_TIMEOUT_MS, label: "Auxiliary ledger" },
);

export const getDashboardKpis = (id, dateFrom, dateTo, accountPrefix = null) =>
  invokeWithTimeout(
    "get_dashboard_kpis",
    { id, dateFrom, dateTo, accountPrefix },
    { timeoutMs: DASHBOARD_TIMEOUT_MS, label: "Dashboard overview" },
  );

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

// ─── Invoicing ───────────────────────────────────────────────────────────────

export const listInvoices = (id, {
  dateFrom = null,
  dateTo = null,
  nature = null,
  statut = null,
  tiersId = null,
  search = null,
} = {}) => {
  const payload = {
    id,
    dateFrom,
    dateTo,
    nature,
    statut,
    tiersId,
    search,
  };
  logInvoiceRequest("listInvoices", payload);
  return invokeWithTimeout("list_invoices", payload, {
    timeoutMs: INVOICE_TIMEOUT_MS,
    label: "Invoice list",
  }).then((items) => {
    logInvoiceResponse("listInvoices", { status: "success", count: items.length });
    return items;
  }).catch((error) => {
    logInvoiceError("listInvoices", error, payload);
    throw error;
  });
};

export const getInvoice = (id, pieceId) => {
  const payload = { id, pieceId };
  logInvoiceRequest("getInvoice", payload);
  return invokeWithTimeout("get_invoice", payload, {
    timeoutMs: INVOICE_TIMEOUT_MS,
    label: "Invoice detail",
  }).then((item) => {
    logInvoiceResponse("getInvoice", { status: "success", pieceId: item?.id || pieceId });
    return item;
  }).catch((error) => {
    logInvoiceError("getInvoice", error, payload);
    throw error;
  });
};
export const createInvoice = (id, invoice) => invoke("create_invoice", { id, invoice });
export const updateInvoice = (id, invoice) => invoke("update_invoice", { id, invoice });

export const updateStatut = (id, pieceId, newStatut, updatedBy = "") => invoke("update_statut", {
  id,
  pieceId,
  newStatut,
  updatedBy,
});

export const deleteInvoice = (id, pieceId) => invoke("delete_invoice", { id, pieceId });
export const comptabiliserInvoice = (id, pieceId) => invoke("comptabiliser_invoice", { id, pieceId });

export const listTiers = (id, typeTiers = "all", search = null, { limit = 40 } = {}) =>
  invokeWithTimeout("list_tiers", {
    id,
    typeTiers,
    search,
    limit,
  }, {
    timeoutMs: SEARCH_TIMEOUT_MS,
    label: "Tiers search",
  });

export const searchTiers = (id, query, typeTiers = "all", { limit = 40 } = {}) => {
  const payload = { id, typeTiers, search: query || null, limit };
  logInvoiceRequest("searchTiers", payload);
  return invokeWithTimeout("list_tiers", payload, {
    timeoutMs: SEARCH_TIMEOUT_MS,
    label: "Tiers search",
  }).then((items) => {
    logInvoiceResponse("searchTiers", { status: "success", count: items.length });
    return items;
  }).catch((error) => {
    logInvoiceError("searchTiers", error, payload);
    throw error;
  });
};

const normalizeTiersPayload = (tiers = {}) => ({
  id: tiers.id ?? "",
  code: tiers.code ?? "",
  nom: tiers.nom ?? "",
  adresse: tiers.adresse ?? "",
  cp: tiers.cp ?? "",
  ville: tiers.ville ?? "",
  pays: tiers.pays ?? "",
  siret: tiers.siret ?? "",
  email: tiers.email ?? "",
  telephone: tiers.telephone ?? "",
  tva_intra: tiers.tva_intra ?? "",
  type_tiers: tiers.type_tiers || "client",
  encours: Number(tiers.encours) || 0,
  nb_factures: Number(tiers.nb_factures) || 0,
  is_local: Boolean(tiers.is_local),
});

export const createTiers = (id, tiers) => invoke("create_tiers", {
  id,
  tiers: normalizeTiersPayload(tiers),
});
export const updateTiers = (id, tiers) => invoke("update_tiers", {
  id,
  tiers: normalizeTiersPayload(tiers),
});
export const deleteTiers = (id, tiersId) => invoke("delete_tiers", { id, tiersId });

export const listArticles = (id, search = null, { limit = 40 } = {}) =>
  invokeWithTimeout("list_articles", { id, search, limit }, {
    timeoutMs: SEARCH_TIMEOUT_MS,
    label: "Article search",
  });

export const searchArticles = (id, query, { limit = 40 } = {}) => {
  const payload = { id, search: query || null, limit };
  logInvoiceRequest("searchArticles", payload);
  return invokeWithTimeout("list_articles", payload, {
    timeoutMs: SEARCH_TIMEOUT_MS,
    label: "Article search",
  }).then((items) => {
    logInvoiceResponse("searchArticles", { status: "success", count: items.length });
    return items;
  }).catch((error) => {
    logInvoiceError("searchArticles", error, payload);
    throw error;
  });
};

export const createArticle = (id, article) => invoke("create_article", { id, article });
export const updateArticle = (id, article) => invoke("update_article", { id, article });
export const deleteArticle = (id, articleId) => invoke("delete_article", { id, articleId });

export const renderInvoiceHtml = (invoice, templateId = "") => invoke("render_invoice_html", {
  invoice,
  templateId,
});

export const renderInvoiceHtmlPreview = (invoice, template) => invoke("render_invoice_html_preview", {
  invoice,
  template,
});

export const exportInvoicePdf = (invoice, templateId, filePath) => invoke("export_invoice_pdf", {
  invoice,
  templateId,
  filePath,
});

export const saveInvoiceTemplate = (connectionId, template) => invoke("save_invoice_template", {
  connectionId,
  template,
});

export const getInvoiceTemplates = (connectionId = null) => invoke("get_invoice_templates", {
  connectionId,
});
