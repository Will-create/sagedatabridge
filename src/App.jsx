import { useCallback, useEffect, useMemo, useState } from "react";
import packageJson from "../package.json";
import {
  connectDb,
  getActiveSchemas,
  getColumns,
  getDatabases,
  getTableData,
  getTables,
  listConnections,
  closeDatabaseWorkspace,
  getSettings,
  openDatabaseWorkspace,
  runConnectionDiagnostics,
} from "./hooks/useTauri";
import { useT } from "./i18n";

import Dashboard from "./components/Dashboard";
import AboutPage from "./components/AboutPage";
import DataGrid from "./components/DataGrid";
import FilterBar from "./components/FilterBar";
import Invoicing from "./components/Invoicing";
import JournalsView from "./components/Journals";
import OperationsWorkspace from "./components/OperationsWorkspace";
import TemplateDesigner from "./components/Invoicing/TemplateDesigner";
import LockScreen from "./components/LockScreen";
import SettingsDrawer from "./components/SettingsDrawer";
import Sidebar from "./components/Sidebar";
import StatusBar from "./components/StatusBar";
import TablePanel from "./components/TablePanel";
import ThemeToggle from "./components/ThemeToggle";
import Toast from "./components/Toast";
import Toolbar from "./components/Toolbar";
import ExportProgressCenter from "./components/ExportProgressCenter";
import UpdatePrompter from "./components/UpdatePrompter";

const ADMIN_MODE_KEY = "sdb_admin_mode";
const SHOW_TABLE_PANEL_KEY = "sdb_show_table_panel";
const SHOW_TOOLBAR_KEY = "sdb_show_toolbar";

function readBooleanStorage(key, fallback) {
  const value = localStorage.getItem(key);
  if (value == null) return fallback;
  return value === "true";
}

function connectionDisplayName(connection, database = connection?.database) {
  const host = connection?.host?.trim() || "SQL Server";
  return connection?.name?.trim() || (database ? `${host} / ${database}` : host);
}

function upsertConnection(currentConnections, savedConnection) {
  const index = currentConnections.findIndex((connection) => connection.id === savedConnection.id);
  if (index === -1) return [...currentConnections, savedConnection];

  const nextConnections = [...currentConnections];
  nextConnections[index] = savedConnection;
  return nextConnections;
}

function isWorkspaceId(id) {
  return typeof id === "string" && id.startsWith("workspace:");
}

function emptyWorkspaceState() {
  return {
    tables: [],
    activeTable: null,
    columns: [],
    data: null,
    page: 0,
    pageSize: 100,
    filters: [],
    mainView: "dashboard",
    invoicingOpen: false,
  };
}

