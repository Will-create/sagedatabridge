import { invoke } from "@tauri-apps/api/tauri";
import { listen } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/api/dialog";
import { createInvokeWithTimeout, secondsToTimeoutMs, withTimeout } from "../utils/tauriTimeout";

// Re-export tauri invoke for easy mocking/testing
export { invoke };

const CONNECTION_TIMEOUT_MS = 75_000;
const METADATA_TIMEOUT_MS = 75_000;
const TABLE_DATA_TIMEOUT_MS = 90_000;
const DEFAULT_DASHBOARD_TIMEOUT_MS = 90_000;
const INVOICE_TIMEOUT_MS = 75_000;
const SEARCH_TIMEOUT_MS = 10_000;
const LARGE_EXPORT_TIMEOUT_MS = 24 * 60 * 60 * 1000;
export const DASHBOARD_PREVIEW_ROW_LIMIT = 5_000;
const invokeWithTimeout = createInvokeWithTimeout(invoke);
let dashboardTimeoutMs = DEFAULT_DASHBOARD_TIMEOUT_MS;

export function configureTauriTimeouts(settings = {}) {
  dashboardTimeoutMs = secondsToTimeoutMs(
    settings.dashboard_timeout_secs,
    DEFAULT_DASHBOARD_TIMEOUT_MS,
  );
}

function getDashboardTimeoutMs(timeoutMs = null) {
  return Number.isFinite(Number(timeoutMs)) && Number(timeoutMs) > 0
    ? Number(timeoutMs)
    : dashboardTimeoutMs;
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

export const getSettings = () => invoke("get_settings").then((settings) => {
  configureTauriTimeouts(settings);
  return settings;
});
export const saveSettings = (settings) => invoke("save_settings", { settings }).then((result) => {
  configureTauriTimeouts(settings);
  return result;
});

// ─── Field History ────────────────────────────────────────────────────────────

export const getFieldHistory = (field) => invoke("get_field_history", { field });
export const saveFieldHistory = (field, value) => invoke("save_field_history", { field, value });
export const removeFieldHistoryEntry = (field, value) => invoke("remove_field_history_entry", { field, value });

// ─── Connections ─────────────────────────────────────────────────────────────

export const listConnections = () => invoke("list_connections");
export const saveConnection = (connection) => invoke("save_connection", { connection });
export const deleteConnection = (id) => invoke("delete_connection", { id });
export const revealConnectionPassword = (connectionId, unlockPassword) =>
  invoke("reveal_connection_password", { connectionId, unlockPassword });
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

export const getAccountingJournals = (id) =>
  invokeWithTimeout("get_accounting_journals", { id }, {
    timeoutMs: TABLE_DATA_TIMEOUT_MS,
    label: "Load accounting journals",
  });

export const getAccountingJournalStats = (
  id,
  dateFrom,
  dateTo,
  journalType,
  journalCode,
) => invokeWithTimeout(
  "get_accounting_journal_stats",
  { id, dateFrom, dateTo, journalType, journalCode },
  { timeoutMs: getDashboardTimeoutMs(), label: "Load accounting journal statistics" },
);

export async function streamAccountingJournalEntries(
  id,
  requestId,
  dateFrom,
  dateTo,
  journalType,
  journalCode,
  handlers = {},
) {
  let streamError = null;
  const matches = (event) => event.payload?.request_id === requestId;
  const unlisten = await Promise.all([
    listen("journals_stream_total", (event) => {
      if (matches(event)) handlers.onTotal?.(event.payload);
    }),
    listen("journals_stream_chunk", (event) => {
      if (matches(event)) handlers.onChunk?.(event.payload?.rows ?? []);
    }),
    listen("journals_stream_complete", (event) => {
      if (matches(event)) handlers.onComplete?.(event.payload);
    }),
    listen("journals_stream_error", (event) => {
      if (!matches(event)) return;
      streamError = String(event.payload?.error ?? "Journal stream failed");
      handlers.onError?.(streamError);
    }),
  ]);
  try {
    await withTimeout(
      invoke("stream_accounting_journal_entries", {
        id,
        requestId,
        dateFrom,
        dateTo,
        journalType,
        journalCode,
      }),
      LARGE_EXPORT_TIMEOUT_MS,
      "Load accounting journal entries",
    );
    if (streamError) throw new Error(streamError);
  } finally {
    unlisten.forEach((stop) => stop());
  }
}

export async function exportAccountingJournalsXlsx(
  id,
  requestId,
  dateFrom,
  dateTo,
  journalType,
  journalCode,
  filePath,
  locale,
  handlers = {},
) {
  let exportError = null;
  const matches = (event) => event.payload?.request_id === requestId;
  const unlisten = await Promise.all([
    listen("journals_export_progress", (event) => {
      if (matches(event)) handlers.onProgress?.(event.payload);
    }),
    listen("journals_export_complete", (event) => {
      if (matches(event)) handlers.onComplete?.(event.payload);
    }),
    listen("journals_export_error", (event) => {
      if (!matches(event)) return;
      exportError = String(event.payload?.error ?? "Journal Excel export failed");
      handlers.onError?.(exportError);
    }),
  ]);
  try {
    const result = await invokeWithTimeout(
      "export_accounting_journals_xlsx",
      { id, requestId, dateFrom, dateTo, journalType, journalCode, filePath, locale },
      { timeoutMs: LARGE_EXPORT_TIMEOUT_MS, label: "Accounting journals Excel export" },
    );
    if (exportError) throw new Error(exportError);
    return result;
  } finally {
    unlisten.forEach((stop) => stop());
  }
}

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
    { timeoutMs: getDashboardTimeoutMs(), label: "Grand livre" },
  );

