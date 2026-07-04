import { useEffect, useMemo, useState } from "react";
import {
  clearQueryHistory,
  deleteSavedQuery,
  executeQuery,
  listQueryHistory,
  listSavedQueries,
  saveSavedQuery,
} from "../hooks/useTauri";
import { useT } from "../i18n";
import DataGrid from "./DataGrid";
import { useExportJobs } from "../exportJobs";

const DEFAULT_PAGE_SIZE = 100;
const SQL_RESULT_PAGE_SIZES = [50, 100, 250, 500, 1000, 10000, 20000, 0];

function isRiskySql(sql) {
  const trimmed = sql.trim().toUpperCase();
  return /^(UPDATE|DELETE|INSERT|MERGE|DROP|ALTER|TRUNCATE|CREATE|EXEC|EXECUTE)\b/.test(trimmed);
}

function snippet(sql) {
  return sql.replace(/\s+/g, " ").trim().slice(0, 120);
}

function cellToClipboardValue(value) {
  if (value === null || value === undefined) return "";
  return String(value).replace(/\t/g, " ").replace(/\r?\n/g, " ");
}

function tableToTsv(columns, rows) {
  const header = columns.map((column) => cellToClipboardValue(column.name)).join("\t");
  const body = rows.map((row) => row.map(cellToClipboardValue).join("\t"));
  return [header, ...body].join("\n");
}

