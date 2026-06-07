import { useEffect, useState } from "react";
import { deleteConnection, disconnectDb, getDatabases } from "../hooks/useTauri";
import { useT } from "../i18n";
import ConnectionModal from "./ConnectionModal";

function describeConnection(connection) {
  const host = connection.host?.trim() || "SQL Server";
  const label = connection.name?.trim() || (connection.database ? `${host} / ${connection.database}` : host);
  const meta = `${host}:${connection.port}${connection.database ? ` · ${connection.database}` : ""}`;
  return { label, meta };
}

function EditionBadge({ configuredEdition, detectedSchema }) {
  const edition = detectedSchema?.edition || configuredEdition || "auto";
  const confidence = detectedSchema?.confidence;

  const label = edition === "sage100" ? "S100"
    : edition === "sage1000" ? "S1000"
    : edition === "sagex3" ? "X3"
    : null;

  if (!label) return null;

  const isCustom = configuredEdition === "custom";
  const isDetected = !!detectedSchema;
  const color = isCustom ? "#ca8a04" : isDetected ? "var(--accent)" : "var(--text-lo)";
  const bg = isCustom ? "rgba(234,179,8,0.12)" : isDetected ? "var(--accent-mute)" : "rgba(255,255,255,0.05)";
  
  let fullName = edition === "sage100" ? "Sage 100 Comptabilité"
    : edition === "sage1000" ? "Sage 1000"
    : edition === "sagex3" ? "Sage X3"
    : edition;
  
  if (confidence !== undefined && confidence > 0) {
    fullName += ` (${confidence}%)`;
  }

  return (
    <span
      title={fullName}
      style={{
        fontSize: 9.5,
        fontWeight: 700,
        letterSpacing: "0.04em",
        padding: "1px 5px",
        borderRadius: 4,
        background: bg,
        color,
        flexShrink: 0,
      }}
    >
      {label}
    </span>
  );
}