export const getGrandLivrePage = (
  id,
  dateFrom,
  dateTo,
  accountPrefix = null,
  offset = 0,
  limit = 500,
  search = "",
) =>
  invokeWithTimeout(
    "get_grand_livre_page",
    { id, dateFrom, dateTo, accountPrefix, offset, limit, search },
    { timeoutMs: getDashboardTimeoutMs(), label: "Grand livre page" },
  );

export const getBalance = (id, dateFrom, dateTo, accountPrefix = null) =>
  invokeWithTimeout(
    "get_balance",
    { id, dateFrom, dateTo, accountPrefix },
    { timeoutMs: getDashboardTimeoutMs(), label: "Balance" },
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
  { timeoutMs: getDashboardTimeoutMs(), label: "Auxiliary ledger" },
);

export const getDashboardKpis = (id, dateFrom, dateTo, accountPrefix = null) =>
  invokeWithTimeout(
    "get_dashboard_kpis",
    { id, dateFrom, dateTo, accountPrefix },
    { timeoutMs: getDashboardTimeoutMs(), label: "Dashboard overview" },
  );

export const getDashboardTiers = (id, tiersType = "all", search = "") =>
  invokeWithTimeout(
    "get_dashboard_tiers",
    { id, tiersType, search: search || null },
    { timeoutMs: getDashboardTimeoutMs(), label: "Dashboard tiers" },
  );

export const listExploitationReports = (id, year) =>
  invoke("list_exploitation_reports", { id, year });

export const getExploitationReport = (id, year, reportId = null) =>
  invoke("get_exploitation_report", { id, year, reportId });

export const saveExploitationReport = (id, year, report) =>
  invoke("save_exploitation_report", { id, year, report });

export const deleteExploitationReport = (id, year, reportId = null) =>
  invoke("delete_exploitation_report", { id, year, reportId });

export const setActiveExploitationReport = (id, year, reportId) =>
  invoke("set_active_exploitation_report", { id, year, reportId });

export const getExploitationMappings = (id) =>
  invoke("get_exploitation_mappings", { id });

export const saveExploitationMappings = (id, mappings) =>
  invoke("save_exploitation_mappings", { id, mappings });

export const resetExploitationMappings = (id) =>
  invoke("reset_exploitation_mappings", { id });