function cellToCsvValue(value) {
  if (value === null || value === undefined) return "";
  const text = String(value).replace(/\r\n/g, "\n").replace(/\r/g, "\n");
  return /[",\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

function tableToCsv(columns, rows) {
  const header = columns.map((column) => cellToCsvValue(column.name)).join(",");
  const body = rows.map((row) => row.map(cellToCsvValue).join(","));
  return [header, ...body].join("\r\n");
}

function downloadTextFile(text, filename, mimeType) {
  const blob = new Blob([text], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

function timestampForFilename() {
  return new Date().toISOString().replace(/[:.]/g, "-");
}

export default function QueryStudioModal({ activeConn, activeTable, onClose, onToast }) {
  const { t, lang } = useT();
  const { beginLocalJob, updateLocalJob } = useExportJobs();
  const [savedQueries, setSavedQueries] = useState([]);
  const [history, setHistory] = useState([]);
  const [queryName, setQueryName] = useState("");
  const [sql, setSql] = useState("");
  const [selectedSavedId, setSelectedSavedId] = useState(null);
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState(null);
  const [error, setError] = useState("");
  const [elapsed, setElapsed] = useState(null);
  const [page, setPage] = useState(0);
  const [pageSize, setPageSize] = useState(DEFAULT_PAGE_SIZE);
  const [exportingCsv, setExportingCsv] = useState(false);
  const locale = lang === "fr" ? "fr-FR" : "en-US";

  const refreshLibrary = async () => {
    const [queries, entries] = await Promise.all([
      listSavedQueries(activeConn),
      listQueryHistory(activeConn),
    ]);
    setSavedQueries(queries);
    setHistory(entries);
  };

  useEffect(() => {
    refreshLibrary().catch((err) => onToast({ type: "error", msg: String(err) }));
  }, [activeConn]);

  useEffect(() => {
    if (!sql.trim() && activeTable) {
      setSql(`SELECT TOP 500 *\nFROM ${activeTable.full_name}\nORDER BY 1;`);
    }
  }, [activeTable]);

  const pagedResults = useMemo(() => {
    if (!results) return null;
    const effectivePageSize = pageSize === 0 ? Math.max(results.rows.length, 1) : pageSize;
    const start = page * effectivePageSize;
    const end = start + effectivePageSize;
    return {
      ...results,
      rows: results.rows.slice(start, end),
      total_count: results.total_count,
      page,
      page_size: effectivePageSize,
    };
  }, [page, pageSize, results]);

  const handleRun = async () => {
    if (!sql.trim()) return;
    if (isRiskySql(sql) && !confirm(t("sql_confirm_danger"))) return;

    setRunning(true);
    setError("");
    const started = Date.now();

    try {
      const data = await executeQuery(activeConn, sql);
      setResults(data);
      setPage(0);
      setElapsed(Date.now() - started);
      await refreshLibrary();
      onToast({ type: "success", msg: t("sql_run_ok", data.total_count.toLocaleString(locale)) });
    } catch (err) {
      setResults(null);
      setError(String(err));
      setElapsed(Date.now() - started);
      await refreshLibrary();
      onToast({ type: "error", msg: String(err) });
    } finally {
      setRunning(false);
    }
  };

  const handleSave = async () => {
    const fallbackName = activeTable ? `${activeTable.schema}.${activeTable.name}` : t("sql_saved_default");
    const name = (queryName.trim() || prompt(t("sql_saved_name_prompt"), fallbackName) || "").trim();
    if (!name || !sql.trim()) return;

    try {
      const saved = await saveSavedQuery({
        id: selectedSavedId || "",
        connection_id: activeConn,
        name,
        sql,
        updated_at: "",
      });
      setQueryName(saved.name);
      setSelectedSavedId(saved.id);
      await refreshLibrary();
      onToast({ type: "success", msg: t("sql_saved_ok", saved.name) });
    } catch (err) {
      onToast({ type: "error", msg: String(err) });
    }
  };

  const handleDeleteSaved = async () => {
    if (!selectedSavedId) return;
    const match = savedQueries.find((item) => item.id === selectedSavedId);
    if (match && !confirm(t("sql_saved_delete_confirm", match.name))) return;

    try {
      await deleteSavedQuery(selectedSavedId);
      setSelectedSavedId(null);
      setQueryName("");
      await refreshLibrary();
    } catch (err) {
      onToast({ type: "error", msg: String(err) });
    }
  };

  const handleSelectSaved = (item) => {
    setSelectedSavedId(item.id);
    setQueryName(item.name);
    setSql(item.sql);
    setError("");
  };

  const handleSelectHistory = (item) => {
    setSelectedSavedId(null);
    setQueryName("");
    setSql(item.sql);
    setError(item.error || "");
  };

  const handleClearHistory = async () => {
    if (!confirm(t("sql_history_clear_confirm"))) return;
    try {
      await clearQueryHistory(activeConn);
      await refreshLibrary();
    } catch (err) {
      onToast({ type: "error", msg: String(err) });
    }
  };

  const copyResults = async (scope) => {
    if (!results) return;
    const source = scope === "page" ? pagedResults : results;
    const text = tableToTsv(source.columns, source.rows);
    try {
      await navigator.clipboard.writeText(text);
      onToast({ type: "success", msg: t("copy") });
    } catch (err) {
      onToast({ type: "error", msg: String(err) });
    }
  };

  const exportResultsCsv = async () => {
    if (!results) return;
    const tracked = beginLocalJob("Query results CSV", "query-csv", `query-csv:${activeConn}:${sql}`);
    if (tracked.existing) return;
    setExportingCsv(true);
    try {
      updateLocalJob(tracked.job.id, { phase: "writing CSV", percent: 50, total_rows: results.rows.length });
      await new Promise((resolve) => requestAnimationFrame(() => resolve()));
      const csv = tableToCsv(results.columns, results.rows);
      const filename = `sql-results-${timestampForFilename()}.csv`;
      downloadTextFile(`\uFEFF${csv}`, filename, "text/csv;charset=utf-8");
      updateLocalJob(tracked.job.id, { status: "completed", phase: "completed", percent: 100, processed_rows: results.rows.length, finished_at: new Date().toISOString() });
      onToast({ type: "success", msg: t("sql_export_csv_ok", results.rows.length.toLocaleString(locale)) });
    } catch (error) {
      updateLocalJob(tracked.job.id, { status: "failed", phase: "failed", error: String(error), finished_at: new Date().toISOString() });
      onToast({ type: "error", msg: String(error) });
    } finally {
      setExportingCsv(false);
    }
  };

  return (
    <div className="modal-overlay" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal query-studio-modal">
        <div className="modal-title">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M8 21h12" />
            <path d="M12 17h8" />
            <path d="M4 3h16v12H4z" />
          </svg>
          {t("sql_title")}
        </div>

        <div className="modal-body modal-body-fill">
          <div className="query-studio-layout">
            <aside className="query-library">
              <div className="query-library-section">
                <div className="query-library-header">
                  <span>{t("sql_saved_queries")}</span>
                  <button className="btn btn-ghost btn-sm" onClick={() => { setSelectedSavedId(null); setQueryName(""); setSql(""); }}>
                    {t("sql_new")}
                  </button>
                </div>

                <div className="query-library-list">
                  {savedQueries.length === 0 ? (
                    <div className="query-library-empty">{t("sql_saved_empty")}</div>
                  ) : savedQueries.map((item) => (
                    <button
                      key={item.id}
                      className={`query-library-item ${selectedSavedId === item.id ? "active" : ""}`}
                      onClick={() => handleSelectSaved(item)}
                    >
                      <strong>{item.name}</strong>
                      <span>{snippet(item.sql)}</span>
                    </button>
                  ))}
                </div>
              </div>

              <div className="query-library-section">
                <div className="query-library-header">
                  <span>{t("sql_history")}</span>
                  <button className="btn btn-ghost btn-sm" onClick={handleClearHistory} disabled={history.length === 0}>
                    {t("sql_history_clear")}
                  </button>
                </div>

                <div className="query-library-list">
                  {history.length === 0 ? (
                    <div className="query-library-empty">{t("sql_history_empty")}</div>
                  ) : history.map((item) => (
                    <button key={item.id} className="query-library-item" onClick={() => handleSelectHistory(item)}>
                      <strong className={item.ok ? "history-ok" : "history-error"}>
                        {item.ok ? t("sql_history_ok") : t("sql_history_error")}
                      </strong>
                      <span>{snippet(item.sql)}</span>
                      <small>{new Date(item.ran_at).toLocaleString(locale)}</small>
                    </button>
                  ))}
                </div>
              </div>
            </aside>

            <div className="query-studio-main">
              <div className="query-editor-toolbar">
                <input
                  type="text"
                  className="query-name-input"
                  placeholder={t("sql_name_ph")}
                  value={queryName}
                  onChange={(e) => setQueryName(e.target.value)}
                />

                {activeTable && (
                  <button
                    className="btn btn-ghost"
                    onClick={() => setSql(`SELECT TOP 500 *\nFROM ${activeTable.full_name}\nORDER BY 1;`)}
                  >
                    {t("sql_from_table")}
                  </button>
                )}

                <button className="btn btn-ghost" onClick={handleDeleteSaved} disabled={!selectedSavedId}>
                  {t("delete")}
                </button>
                <button className="btn" onClick={handleSave} disabled={!sql.trim()}>
                  {t("save")}
                </button>
                <button className="btn btn-accent" onClick={handleRun} disabled={running || !sql.trim()}>
                  {running ? t("sql_running") : t("sql_run")}
                </button>
              </div>

              <textarea
                className="query-editor"
                value={sql}
                onChange={(e) => setSql(e.target.value)}
                placeholder={t("sql_editor_ph")}
                onKeyDown={(e) => {
                  if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
                    e.preventDefault();
                    handleRun();
                  }
                }}
              />

              <div className="query-results-header">
                <div>{t("sql_results")}</div>
                <div className="query-results-actions">
                  {running ? <span className="query-results-meta"><span className="spinner" style={{ width: 9.84, height: 9.84 }} /> {t("sql_running")}</span> : null}
                  {elapsed != null && <div className="query-results-meta">{t("sql_elapsed", elapsed)}</div>}
                  <button className="btn btn-ghost btn-sm" onClick={() => copyResults("page")} disabled={!pagedResults || running}>
                    {t("sql_copy_page")}
                  </button>
                  <button className="btn btn-ghost btn-sm" onClick={() => copyResults("all")} disabled={!results || running}>
                    {t("sql_copy_all")}
                  </button>
                  <button className="btn btn-ghost btn-sm" onClick={exportResultsCsv} disabled={!results || running || exportingCsv}>
                    {t("sql_export_csv")}
                  </button>
                </div>
              </div>

              {error && (
                <div className="query-error-banner">
                  <strong>{t("error")}:</strong> {error}
                </div>
              )}

              <div className="query-results-panel">
                {pagedResults ? (
                  <DataGrid
                    data={pagedResults}
                    loading={running}
                    page={page}
                    pageSize={pageSize}
                    pageSizeOptions={SQL_RESULT_PAGE_SIZES}
                    onPageChange={setPage}
                    onPageSizeChange={(size) => { setPageSize(size); setPage(0); }}
                  />
                ) : (
                  <div className="query-results-empty">{t("sql_results_empty")}</div>
                )}
              </div>
            </div>
          </div>
        </div>

        <div className="modal-actions">
          <button className="btn btn-ghost" onClick={onClose}>{t("close")}</button>
        </div>
      </div>
    </div>
  );
}
