import { invoke } from "@tauri-apps/api/tauri";
import { save } from "@tauri-apps/api/dialog";
import { writeBinaryFile } from "@tauri-apps/api/fs";
import * as XLSX from "xlsx";
import { createInvokeWithTimeout, withTimeout } from "../utils/tauriTimeout";

// Re-export tauri invoke for easy mocking/testing
export { invoke };

const CONNECTION_TIMEOUT_MS = 75_000;
const METADATA_TIMEOUT_MS = 75_000;
const TABLE_DATA_TIMEOUT_MS = 90_000;
const DASHBOARD_TIMEOUT_MS = 100_000;
const INVOICE_TIMEOUT_MS = 75_000;
const SEARCH_TIMEOUT_MS = 10_000;
const EXPORT_TIMEOUT_MS = 180_000;
const invokeWithTimeout = createInvokeWithTimeout(invoke);

function logInvoiceRequest(scope, payload) {
  console.debug("[invoice]", scope, "request", payload);
}

function logInvoiceResponse(scope, meta) {
  console.debug("[invoice]", scope, "response", meta);
}

function logInvoiceError(scope, error, payload) {
  console.error("[invoice]", scope, "error", { error: String(error), payload });
}

function runInvoiceMutation(scope, command, payload, label) {
  logInvoiceRequest(scope, payload);
  return invokeWithTimeout(command, payload, {
    timeoutMs: INVOICE_TIMEOUT_MS,
    label,
  }).then((result) => {
    logInvoiceResponse(scope, { status: "success" });
    return result;
  }).catch((error) => {
    logInvoiceError(scope, error, payload);
    throw error;
  });
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

// ─── Settings ───────────────────────────────────────────────────────────────────

export const getSettings = () => invoke("get_settings");
export const saveSettings = (settings) => invoke("save_settings", { settings });

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
export const connectDb = (id) => invokeWithTimeout("connect_db", { id }, {
  timeoutMs: CONNECTION_TIMEOUT_MS,
  label: "Connect database",
});
export const getDatabases = (connectionId) => invokeWithTimeout("get_databases", { connectionId }, {
  timeoutMs: METADATA_TIMEOUT_MS,
  label: "Load databases",
});
export const switchDatabase = (connectionId, database) =>
  invokeWithTimeout("switch_database", { connectionId, database }, {
    timeoutMs: CONNECTION_TIMEOUT_MS,
    label: "Switch database",
  });
export const openDatabaseWorkspace = (baseConnectionId, database) =>
  invokeWithTimeout("open_database_workspace", { baseConnectionId, database }, {
    timeoutMs: CONNECTION_TIMEOUT_MS,
    label: "Open database workspace",
  });
export const closeDatabaseWorkspace = (workspaceId) =>
  invoke("close_database_workspace", { workspaceId });
export const runConnectionDiagnostics = (id) =>
  invokeWithTimeout("run_connection_diagnostics", { id }, {
    timeoutMs: CONNECTION_TIMEOUT_MS,
    label: "Run connection diagnostics",
  });
export const disconnectDb = (id) => invoke("disconnect_db", { id });
export const getConnectionStatuses = () => invoke("get_connection_statuses");
export const detectSageEdition = (connectionId) => invoke("detect_sage_edition", { connectionId });
export const scanNetworkForSqlServers = () => invoke("scan_network_for_sql_servers");
export const testMapping = (connection, schema) => invoke("test_mapping", { connection, schema });
export const getActiveSchema = (connectionId) => invoke("get_active_schema", { connectionId });
export const getActiveSchemas = () => invoke("get_active_schemas");

// ─── Schema ───────────────────────────────────────────────────────────────────

export const getTables = (id) => invokeWithTimeout("get_tables", { id }, {
  timeoutMs: METADATA_TIMEOUT_MS,
  label: "Load tables",
});
export const getColumns = (id, schema, table) =>
  invokeWithTimeout("get_columns", { id, schema, table }, {
    timeoutMs: METADATA_TIMEOUT_MS,
    label: `Load columns for ${schema}.${table}`,
  });
export const getRelationships = (id) => invokeWithTimeout("get_relationships", { id }, {
  timeoutMs: METADATA_TIMEOUT_MS,
  label: "Load relationships",
});

// ─── Data ─────────────────────────────────────────────────────────────────────

export const getTableData = (id, schema, table, page, pageSize, filters) =>
  invokeWithTimeout("get_table_data", { id, schema, table, page, pageSize, filters }, {
    timeoutMs: TABLE_DATA_TIMEOUT_MS,
    label: `Load rows for ${schema}.${table}`,
  });

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
  return invokeWithTimeout("export_csv", { id, schema, table, filters, selectedColumns, filePath }, {
    timeoutMs: EXPORT_TIMEOUT_MS,
    label: `Export CSV for ${schema}.${table}`,
  });
};