export const getCompteExploitationAccounts = (id, dateFrom, dateTo) =>
  invokeWithTimeout(
    "get_compte_exploitation_accounts",
    { id, dateFrom, dateTo },
    { timeoutMs: getDashboardTimeoutMs(), label: "Compte d'exploitation" },
  );

async function streamDashboardRows(command, payload, events, handlers = {}, options = {}) {
  const rows = [];
  const maxRetainedRows = Number.isFinite(Number(options.maxRetainedRows))
    ? Math.max(0, Number(options.maxRetainedRows))
    : Number.POSITIVE_INFINITY;
  let warning = null;
  let total = null;
  let loaded = 0;
  let streamError = null;

  const unlisten = await Promise.all([
    listen(events.total, (event) => {
      total = Number(event.payload?.total ?? total ?? 0);
      warning = event.payload?.warning ?? warning;
      handlers.onTotal?.({ total, warning });
    }),
    listen(events.chunk, (event) => {
      const chunk = Array.isArray(event.payload?.rows) ? event.payload.rows : [];
      loaded += chunk.length;
      if (rows.length < maxRetainedRows) {
        rows.push(...chunk.slice(0, maxRetainedRows - rows.length));
      }
      handlers.onRows?.(rows.slice(), {
        chunk,
        total,
        warning,
        loaded,
        retained: rows.length,
        limited: Number.isFinite(maxRetainedRows) && rows.length >= maxRetainedRows,
      });
    }),
    listen(events.complete, (event) => {
      warning = event.payload?.warning ?? warning;
      handlers.onComplete?.({
        rows: rows.slice(),
        total,
        warning,
        loaded,
        retained: rows.length,
        limited: total != null && rows.length < total,
      });
    }),
    listen(events.error, (event) => {
      streamError = String(event.payload ?? `${options.label || command} failed`);
      handlers.onError?.(streamError);
    }),
  ]);

  try {
    await withTimeout(
      invoke(command, payload),
      getDashboardTimeoutMs(options.timeoutMs),
      options.label || command,
    );
    if (streamError) throw new Error(streamError);
    return {
      data: rows.slice(),
      warning,
      total,
      loaded,
      retained: rows.length,
      limited: total != null && rows.length < total,
    };
  } finally {
    unlisten.forEach((stop) => stop());
  }
}

export const streamGrandLivre = (
  id,
  dateFrom,
  dateTo,
  accountPrefix = null,
  handlers = {},
  options = {},
) => streamDashboardRows(
  "stream_grand_livre",
  {
    id,
    dateFrom,
    dateTo,
    accountPrefix,
    previewLimit: options.previewLimit ?? null,
  },
  {
    total: "gl_total",
    chunk: "gl_chunk",
    complete: "gl_complete",
    error: "gl_error",
  },
  handlers,
  {
    maxRetainedRows: options.maxRetainedRows ?? Number.POSITIVE_INFINITY,
    ...options,
    label: options.label || "Grand livre",
  },
);

export const exportGrandLivreXlsx = async (
  id,
  dateFrom,
  dateTo,
  accountPrefix = null,
  filePath,
  handlers = {},
) => {
  let progressError = null;
  const unlisten = await Promise.all([
    listen("gl_export_progress", (event) => {
      handlers.onProgress?.(event.payload);
    }),
    listen("gl_export_complete", (event) => {
      handlers.onComplete?.(event.payload);
    }),
    listen("gl_export_error", (event) => {
      progressError = String(event.payload ?? "Grand Livre export failed");
      handlers.onError?.(progressError);
    }),
  ]);

  try {
    const result = await invokeWithTimeout(
      "export_grand_livre_xlsx",
      { id, dateFrom, dateTo, accountPrefix, filePath },
      {
        timeoutMs: Math.max(getDashboardTimeoutMs(), LARGE_EXPORT_TIMEOUT_MS),
        label: "Grand livre Excel export",
      },
    );
    if (progressError) throw new Error(progressError);
    return result;
  } finally {
    unlisten.forEach((stop) => stop());
  }
};

