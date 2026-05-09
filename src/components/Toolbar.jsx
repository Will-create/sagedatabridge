import { useState } from "react";
import { exportToCsv, exportToExcel, exportToJson, exportToSql } from "../hooks/useTauri";
import { useT } from "../i18n";
import QueryStudioModal from "./QueryStudioModal";
import SchemaMapModal from "./SchemaMapModal";
import VisualizationModal from "./VisualizationModal";

function ExportModal({ activeConn, activeTable, filters, columns, onClose, onToast }) {
  const { t } = useT();
  const [format, setFormat] = useState("xlsx");
  const [selectedCols, setSelectedCols] = useState([]);
  const [exporting, setExporting] = useState(false);

  const toggleCol = (name) =>
    setSelectedCols((prev) => prev.includes(name) ? prev.filter((c) => c !== name) : [...prev, name]);

  const handleExport = async () => {
    setExporting(true);
    try {
      let path = null;
      if (format === "xlsx") path = await exportToExcel(activeConn, activeTable.schema, activeTable.name, filters, selectedCols);
      if (format === "csv")  path = await exportToCsv(activeConn, activeTable.schema, activeTable.name, filters, selectedCols);
      if (format === "json") path = await exportToJson(activeConn, activeTable.schema, activeTable.name, filters, selectedCols);
      if (format === "sql")  path = await exportToSql(activeConn, activeTable.schema, activeTable.name, filters);
      if (path) { onToast({ type:"success", msg: t("export_ok", path.split(/[\\/]/).pop()) }); onClose(); }
    } catch (err) { onToast({ type:"error", msg: err.toString() }); }
    finally { setExporting(false); }
  };

  const formats = [
    { id:"xlsx", icon:"📊", label:"XLSX", desc: t("export_excel_desc") },
    { id:"csv",  icon:"📄", label:"CSV",  desc: t("export_csv_desc") },
    { id:"json", icon:"{ }", label:"JSON", desc: t("export_json_desc") },
    { id:"sql",  icon:"⚡",  label:"SQL",  desc: t("export_sql_desc") },
  ];

  return (
    <div className="modal-overlay" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" style={{ "--modal-width": "460px", "--modal-min-width": "460px" }}>
        <div className="modal-title">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
            <polyline points="7 10 12 15 17 10" /><line x1="12" y1="15" x2="12" y2="3" />
          </svg>
          {t("export_title", activeTable.full_name)}
        </div>

        <div className="modal-body">
          <div style={{ marginBottom:16 }}>
            <label style={{ marginBottom:8, display:"block" }}>{t("export_format")}</label>
            <div className="export-format-grid" style={{ gridTemplateColumns:"1fr 1fr" }}>
              {formats.map((f) => (
                <button key={f.id} className={`export-format-btn ${format === f.id ? "selected" : ""}`}
                  onClick={() => setFormat(f.id)}>
                  <span className="format-icon">{f.icon}</span>
                  <strong>{f.label}</strong>
                  <span style={{ fontSize:10, color:"inherit", opacity:0.7 }}>{f.desc}</span>
                </button>
              ))}
            </div>
          </div>

          {format !== "sql" && columns.length > 0 && (
            <div style={{ marginBottom:16 }}>
              <label style={{ marginBottom:8, display:"flex", alignItems:"center", gap:8 }}>
                {t("export_columns")}
                <button className="btn btn-ghost btn-sm" style={{ marginLeft:"auto", textTransform:"none", fontWeight:"normal" }}
                  onClick={() => setSelectedCols([])}>{t("all")}</button>
              </label>
              <div style={{ display:"grid", gridTemplateColumns:"1fr 1fr", gap:4, maxHeight:180,
                overflowY:"auto", background:"var(--bg-input)", border:"1px solid var(--border)",
                borderRadius:"var(--r-sm)", padding:8 }}>
                {columns.map((col) => {
                  const checked = selectedCols.length === 0 || selectedCols.includes(col.name);
                  return (
                    <label key={col.name} className="checkbox-group" style={{ fontSize:11.5, gap:6 }}>
                      <input type="checkbox" checked={checked} onChange={() => {
                        if (selectedCols.length === 0) setSelectedCols(columns.map((c) => c.name).filter((n) => n !== col.name));
                        else toggleCol(col.name);
                      }} />
                      <span style={{ fontFamily:"var(--font-mono)", color:"var(--text-hi)", overflow:"hidden", textOverflow:"ellipsis", whiteSpace:"nowrap" }}>
                        {col.name}
                      </span>
                    </label>
                  );
                })}
              </div>
              {selectedCols.length > 0 && (
                <div style={{ fontSize:11, color:"var(--text-lo)", marginTop:4 }}>
                  {t("export_n_of", selectedCols.length, columns.length)}
                </div>
              )}
            </div>
          )}

          <div style={{ fontSize:11, color:"var(--text-lo)", padding:"6px 10px",
            background:"var(--bg-input)", borderRadius:"var(--r-sm)", border:"1px solid var(--border)" }}>
            {t("export_note")}
          </div>
        </div>

        <div className="modal-actions">
          <button className="btn btn-ghost" onClick={onClose}>{t("cancel")}</button>
          <button className="btn btn-accent" onClick={handleExport} disabled={exporting}>
            {exporting
              ? <><span className="spinner" style={{width:12,height:12}} /> {t("export_ing")}</>
              : t("export_btn", format)}
          </button>
        </div>
      </div>
    </div>
  );
}