export default function Sidebar({
  connections,
  statuses,
  activeSchemas,
  activeId,
  activeDatabase,
  adminMode,
  invoicingOpen,
  onSelectConnection,
  onRefreshConnection,
  onRunDiagnostics,
  onDuplicateWithDatabase,
  onConnectionSaved,
  onCollapse,
  onOpenDashboard,
  onOpenInvoicing,
  onOpenSettings,
  disabled,
}) {
  const { t } = useT();
  const [showModal, setShowModal] = useState(false);
  const [editingConn, setEditingConn] = useState(null);
  const [contextMenu, setContextMenu] = useState(null);
  const [dbSubmenu, setDbSubmenu] = useState(null); // { connection, databases, loading }

  useEffect(() => {
    const handler = () => {
      setContextMenu(null);
      setDbSubmenu(null);
    };
    window.addEventListener("click", handler);
    return () => window.removeEventListener("click", handler);
  }, []);

  const handleSave = (saved) => {
    onConnectionSaved(saved);
    setShowModal(false);
    setEditingConn(null);
  };

  const handleDelete = async (connection) => {
    if (!confirm(t("sidebar_delete_confirm", connection.name || connection.host))) return;

    try {
      await disconnectDb(connection.id);
      await deleteConnection(connection.id);
      onConnectionSaved(null, connection.id);
      if (activeId === connection.id) onSelectConnection(null);
    } catch (error) {
      alert(t("error") + ": " + error);
    }

    setContextMenu(null);
  };

  const handleContextMenu = (event, connection) => {
    event.preventDefault();
    setContextMenu({ x: event.clientX, y: event.clientY, connection });
    setDbSubmenu(null);
  };

  const handleOpenDatabases = async (connection) => {
    if (dbSubmenu?.connection?.id === connection.id) return;
    
    setDbSubmenu({ connection, databases: [], loading: true });
    try {
      const dbs = await getDatabases(connection.id);
      setDbSubmenu({ connection, databases: dbs, loading: false });
    } catch (err) {
      setDbSubmenu(null);
      console.error("Failed to load databases for context menu", err);
    }
  };

  const getStatus = (id) => statuses[id] || "disconnected";

  return (
    <>
      <aside className={`sidebar ${disabled ? "disabled" : ""}`}>
        <div className="sidebar-header">
          <div className="sidebar-logo">
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M12 2L2 7l10 5 10-5-10-5z" />
              <path d="M2 17l10 5 10-5" />
              <path d="M2 12l10 5 10-5" />
            </svg>
            Sage Bridge
          </div>
          <button className="panel-header-btn" onClick={onCollapse} title={t("panel_hide_connections")} disabled={disabled}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M15 18l-6-6 6-6" />
            </svg>
          </button>
        </div>

        <div className="sidebar-scroll">
          <div className="sidebar-section-title">{t("sidebar_connections")}</div>

          {connections.length === 0 ? (
            <div className="sidebar-empty-copy">{t("sidebar_no_connections")}</div>
          ) : connections.map((connection) => {
            const status = getStatus(connection.id);
            const { label, meta } = describeConnection(connection);

            return (
              <button
                key={connection.id}
                type="button"
                className={`sidebar-item ${activeId === connection.id ? "active" : ""}`}
                onClick={() => !disabled && onSelectConnection(connection.id)}
                onContextMenu={(event) => !disabled && handleContextMenu(event, connection)}
                title={meta}
                disabled={disabled}
              >
                <span className={`dot ${status}`} />
                <span className="sidebar-item-copy">
                  <span className="sidebar-item-label" style={{ display: "flex", alignItems: "center", gap: 5 }}>
                    {label}
                    <EditionBadge
                      configuredEdition={connection.sage_edition}
                      detectedSchema={activeSchemas?.[connection.id]}
                    />
                  </span>
                  <span className="sidebar-item-meta">{meta}</span>
                </span>
              </button>
            );
          })}
        </div>

        <div className="sidebar-footer">
          {adminMode && activeId && activeDatabase ? (
            <button className="sidebar-dashboard-btn" onClick={onOpenDashboard} disabled={disabled}>
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M3 3v18h18" />
                <path d="M7 14l3-3 3 2 4-6" />
              </svg>
              {t("sidebar_dashboard")}
            </button>
          ) : null}

          {activeId && activeDatabase ? (
            <button
              className={`sidebar-dashboard-btn ${invoicingOpen ? "active" : ""}`}
              onClick={onOpenInvoicing}
              disabled={disabled}
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M6 3h9l3 3v15H6z" />
                <path d="M9 9h6M9 13h6M9 17h4" />
              </svg>
              {t("sidebar_invoicing")}
            </button>
          ) : null}

          <button
            className="sidebar-add-btn"
            onClick={() => {
              setEditingConn(null);
              setShowModal(true);
            }}
            disabled={disabled}
          >
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
            {t("sidebar_new")}
          </button>

          <button className="sidebar-settings-btn" onClick={onOpenSettings} disabled={disabled}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <circle cx="12" cy="12" r="3" />
              <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01A1.65 1.65 0 0 0 10.09 3H10a2 2 0 1 1 4 0h-.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01A1.65 1.65 0 0 0 21 10.09V10a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
            </svg>
            {t("sidebar_settings")}
          </button>
        </div>
      </aside>

      {contextMenu ? (
        <div className="context-menu" style={{ left: contextMenu.x, top: contextMenu.y }}>
          <div
            className="context-menu-item"
            onClick={() => {
              const target = contextMenu.connection;
              setContextMenu(null);
              onRefreshConnection?.(target.id, target);
            }}
          >
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M21 12a9 9 0 1 1-2.64-6.36" />
              <path d="M21 3v6h-6" />
            </svg>
            {t("refresh")}
          </div>

          <div
            className="context-menu-item"
            onClick={() => {
              const target = contextMenu.connection;
              setContextMenu(null);
              onRunDiagnostics?.(target.id);
            }}
          >
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M4 19h16" />
              <path d="M7 16V8" />
              <path d="M12 16V5" />
              <path d="M17 16v-4" />
            </svg>
            Diagnostics
          </div>

          {getStatus(contextMenu.connection.id) === "connected" && (
            <div
              className={`context-menu-item ${dbSubmenu ? "active" : ""}`}
              onMouseEnter={() => handleOpenDatabases(contextMenu.connection)}
              onClick={(e) => {
                e.stopPropagation();
                handleOpenDatabases(contextMenu.connection);
              }}
            >
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <ellipse cx="12" cy="5" rx="9" ry="3" />
                <path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3" />
                <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5" />
              </svg>
              {t("sidebar_databases")}
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{ marginLeft: "auto", opacity: 0.5 }}>
                <path d="M9 18l6-6-6-6" />
              </svg>

              {dbSubmenu?.connection?.id === contextMenu.connection.id && (
                <div className="context-submenu">
                  {dbSubmenu.loading ? (
                    <div className="context-menu-item disabled">
                      <div className="spinner" style={{ width: 12, height: 12, borderWidth: 1.5 }} />
                      {t("loading")}
                    </div>
                  ) : dbSubmenu.databases.length === 0 ? (
                    <div className="context-menu-item disabled">{t("sidebar_no_databases")}</div>
                  ) : dbSubmenu.databases.map((db) => (
                    <div
                      key={db}
                      className={`context-menu-item ${activeDatabase === db && activeId === contextMenu.connection.id ? "active" : ""}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        setContextMenu(null);
                        setDbSubmenu(null);
                        onDuplicateWithDatabase(contextMenu.connection, db);
                      }}
                    >
                      <span className={`dot ${activeDatabase === db && activeId === contextMenu.connection.id ? "connected" : ""}`} style={{ width: 6, height: 6 }} />
                      {db}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          <div className="context-menu-sep" />
          <div
            className="context-menu-item"
            onClick={() => {
              setEditingConn(contextMenu.connection);
              setShowModal(true);
              setContextMenu(null);
            }}
          >
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
              <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
            </svg>
            {t("edit")}
          </div>
          <div className="context-menu-sep" />
          <div className="context-menu-item danger" onClick={() => handleDelete(contextMenu.connection)}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <polyline points="3 6 5 6 21 6" />
              <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
              <path d="M10 11v6M14 11v6M9 6V4h6v2" />
            </svg>
            {t("delete")}
          </div>
        </div>
      ) : null}

      {showModal ? (
        <ConnectionModal
          existing={editingConn}
          onSave={handleSave}
          onClose={() => {
            setShowModal(false);
            setEditingConn(null);
          }}
        />
      ) : null}
    </>
  );
}