export const streamGrandLivreAuxiliaire = (
  id,
  dateFrom,
  dateTo,
  tiersType,
  accountPrefix = null,
  handlers = {},
  options = {},
) => streamDashboardRows(
  "stream_grand_livre_auxiliaire",
  {
    id,
    dateFrom,
    dateTo,
    tiersType,
    accountPrefix,
    previewLimit: options.previewLimit ?? null,
  },
  {
    total: "aux_total",
    chunk: "aux_chunk",
    complete: "aux_complete",
    error: "aux_error",
  },
  handlers,
  {
    maxRetainedRows: options.maxRetainedRows ?? Number.POSITIVE_INFINITY,
    ...options,
    label: options.label || "Auxiliary ledger",
  },
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
  return startTableExport({ connectionId: id, schema, table, filters, selectedColumns, format: "csv", filePath });
};

export const exportToJson = async (id, schema, table, filters, selectedColumns) => {
  const filePath = await save({
    defaultPath: `${table}.json`,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (!filePath) return null;
  return startTableExport({ connectionId: id, schema, table, filters, selectedColumns, format: "json", filePath });
};

export const exportToSql = async (id, schema, table, filters) => {
  const filePath = await save({
    defaultPath: `${table}.sql`,
    filters: [{ name: "SQL", extensions: ["sql"] }],
  });
  if (!filePath) return null;
  return startTableExport({ connectionId: id, schema, table, filters, selectedColumns: [], format: "sql", filePath });
};

export const exportToExcel = async (id, schema, table, filters, selectedColumns) => {
  const filePath = await save({
    defaultPath: `${table}.xlsx`,
    filters: [{ name: "Excel Workbook", extensions: ["xlsx"] }],
  });
  if (!filePath) return null;

  return startTableExport({ connectionId: id, schema, table, filters, selectedColumns, format: "xlsx", filePath });
};

export const startTableExport = (spec) => invoke("start_table_export", { spec });
export const listExportJobs = () => invoke("list_export_jobs");
export const cancelExportJob = (jobId) => invoke("cancel_export_job", { jobId });

// ─── Transfer, backup, and restore operations ─────────────────────────────

export const preflightTransfer = (spec) => invokeWithTimeout("preflight_transfer", { spec }, {
  timeoutMs: LARGE_EXPORT_TIMEOUT_MS,
  label: "Transfer preflight",
});
export const preflightBackup = (spec) => invokeWithTimeout("preflight_backup", { spec }, {
  timeoutMs: LARGE_EXPORT_TIMEOUT_MS,
  label: "Backup preflight",
});
export const preflightRestore = (spec) => invokeWithTimeout("preflight_restore", { spec }, {
  timeoutMs: LARGE_EXPORT_TIMEOUT_MS,
  label: "Restore preflight",
});
export const startTransfer = (spec) => invoke("start_transfer", { spec });
export const startBackup = (spec) => invoke("start_backup", { spec });
export const startRestore = (spec) => invoke("start_restore", { spec });
export const listOperationJobs = () => invoke("list_operation_jobs");
export const getOperationJob = (jobId) => invoke("get_operation_job", { jobId });
export const cancelOperationJob = (jobId) => invoke("cancel_operation_job", { jobId });
export const resumeOperationJob = (jobId) => invoke("resume_operation_job", { jobId });
export const listOperationTemplates = () => invoke("list_operation_templates");
export const saveOperationTemplate = (template) => invoke("save_operation_template", { template });
export const deleteOperationTemplate = (templateId) => invoke("delete_operation_template", { templateId });
export const exportOperationDiagnostic = (jobId, filePath) => invoke("export_operation_diagnostic", { jobId, filePath });
export const operationsListDatabases = (connectionId) => invokeWithTimeout("operations_list_databases", { connectionId }, {
  timeoutMs: METADATA_TIMEOUT_MS,
  label: "List operation databases",
});
export const operationsListTables = (connectionId, database) => invokeWithTimeout("operations_list_tables", { connectionId, database }, {
  timeoutMs: METADATA_TIMEOUT_MS,
  label: "List operation tables",
});
export const previewTransferTable = (connectionId, database, schema, table) => invokeWithTimeout("preview_transfer_table", { connectionId, database, schema, table }, {
  timeoutMs: TABLE_DATA_TIMEOUT_MS,
  label: "Preview transfer table",
});
export const listSqlServerPaths = (connectionId, directory = null) => invokeWithTimeout("list_sql_server_paths", { connectionId, directory }, {
  timeoutMs: METADATA_TIMEOUT_MS,
  label: "List SQL Server paths",
});
export const inspectBackup = (connectionId, backupPath, extraPaths = []) => invokeWithTimeout("inspect_backup", { connectionId, backupPath, extraPaths }, {
  timeoutMs: LARGE_EXPORT_TIMEOUT_MS,
  label: "Inspect backup",
});
export const previewBackupStage = (connectionId, sourcePath) => invokeWithTimeout("preview_backup_stage", { connectionId, sourcePath }, {
  timeoutMs: METADATA_TIMEOUT_MS,
  label: "Preview backup staging",
});
export const confirmBackupStage = (connectionId, sourcePath) => invokeWithTimeout("confirm_backup_stage", { connectionId, sourcePath, confirmed: true }, {
  timeoutMs: LARGE_EXPORT_TIMEOUT_MS,
  label: "Copy backup for SQL Server",
});
export const sqlHostIsLocal = (connectionId) => invoke("sql_host_is_local", { connectionId });

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
export const markInvoiceComptabiliseFromBridge = (id, pieceId, bridgeDocumentId) => runInvoiceMutation(
  "markInvoiceComptabiliseFromBridge",
  "mark_invoice_comptabilise_from_bridge",
  { id, pieceId, bridgeDocumentId },
  "Mark invoice posted from bridge",
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
  product_family_code: article.product_family_code ?? "",
  accounting_category_code: article.accounting_category_code ?? "",
  tax_code: article.tax_code ?? "",
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

// Normalized commercial-to-accounting bridge. These commands never write to
// native Sage commercial or accounting tables.
export const initializeBridge = (id) => invoke("initialize_bridge", { id });
export const listBridgeDocuments = (id, documentType = null) =>
  invoke("list_bridge_documents", { id, documentType });
export const createBridgeDocument = (id, invoice) =>
  invoke("create_bridge_document", { id, invoice });
export const validateBridgeDocument = (id, documentId, validatedBy = "SDB") =>
  invoke("validate_bridge_document", { id, documentId, validatedBy });
export const transformBridgeQuote = (id, quoteId) =>
  invoke("transform_bridge_quote", { id, quoteId });
export const generateBridgeInvoicePosting = (id, documentId) =>
  invoke("generate_bridge_invoice_posting", { id, documentId });
export const registerBridgePayment = (id, payment) =>
  invoke("register_bridge_payment", { id, payment });
export const generateBridgePaymentPosting = (id, paymentId) =>
  invoke("generate_bridge_payment_posting", { id, paymentId });
export const getBridgeAccountingConfiguration = (id) =>
  invoke("get_bridge_accounting_configuration", { id });
export const saveBridgeAccountingConfiguration = (id, configuration) =>
  invoke("save_bridge_accounting_configuration", { id, configuration });
export const getBridgeMasterData = (id) =>
  invoke("get_bridge_master_data", { id });
export const saveBridgeProductProfile = (id, profile) =>
  invoke("save_bridge_product_profile", { id, profile });
export const getBridgeManagementData = (id) =>
  invoke("get_bridge_management_data", { id });
export const saveBridgeManagementRecord = (id, entity, record) =>
  invoke("save_bridge_management_record", { id, entity, record });
export const deactivateBridgeManagementRecord = (id, entity, key) =>
  invoke("deactivate_bridge_management_record", { id, entity, key });
export const previewBridgeAccounting = (id, invoice) =>
  invoke("preview_bridge_accounting", { id, invoice });
export const listBridgePostingBatches = (id) =>
  invoke("list_bridge_posting_batches", { id });
export const prepareBridgeExport = (id, batchId, adapterType = "neutral") =>
  invoke("prepare_bridge_export", { id, batchId, adapterType });