export const exportToJson = async (id, schema, table, filters, selectedColumns) => {
  const filePath = await save({
    defaultPath: `${table}.json`,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (!filePath) return null;
  return invokeWithTimeout("export_json", { id, schema, table, filters, selectedColumns, filePath }, {
    timeoutMs: EXPORT_TIMEOUT_MS,
    label: `Export JSON for ${schema}.${table}`,
  });
};

export const exportToSql = async (id, schema, table, filters) => {
  const filePath = await save({
    defaultPath: `${table}.sql`,
    filters: [{ name: "SQL", extensions: ["sql"] }],
  });
  if (!filePath) return null;
  return invokeWithTimeout("export_sql", { id, schema, table, filters, filePath }, {
    timeoutMs: EXPORT_TIMEOUT_MS,
    label: `Export SQL for ${schema}.${table}`,
  });
};

export const exportToExcel = async (id, schema, table, filters, selectedColumns) => {
  const filePath = await save({
    defaultPath: `${table}.xlsx`,
    filters: [{ name: "Excel Workbook", extensions: ["xlsx"] }],
  });
  if (!filePath) return null;

  return withTimeout((async () => {
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
  })(), EXPORT_TIMEOUT_MS, `Export Excel for ${schema}.${table}`);
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
export const createInvoice = (id, invoice) => runInvoiceMutation(
  "createInvoice",
  "create_invoice",
  { id, invoice },
  "Create invoice",
);
export const updateInvoice = (id, invoice) => runInvoiceMutation(
  "updateInvoice",
  "update_invoice",
  { id, invoice },
  "Update invoice",
);

export const updateStatut = (id, pieceId, newStatut, updatedBy = "") => runInvoiceMutation(
  "updateStatut",
  "update_statut",
  {
    id,
    pieceId,
    newStatut,
    updatedBy,
  },
  "Update invoice status",
);

export const deleteInvoice = (id, pieceId) => runInvoiceMutation(
  "deleteInvoice",
  "delete_invoice",
  { id, pieceId },
  "Delete invoice",
);
export const comptabiliserInvoice = (id, pieceId) => runInvoiceMutation(
  "comptabiliserInvoice",
  "comptabiliser_invoice",
  { id, pieceId },
  "Post invoice",
);

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

const normalizeArticlePayload = (article = {}) => ({
  id: article.id ?? "",
  code: article.code ?? "",
  libelle: article.libelle ?? "",
  description: article.description ?? "",
  prix_ht: Number(article.prix_ht) || 0,
  currency: (article.currency ?? article.devise ?? "").toString().toUpperCase(),
  taux_tva: Number(article.taux_tva) || 0,
  taux_bic: Number(article.taux_bic) || 0,
  unite: article.unite ?? "",
  reference: article.reference ?? "",
  category: article.category ?? "",
  en_activite: article.en_activite !== false,
  revenue_account: article.revenue_account ?? "",
  expense_account: article.expense_account ?? "",
  vat_account: article.vat_account ?? "",
  bic_account: article.bic_account ?? "",
  tax_exempt: Boolean(article.tax_exempt),
  custom_tax_rules: article.custom_tax_rules ?? "",
});

export const createArticle = (id, article) => invoke("create_article", { id, article: normalizeArticlePayload(article) });
export const updateArticle = (id, article) => invoke("update_article", { id, article: normalizeArticlePayload(article) });
export const deleteArticle = (id, articleId) => invoke("delete_article", { id, articleId });

export const renderInvoiceHtml = (invoice, templateId = "", locale = "fr") => invoke("render_invoice_html", {
  invoice,
  templateId,
  locale,
});

export const renderInvoiceHtmlPreview = (invoice, template, locale = "fr") => invoke("render_invoice_html_preview", {
  invoice,
  template,
  locale,
});

export const exportInvoicePdf = (invoice, templateId, filePath, locale = "fr") => invoke("export_invoice_pdf", {
  invoice,
  templateId,
  filePath,
  locale,
});

export const saveInvoiceTemplate = (connectionId, template) => invoke("save_invoice_template", {
  connectionId,
  template,
});

export const getInvoiceTemplates = (connectionId = null) => invoke("get_invoice_templates", {
  connectionId,
});