export default function Toolbar({
  activeConn,
  activeTable,
  tables,
  filters,
  columns,
  totalCount,
  loading,
  onRefresh,
  onSelectTable,
  onToast,
  sidebarCollapsed,
  tablePanelCollapsed,
  showTablePanel = true,
  onToggleSidebar,
  onToggleTables,
}) {
  const { t } = useT();
  const [showExport, setShowExport] = useState(false);
  const [showSqlStudio, setShowSqlStudio] = useState(false);
  const [showSchemaMap, setShowSchemaMap] = useState(false);
  const [showViz, setShowViz] = useState(false);

  return (
    <>
      <div className="toolbar">
        <button
          className="btn btn-ghost btn-icon"
          title={sidebarCollapsed ? t("panel_show_connections") : t("panel_hide_connections")}
          onClick={onToggleSidebar}
          style={{ padding:"5px 7px" }}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <rect x="3" y="4" width="6" height="16" rx="1" />
            <path d={sidebarCollapsed ? "M15 8l4 4-4 4" : "M19 8l-4 4 4 4"} />
          </svg>
        </button>

        {showTablePanel ? (
          <>
            <button
              className="btn btn-ghost btn-icon"
              title={tablePanelCollapsed ? t("panel_show_tables") : t("panel_hide_tables")}
              onClick={onToggleTables}
              disabled={!activeConn}
              style={{ padding:"5px 7px" }}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <rect x="4" y="4" width="8" height="16" rx="1" />
                <path d={tablePanelCollapsed ? "M16 8l4 4-4 4" : "M20 8l-4 4 4 4"} />
              </svg>
            </button>

            <div className="toolbar-sep" />
          </>
        ) : null}

        {activeTable
          ? <div className="toolbar-label" title={activeTable.full_name}>{activeTable.schema}.{activeTable.name}</div>
          : <div style={{ color:"var(--text-lo)", fontSize:12 }}>{t("toolbar_no_table")}</div>
        }

        {totalCount != null && (
          <div style={{ fontSize:11.5, color:"var(--text-mid)" }}>
            <span style={{ color:"var(--accent)", fontFamily:"var(--font-mono)", fontWeight:600 }}>
              {filters.length > 0 ? "~" : ""}{t("toolbar_rows", totalCount)}
            </span>
            {filters.length > 0 && (
              <span style={{ color:"var(--yellow)", marginLeft:6 }}>({t("toolbar_filters", filters.length)})</span>
            )}
          </div>
        )}

        <div className="toolbar-spacer" />

        <button className="btn btn-ghost btn-icon" title={t("toolbar_refresh")}
          onClick={onRefresh} disabled={loading || !activeTable} style={{ padding:"5px 7px" }}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
            style={loading ? { animation:"spin 0.7s linear infinite" } : {}}>
            <polyline points="23 4 23 10 17 10" />
            <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
          </svg>
        </button>

        <div className="toolbar-sep" />

        <button className="btn" onClick={() => setShowSqlStudio(true)} disabled={!activeConn}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M8 21h12" />
            <path d="M12 17h8" />
            <path d="M4 3h16v12H4z" />
          </svg>
          {t("toolbar_sql")}
        </button>

        <button className="btn" onClick={() => setShowSchemaMap(true)} disabled={!activeConn}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <rect x="3" y="5" width="7" height="5" rx="1" />
            <rect x="14" y="3" width="7" height="5" rx="1" />
            <rect x="14" y="16" width="7" height="5" rx="1" />
            <path d="M10 7h4M12 7v11M12 18h2" />
          </svg>
          {t("toolbar_schema")}
        </button>

        <button className="btn" onClick={() => setShowViz(true)} disabled={!activeTable || loading}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M3 3v18h18" />
            <path d="M7 15l4-4 3 3 5-6" />
          </svg>
          {t("toolbar_visualize")}
        </button>

        <button className="btn btn-accent" onClick={() => setShowExport(true)} disabled={!activeTable || loading}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2">
            <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
            <polyline points="7 10 12 15 17 10" /><line x1="12" y1="15" x2="12" y2="3" />
          </svg>
          {t("toolbar_export")}
        </button>
      </div>

      {showExport && (
        <ExportModal activeConn={activeConn} activeTable={activeTable} filters={filters} columns={columns}
          onClose={() => setShowExport(false)} onToast={onToast} />
      )}

      {showSqlStudio && activeConn && (
        <QueryStudioModal activeConn={activeConn} activeTable={activeTable}
          onClose={() => setShowSqlStudio(false)} onToast={onToast} />
      )}

      {showSchemaMap && activeConn && (
        <SchemaMapModal
          activeConn={activeConn}
          activeTable={activeTable}
          tables={tables}
          onSelectTable={onSelectTable}
          onClose={() => setShowSchemaMap(false)}
          onToast={onToast}
        />
      )}

      {showViz && activeTable && (
        <VisualizationModal activeConn={activeConn} activeTable={activeTable} filters={filters}
          onClose={() => setShowViz(false)} onToast={onToast} />
      )}
    </>
  );
}