export default function App() {
  const { t, lang } = useT();

  const [unlocked, setUnlocked] = useState(false);

  const [connections, setConnections] = useState([]);
  const [statuses, setStatuses] = useState({});
  const [activeSchemas, setActiveSchemas] = useState({});
  const [activeConnId, setActiveConnId] = useState(null);
  const [activeDatabase, setActiveDatabase] = useState("");
  const [workspaces, setWorkspaces] = useState([]);
  const [workspaceStates, setWorkspaceStates] = useState({});
  const [databases, setDatabases] = useState([]);
  const [databasesLoading, setDatabasesLoading] = useState(false);
  const [globalLoading, setGlobalLoading] = useState(false);

  const [tables, setTables] = useState([]);
  const [tablesLoading, setTablesLoading] = useState(false);
  const [activeTable, setActiveTable] = useState(null);

  const [columns, setColumns] = useState([]);
  const [data, setData] = useState(null);
  const [dataLoading, setDataLoading] = useState(false);
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState(100);
  const [filters, setFilters] = useState([]);

  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [tablePanelCollapsed, setTablePanelCollapsed] = useState(false);
  const [mainView, setMainView] = useState("welcome");
  const [aboutReturnView, setAboutReturnView] = useState("welcome");
  const [invoicingOpen, setInvoicingOpen] = useState(false);

  const [adminMode, setAdminMode] = useState(() => readBooleanStorage(ADMIN_MODE_KEY, false));
  const [showTablePanel, setShowTablePanel] = useState(() => readBooleanStorage(SHOW_TABLE_PANEL_KEY, true));
  const [showToolbar, setShowToolbar] = useState(() => readBooleanStorage(SHOW_TOOLBAR_KEY, true));
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [templateDesignerOpen, setTemplateDesignerOpen] = useState(false);

  const [statusMsg, setStatusMsg] = useState({ message: "", type: "idle" });
  const [elapsed, setElapsed] = useState(null);
  const [toast, setToast] = useState(null);

  const activeWorkspace = useMemo(
    () => workspaces.find((workspace) => workspace.id === activeConnId) ?? null,
    [activeConnId, workspaces],
  );

  const activeConnection = useMemo(
    () => activeWorkspace ?? connections.find((connection) => connection.id === activeConnId) ?? null,
    [activeConnId, activeWorkspace, connections],
  );

  const showStatus = useCallback((message, type = "idle", ms = null) => {
    setStatusMsg({ message, type });
    setElapsed(ms ?? null);
  }, []);

  const showToast = useCallback((nextToast) => setToast(nextToast), []);

  const snapshotActiveWorkspace = useCallback(() => {
    if (!isWorkspaceId(activeConnId)) return;
    setWorkspaceStates((current) => ({
      ...current,
      [activeConnId]: {
        tables,
        activeTable,
        columns,
        data,
        page,
        pageSize,
        filters,
        mainView,
        invoicingOpen: adminMode && invoicingOpen,
      },
    }));
  }, [
    activeConnId,
    activeTable,
    columns,
    data,
    filters,
    adminMode,
    invoicingOpen,
    mainView,
    page,
    pageSize,
    tables,
  ]);

  useEffect(() => {
    if (!isWorkspaceId(activeConnId)) return;
    setWorkspaceStates((current) => {
      const previous = current[activeConnId];
      if (
        previous?.tables === tables
        && previous?.activeTable === activeTable
        && previous?.columns === columns
        && previous?.data === data
        && previous?.page === page
        && previous?.pageSize === pageSize
        && previous?.filters === filters
        && previous?.mainView === mainView
        && previous?.invoicingOpen === (adminMode && invoicingOpen)
      ) {
        return current;
      }
      return {
        ...current,
        [activeConnId]: {
          tables,
          activeTable,
          columns,
          data,
          page,
          pageSize,
          filters,
          mainView,
          invoicingOpen: adminMode && invoicingOpen,
        },
      };
    });
  }, [activeConnId, activeTable, adminMode, columns, data, filters, invoicingOpen, mainView, page, pageSize, tables]);

  const restoreWorkspaceState = useCallback((workspaceId) => {
    const workspace = workspaces.find((item) => item.id === workspaceId);
    const state = workspaceStates[workspaceId] ?? emptyWorkspaceState();
    setActiveConnId(workspaceId);
    setActiveDatabase(workspace?.database ?? "");
    setTables(state.tables ?? []);
    setActiveTable(state.activeTable ?? null);
    setColumns(state.columns ?? []);
    setData(state.data ?? null);
    setPage(state.page ?? 0);
    setPageSize(state.pageSize ?? 100);
    setFilters(state.filters ?? []);
    setMainView(state.mainView ?? "dashboard");
    setInvoicingOpen(adminMode && Boolean(state.invoicingOpen));
    setTemplateDesignerOpen(false);
  }, [adminMode, workspaceStates, workspaces]);

  useEffect(() => {
    localStorage.setItem(ADMIN_MODE_KEY, String(adminMode));
  }, [adminMode]);

  useEffect(() => {
    localStorage.setItem(SHOW_TABLE_PANEL_KEY, String(showTablePanel));
  }, [showTablePanel]);

  useEffect(() => {
    localStorage.setItem(SHOW_TOOLBAR_KEY, String(showToolbar));
  }, [showToolbar]);

  useEffect(() => {
    getSettings().catch(() => {});
  }, []);

  useEffect(() => {
    if (!activeConnId) {
      setMainView((current) => (current === "about" ? current : "welcome"));
      setActiveDatabase((current) => (current ? "" : current));
      setDatabases((current) => (current.length ? [] : current));
      setInvoicingOpen(false);
      setTemplateDesignerOpen(false);
    }
  }, [activeConnId]);

  useEffect(() => {
    if (!adminMode) {
      setTablePanelCollapsed(false);
      setInvoicingOpen(false);
      setTemplateDesignerOpen(false);
      if (activeConnId && activeDatabase && mainView === "tables") {
        setMainView("dashboard");
      }
    }
  }, [adminMode, activeConnId, activeDatabase, mainView]);

  useEffect(() => {
    if (unlocked) {
      setAdminMode(false);
    }
  }, [unlocked]);

  const loadConnections = useCallback(async () => {
    try {
      setConnections(await listConnections());
    } catch (err) {
      console.error("Failed to load connections", err);
    }
  }, []);

  useEffect(() => {
    if (unlocked) loadConnections();
  }, [unlocked, loadConnections]);

  const loadTableData = useCallback(async (connId, table, pg, size, nextFilters) => {
    if (!connId || !table) return;
    setDataLoading(true);
    const t0 = Date.now();
    try {
      showStatus(t("status_querying", `${table.schema}.${table.name}`), "idle");
      const result = await getTableData(connId, table.schema, table.name, pg, size, nextFilters);
      setData(result);
      showStatus(t("status_rows_shown", result.total_count, result.rows.length), "success", Date.now() - t0);
    } catch (err) {
      showStatus(t("status_query_failed") + err, "error");
      showToast({ type: "error", msg: String(err) });
    } finally {
      setDataLoading(false);
    }
  }, [t]);

  const ensureTablesLoaded = useCallback(async (connId) => {
    if (!connId) return [];
    setTablesLoading(true);
    try {
      const nextTables = await getTables(connId);
      setTables(nextTables);
      return nextTables;
    } finally {
      setTablesLoading(false);
    }
  }, []);

  const handleSelectConn = useCallback(async (id, connectionOverride = null, forceReconnect = false) => {
    if (isWorkspaceId(id)) {
      snapshotActiveWorkspace();
      restoreWorkspaceState(id);
      return;
    }

    if (!id) {
      snapshotActiveWorkspace();
      setActiveConnId(null);
      setActiveDatabase("");
      setTables([]);
      setActiveTable(null);
      setData(null);
      setColumns([]);
      setFilters([]);
      setInvoicingOpen(false);
      setTemplateDesignerOpen(false);
      return;
    }

    if (!forceReconnect && id === activeConnId && statuses[id] === "connected") {
      setInvoicingOpen(false);
      setTemplateDesignerOpen(false);
      setMainView(activeDatabase ? (adminMode ? "tables" : "dashboard") : "database-selection");
      return;
    }

    snapshotActiveWorkspace();
    const nextConnection = connectionOverride ?? connections.find((connection) => connection.id === id) ?? null;
    setActiveConnId(id);
    setActiveDatabase("");
    setDatabases([]);
    setTables([]);
    setActiveTable(null);
    setData(null);
    setColumns([]);
    setFilters([]);
    setMainView("welcome");
    setInvoicingOpen(false);
    setTemplateDesignerOpen(false);
    showStatus(t("status_connecting"), "idle");
    setStatuses((current) => ({ ...current, [id]: "connecting" }));
    setGlobalLoading(true);

    try {
      await connectDb(id);
      setStatuses((current) => ({ ...current, [id]: "connected" }));
      getActiveSchemas().then(setActiveSchemas).catch(() => {});

      const preferredDatabase = (nextConnection?.database || "").trim();
      setDatabasesLoading(true);
      const nextDatabases = await getDatabases(id);
      setDatabases(nextDatabases);
      setMainView("database-selection");
      showStatus(t("status_databases", nextDatabases.length), "success");

    } catch (err) {
      setStatuses((current) => ({ ...current, [id]: "error" }));
      showStatus(t("status_conn_failed") + err, "error");
      showToast({ type: "error", msg: String(err) });
    } finally {
      setDatabasesLoading(false);
      setGlobalLoading(false);
    }
  }, [
    activeConnId,
    activeDatabase,
    adminMode,
    connections,
    restoreWorkspaceState,
    snapshotActiveWorkspace,
    statuses,
    t,
  ]);

  const handleRefreshConnection = useCallback(async (id, connectionOverride = null) => {
    await handleSelectConn(id, connectionOverride, true);
  }, [handleSelectConn]);

  const handleRunDiagnostics = useCallback(async (id) => {
    if (!id) return;
    setGlobalLoading(true);
    try {
      const report = await runConnectionDiagnostics(id);
      const failed = report.checks.filter((check) => check.status === "error");
      const warnings = report.checks.filter((check) => check.status === "warning");
      const summary = failed.length
        ? `${failed.length} diagnostic error(s): ${failed.map((check) => check.name).join(", ")}`
        : warnings.length
          ? `${warnings.length} diagnostic warning(s): ${warnings.map((check) => check.name).join(", ")}`
          : `Diagnostics OK: ${report.database}`;
      showToast({ type: failed.length ? "error" : warnings.length ? "warning" : "success", msg: summary });
      console.table(report.checks);
    } catch (err) {
      showToast({ type: "error", msg: String(err) });
    } finally {
      setGlobalLoading(false);
    }
  }, [showToast]);

  const handleSelectDatabase = useCallback(async (database) => {
    if (!activeConnId || !database) return;
    if (database === activeDatabase) {
      setInvoicingOpen(false);
      setTemplateDesignerOpen(false);
      setMainView(adminMode ? "tables" : "dashboard");
      return;
    }

    setGlobalLoading(true);
    try {
      snapshotActiveWorkspace();
      setInvoicingOpen(false);
      setTemplateDesignerOpen(false);
      const baseConnectionId = isWorkspaceId(activeConnId)
        ? activeWorkspace?.baseConnectionId
        : activeConnId;
      if (!baseConnectionId) throw new Error("Base connection not found");

      const existingWorkspace = workspaces.find(
        (item) => item.baseConnectionId === baseConnectionId && item.database === database,
      );
      if (existingWorkspace) {
        restoreWorkspaceState(existingWorkspace.id);
        showStatus(t("status_database_ready", existingWorkspace.database, tables.length), "success");
        return;
      }

      const workspace = await openDatabaseWorkspace(baseConnectionId, database);
      setWorkspaces((current) => {
        if (current.some((item) => item.id === workspace.id)) return current;
        return [...current, workspace];
      });
      setStatuses((current) => ({ ...current, [workspace.id]: "connected" }));
      setActiveSchemas((current) => ({ ...current, [workspace.id]: workspace.schema }));
      setActiveConnId(workspace.id);
      setActiveDatabase(workspace.database);
      setWorkspaceStates((current) => ({
        ...current,
        [workspace.id]: {
          ...emptyWorkspaceState(),
          mainView: adminMode ? "tables" : "dashboard",
        },
      }));

      const nextTables = await ensureTablesLoaded(workspace.id);
      setTablePanelCollapsed(false);
      setMainView(adminMode ? "tables" : "dashboard");
      showStatus(t("status_database_ready", workspace.database, nextTables.length), "success");
    } catch (error) {
      showStatus(t("status_database_failed") + error, "error");
      showToast({ type: "error", msg: String(error) });
    } finally {
      setGlobalLoading(false);
    }
  }, [
    activeConnId,
    activeDatabase,
    activeWorkspace,
    adminMode,
    ensureTablesLoaded,
    restoreWorkspaceState,
    showStatus,
    showToast,
    snapshotActiveWorkspace,
    tables.length,
    t,
    workspaces,
  ]);

  const handleDuplicateWithDatabase = useCallback(async (baseConnection, database) => {
    if (!baseConnection || !database) return;

    setGlobalLoading(true);
    showStatus(t("status_switching_database", database), "idle");

    try {
      snapshotActiveWorkspace();
      setInvoicingOpen(false);
      setTemplateDesignerOpen(false);
      const workspace = await openDatabaseWorkspace(baseConnection.id, database);
      setWorkspaces((current) => [...current, workspace]);
      setStatuses((current) => ({ ...current, [workspace.id]: "connected" }));
      setActiveSchemas((current) => ({ ...current, [workspace.id]: workspace.schema }));
      setActiveConnId(workspace.id);
      setActiveDatabase(workspace.database);
      setWorkspaceStates((current) => ({
        ...current,
        [workspace.id]: {
          ...emptyWorkspaceState(),
          mainView: adminMode ? "tables" : "dashboard",
        },
      }));
      const nextTables = await ensureTablesLoaded(workspace.id);
      setMainView(adminMode ? "tables" : "dashboard");
      showStatus(t("status_database_ready", workspace.database, nextTables.length), "success");
    } catch (err) {
      showStatus(t("status_database_failed") + err, "error");
      showToast({ type: "error", msg: String(err) });
    } finally {
      setGlobalLoading(false);
    }
  }, [adminMode, ensureTablesLoaded, showStatus, showToast, snapshotActiveWorkspace, t]);

  const handleSelectTable = useCallback(async (table) => {
    setActiveTable(table);
    setFilters([]);
    setPage(0);
    setData(null);
    try {
      setColumns(await getColumns(activeConnId, table.schema, table.name));
    } catch {
      setColumns([]);
    }
    await loadTableData(activeConnId, table, 0, pageSize, []);
  }, [activeConnId, loadTableData, pageSize]);

  const handleFiltersChange = useCallback((nextFilters) => {
    setFilters(nextFilters);
    setPage(0);
    if (activeTable && activeConnId) {
      loadTableData(activeConnId, activeTable, 0, pageSize, nextFilters);
    }
  }, [activeConnId, activeTable, loadTableData, pageSize]);

  const handlePageChange = useCallback((nextPage) => {
    setPage(nextPage);
    loadTableData(activeConnId, activeTable, nextPage, pageSize, filters);
  }, [activeConnId, activeTable, filters, loadTableData, pageSize]);

  const handlePageSizeChange = useCallback((nextSize) => {
    setPageSize(nextSize);
    setPage(0);
    loadTableData(activeConnId, activeTable, 0, nextSize, filters);
  }, [activeConnId, activeTable, filters, loadTableData]);

  const handleRefresh = useCallback(() => {
    if (activeConnId && activeTable) {
      loadTableData(activeConnId, activeTable, page, pageSize, filters);
    }
  }, [activeConnId, activeTable, filters, loadTableData, page, pageSize]);

  const handleCloseWorkspace = useCallback(async (workspaceId) => {
    const index = workspaces.findIndex((workspace) => workspace.id === workspaceId);
    if (index === -1) return;

    try {
      await closeDatabaseWorkspace(workspaceId);
    } catch (err) {
      showToast({ type: "error", msg: String(err) });
    }

    setWorkspaces((current) => current.filter((workspace) => workspace.id !== workspaceId));
    setWorkspaceStates((current) => {
      const next = { ...current };
      delete next[workspaceId];
      return next;
    });
    setStatuses((current) => {
      const next = { ...current };
      delete next[workspaceId];
      return next;
    });
    setActiveSchemas((current) => {
      const next = { ...current };
      delete next[workspaceId];
      return next;
    });

    if (activeConnId === workspaceId) {
      const nextWorkspace = workspaces[index + 1] ?? workspaces[index - 1] ?? null;
      if (nextWorkspace) {
        restoreWorkspaceState(nextWorkspace.id);
      } else {
        setActiveConnId(null);
        setActiveDatabase("");
        setTables([]);
        setActiveTable(null);
        setData(null);
        setColumns([]);
        setFilters([]);
        setMainView("welcome");
        setInvoicingOpen(false);
        setTemplateDesignerOpen(false);
      }
    }
  }, [activeConnId, restoreWorkspaceState, showToast, workspaces]);

  const handleBackToTables = useCallback(async () => {
    if (!activeConnId || !activeDatabase) return;
    if (!tables.length) await ensureTablesLoaded(activeConnId);
    setInvoicingOpen(false);
    setTemplateDesignerOpen(false);
    setMainView("tables");
  }, [activeConnId, activeDatabase, ensureTablesLoaded, tables.length]);

  const handleAppUnlock = useCallback(() => {
    setSettingsOpen(false);
    setUnlocked(true);
    setMainView((current) => (current === "about" ? current : "dashboard"));
  }, []);

  const handleLockNow = useCallback(() => {
    setSettingsOpen(false);
    setTemplateDesignerOpen(false);
    setAdminMode(false);
    setUnlocked(false);
    setMainView((current) => (current === "about" ? "welcome" : current));
    setInvoicingOpen(false);
  }, []);

  const openAboutPage = useCallback(() => {
    setAboutReturnView(mainView);
    setSettingsOpen(false);
    setMainView("about");
  }, [mainView]);

  const returnFromAbout = useCallback(() => {
    if (aboutReturnView === "operations") {
      setMainView("operations");
      return;
    }

    if (!activeConnId) {
      setMainView("welcome");
      return;
    }

    if (!activeDatabase) {
      setMainView("database-selection");
      return;
    }

    if (aboutReturnView === "tables" && adminMode) {
      setMainView("tables");
      return;
    }

    if (aboutReturnView === "journals") {
      setMainView("journals");
      return;
    }

    setMainView("dashboard");
  }, [aboutReturnView, activeConnId, activeDatabase, adminMode]);

  useEffect(() => {
    const handler = (event) => {
      if (mainView !== "tables") return;
      if (event.key === "F5" || ((event.ctrlKey || event.metaKey) && event.key === "r")) {
        event.preventDefault();
        if (activeTable) loadTableData(activeConnId, activeTable, page, pageSize, filters);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [activeConnId, activeTable, filters, loadTableData, mainView, page, pageSize]);

  const handleConnectionSaved = useCallback(async (savedConnection, deletedId = null) => {
    if (deletedId) {
      setConnections((current) => current.filter((connection) => connection.id !== deletedId));
      setStatuses((current) => {
        const nextStatuses = { ...current };
        delete nextStatuses[deletedId];
        return nextStatuses;
      });
      return;
    }

    if (!savedConnection) {
      await loadConnections();
      return;
    }

    setConnections((current) => upsertConnection(current, savedConnection));
    await handleSelectConn(savedConnection.id, savedConnection);
  }, [handleSelectConn, loadConnections]);

  if (!unlocked) {
    return <LockScreen onUnlock={handleAppUnlock} />;
  }

  const showSidebarRail = sidebarCollapsed;
  const showTableBrowser = mainView === "tables" && adminMode;
  const showTableSidebar = showTableBrowser && !invoicingOpen && showTablePanel && activeConnId && activeDatabase;
  const showTableRail = showTableSidebar && tablePanelCollapsed;
  const showTableChrome = showTableBrowser && showToolbar;

  return (
    <div className="app-shell">
      <div className="app-body">
        {showSidebarRail ? (
          <div className={`panel-rail ${globalLoading ? "disabled" : ""}`}>
            <div className="panel-rail-group">
              <button className="panel-rail-btn" onClick={() => setSidebarCollapsed(false)} title={t("panel_show_connections")} disabled={globalLoading}>
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <rect x="4" y="4" width="6" height="16" rx="1" />
                  <path d="M14 8l4 4-4 4" />
                </svg>
              </button>
            </div>
            <div className="panel-rail-group">
              <button className="panel-rail-btn" onClick={() => setSettingsOpen(true)} title={t("sidebar_settings")} disabled={globalLoading}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <circle cx="12" cy="12" r="3" />
                  <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01A1.65 1.65 0 0 0 10.09 3H10a2 2 0 1 1 4 0h-.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01A1.65 1.65 0 0 0 21 10.09V10a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
                </svg>
              </button>
            </div>
          </div>
        ) : (
          <Sidebar
            connections={connections}
            statuses={statuses}
            activeSchemas={activeSchemas}
            activeId={activeConnId}
            activeDatabase={activeDatabase}
            adminMode={adminMode}
            onSelectConnection={handleSelectConn}
            onRefreshConnection={handleRefreshConnection}
            onRunDiagnostics={handleRunDiagnostics}
            onDuplicateWithDatabase={handleDuplicateWithDatabase}
            onConnectionSaved={handleConnectionSaved}
            onCollapse={() => setSidebarCollapsed(true)}
            onOpenDashboard={() => {
              setInvoicingOpen(false);
              setTemplateDesignerOpen(false);
              setMainView("dashboard");
            }}
            onOpenJournals={() => {
              setInvoicingOpen(false);
              setTemplateDesignerOpen(false);
              setMainView("journals");
            }}
            onOpenInvoicing={() => adminMode && activeConnId && activeDatabase && setInvoicingOpen(true)}
            onOpenOperations={() => {
              setInvoicingOpen(false);
              setTemplateDesignerOpen(false);
              setMainView("operations");
            }}
            onOpenSettings={() => setSettingsOpen(true)}
            invoicingOpen={adminMode && invoicingOpen}
            dashboardOpen={mainView === "dashboard" && !invoicingOpen}
            journalsOpen={mainView === "journals" && !invoicingOpen}
            operationsOpen={mainView === "operations" && !invoicingOpen}
            disabled={globalLoading}
          />
        )}

        {showTableSidebar ? (
          showTableRail ? (
            <div className="panel-rail panel-rail-secondary">
              <button className="panel-rail-btn" onClick={() => setTablePanelCollapsed(false)} title={t("panel_show_tables")}>
                <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <rect x="4" y="4" width="8" height="16" rx="1" />
                  <path d="M16 8l4 4-4 4" />
                </svg>
              </button>
            </div>
          ) : (
            <TablePanel
              tables={tables}
              loading={tablesLoading}
              activeTable={activeTable}
              onSelectTable={handleSelectTable}
              onCollapse={() => setTablePanelCollapsed(true)}
            />
          )
        ) : null}

        <div className={`main-content ${mainView === "dashboard" ? "dashboard-main" : ""} ${mainView === "operations" ? "operations-main" : ""}`}>
          {workspaces.length ? (
            <WorkspaceTabs
              workspaces={workspaces}
              activeId={activeConnId}
              onSelect={(workspaceId) => handleSelectConn(workspaceId)}
              onClose={handleCloseWorkspace}
            />
          ) : null}

          {mainView === "about" ? (
            <AboutPage onBack={returnFromAbout} onOpenSettings={() => setSettingsOpen(true)} />
          ) : mainView === "operations" ? (
            <OperationsWorkspace connections={connections} />
          ) : adminMode && invoicingOpen && activeConnId && activeDatabase ? (
            <Invoicing
              connId={activeConnId}
              schema={activeSchemas[activeConnId] ?? null}
              onBack={() => setInvoicingOpen(false)}
              onOpenTemplateDesigner={() => setTemplateDesignerOpen(true)}
            />
          ) : !activeConnId ? (
            <WelcomeScreen />
          ) : !activeDatabase ? (
            <DatabaseSelectionScreen
              loading={databasesLoading}
              databases={databases}
              connectionName={activeConnection?.name || activeConnection?.host || ""}
              onSelectDatabase={handleSelectDatabase}
            />
          ) : mainView === "journals" ? (
            <JournalsView
              connId={activeConnId}
              databaseName={activeDatabase}
            />
          ) : mainView === "dashboard" ? (
            <Dashboard
              connId={activeConnId}
              connectionName={activeConnection?.name || ""}
              databaseName={activeDatabase}
              t={t}
              lang={lang}
              adminMode={adminMode}
              onBackToTables={handleBackToTables}
              onOpenSettings={() => setSettingsOpen(true)}
            />
          ) : (
            <>
              {showTableChrome ? (
                <>
                  <Toolbar
                    activeConn={activeConnId}
                    activeTable={activeTable}
                    filters={filters}
                    tables={tables}
                    columns={columns}
                    totalCount={data?.total_count}
                    loading={dataLoading}
                    onRefresh={handleRefresh}
                    onSelectTable={handleSelectTable}
                    onToast={showToast}
                    sidebarCollapsed={sidebarCollapsed}
                    tablePanelCollapsed={tablePanelCollapsed}
                    showTablePanel={showTablePanel}
                    onToggleSidebar={() => setSidebarCollapsed((current) => !current)}
                    onToggleTables={() => setTablePanelCollapsed((current) => !current)}
                  />
                  <FilterBar filters={filters} columns={columns} onFiltersChange={handleFiltersChange} />
                </>
              ) : null}
              <DataGrid
                data={data}
                loading={dataLoading}
                page={page}
                pageSize={pageSize}
                onPageChange={handlePageChange}
                onPageSizeChange={handlePageSizeChange}
              />
            </>
          )}
        </div>
      </div>

      <div className="status-row">
        <div style={{ flex: 1 }}>
          <StatusBar message={statusMsg.message} type={statusMsg.type} elapsed={elapsed} />
        </div>
        <div className={`status-sidecar ${statusMsg.type === "error" ? "error" : statusMsg.type === "success" ? "success" : "idle"}`}>
          <ThemeToggle />
        </div>
      </div>

      {settingsOpen ? (
        <SettingsDrawer
          open={settingsOpen}
          onClose={() => setSettingsOpen(false)}
          adminMode={adminMode}
          onAdminModeChange={(nextMode) => {
            setAdminMode(nextMode);
            if (!nextMode && activeConnId && activeDatabase) {
              setMainView("dashboard");
            }
          }}
          showTablePanel={showTablePanel}
          onShowTablePanelChange={setShowTablePanel}
          showToolbar={showToolbar}
          onShowToolbarChange={setShowToolbar}
          appVersion={packageJson.version}
          onOpenAbout={openAboutPage}
          onLockNow={handleLockNow}
        />
      ) : null}

      {adminMode && templateDesignerOpen && activeConnId ? (
        <TemplateDesigner
          open={templateDesignerOpen}
          connectionId={activeConnId}
          onClose={() => setTemplateDesignerOpen(false)}
        />
      ) : null}

      {globalLoading && (
        <div className="global-overlay">
          <div className="empty-state">
            <div className="spinner" style={{ width: 32.8, height: 32.8, borderWidth: 2.46 }} />
            <p style={{ marginTop: 9.84, fontSize: 11.48, fontWeight: 500 }}>{t("loading")}</p>
          </div>
        </div>
      )}

      <Toast toast={toast} onDismiss={() => setToast(null)} />
      <UpdatePrompter />
      <ExportProgressCenter />
    </div>
  );
}

function WorkspaceTabs({ workspaces, activeId, onSelect, onClose }) {
  return (
    <div className="workspace-tabs">
      {workspaces.map((workspace) => (
        <button
          key={workspace.id}
          type="button"
          className={`workspace-tab ${workspace.id === activeId ? "active" : ""}`}
          onClick={() => onSelect(workspace.id)}
          title={workspace.name}
        >
          <span className="workspace-tab-title">{workspace.database}</span>
          <span className="workspace-tab-meta">{workspace.schema?.edition || "database"}</span>
          <span
            className="workspace-tab-close"
            role="button"
            tabIndex={0}
            onClick={(event) => {
              event.stopPropagation();
              onClose(workspace.id);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                event.stopPropagation();
                onClose(workspace.id);
              }
            }}
            aria-label="Close workspace"
          >
            x
          </span>
        </button>
      ))}
    </div>
  );
}

function WelcomeScreen() {
  const { t } = useT();

  return (
    <div className="empty-state" style={{ gap: 13.12 }}>
      <svg width="52" height="52" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1">
        <path d="M12 2L2 7l10 5 10-5-10-5z" />
        <path d="M2 17l10 5 10-5" />
        <path d="M2 12l10 5 10-5" />
      </svg>
      <div>
        <h3 style={{ fontSize: 13.12, marginBottom: 4.92 }}>{t("welcome_title")}</h3>
        <p style={{ whiteSpace: "pre-line" }}>{t("welcome_hint")}</p>
      </div>
    </div>
  );
}

function DatabaseSelectionScreen({ loading, databases, connectionName, onSelectDatabase }) {
  const { t } = useT();

  return (
    <div className="empty-state empty-state-wide" style={{ gap: 13.12 }}>
      {loading ? <div className="spinner" /> : (
        <svg width="44" height="44" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.3">
          <ellipse cx="12" cy="5" rx="9" ry="3" />
          <path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3" />
          <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5" />
        </svg>
      )}
      <div>
        <h3 style={{ fontSize: 13.12, marginBottom: 4.92 }}>{t("database_prompt_title")}</h3>
        <p>{loading ? t("database_prompt_loading") : t("database_prompt_body", connectionName)}</p>
      </div>
      {!loading && !databases.length ? <div className="sidebar-empty-copy">{t("sidebar_no_databases")}</div> : null}
      {!loading && databases.length ? (
        <div className="database-selection-list">
          {databases.map((database) => (
            <button key={database} type="button" className="database-selection-item" onClick={() => onSelectDatabase(database)}>
              <span className="database-selection-pill" />
              <span>{database}</span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
