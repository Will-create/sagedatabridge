import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/api/dialog";
import {
  cancelOperationJob,
  deleteOperationTemplate,
  exportOperationDiagnostic,
  listOperationJobs,
  listOperationTemplates,
  confirmBackupStage,
  inspectBackup,
  previewBackupStage,
  operationsListDatabases,
  listSqlServerPaths,
  operationsListTables,
  preflightBackup,
  preflightRestore,
  preflightTransfer,
  previewTransferTable,
  resumeOperationJob,
  saveOperationTemplate,
  startBackup,
  startRestore,
  startTransfer,
} from "../hooks/useTauri";
import { useT } from "../i18n";
import OperationsDocumentation from "./Operations/OperationsDocumentation";
import SalvageRescuePanel from "./Operations/SalvageRescuePanel";
import SqlPathBrowser from "./Operations/SqlPathBrowser";
import {
  confirmationMatches,
  defaultBackupFileName,
  groupTablesBySchema,
  isUserProfilePath,
  isTerminalJob,
  salvageTargetAllowed,
  jobPercent,
  latestLogMessage,
  replaceVisibleTransferTables,
  selectedTableKey,
  showAdvancedTools,
  sourceTableKey,
  toggleTransferTable,
  transferEndpointsClash,
  visibleOperationTables,
} from "./operationsModel";

const EMPTY_TRANSFER = {
  sourceConnectionId: "",
  targetConnectionId: "",
  sourceDatabase: "",
  targetDatabase: "",
  tables: [],
  batchSize: 500,
  conflictStrategy: "fail",
  confirmationText: "",
  editionMismatchConfirmation: "",
};

const EMPTY_BACKUP = {
  connectionId: "",
  database: "",
  backupPath: "",
  copyOnly: true,
  confirmationText: "",
};

const EMPTY_RESTORE = {
  connectionId: "",
  backupPath: "",
  targetDatabase: "",
  replaceExisting: false,
  confirmationText: "",
  salvage: false,
  repairLevel: "rebuild",
  allowDataLossConfirmation: "",
  extraBackupPaths: [],
};

function connectionLabel(connection) {
  return connection?.name?.trim() || `${connection?.host || "SQL Server"}${connection?.database ? ` / ${connection.database}` : ""}`;
}

function kindLabel(t, kind) {
  return t(`operations_kind_${kind}`);
}

function OperationJobs({ jobs, onRefresh }) {
  const { t } = useT();
  const [workingId, setWorkingId] = useState("");

  const downloadDiagnostic = async (job) => {
    const path = await save({
      defaultPath: `sage-data-bridge-${job.kind}-${job.id.slice(0, 8)}-diagnostic.json`,
      filters: [{ name: "Diagnostic bundle", extensions: ["json"] }],
    });
    if (!path) return;
    setWorkingId(job.id);
    try {
      await exportOperationDiagnostic(job.id, path);
    } finally {
      setWorkingId("");
    }
  };

  if (!jobs.length) {
    return <div className="operations-empty">{t("operations_empty_jobs")}</div>;
  }

  return (
    <div className="operations-job-list">
      {jobs.slice(0, 12).map((job) => {
        const percent = jobPercent(job);
        const log = latestLogMessage(job);
        return (
          <article className={`operations-job ${job.status}`} key={job.id}>
            <div className="operations-job-head">
              <div>
                <strong>{job.label}</strong>
                <span>{t("operations_status", job.status, job.phase)}</span>
              </div>
              <b>{percent}%</b>
            </div>
            <div className="operations-progress"><span style={{ width: `${percent}%` }} /></div>
            {job.totalRows ? <div className="operations-job-meta">{t("operations_rows_progress", job.processedRows || 0, job.totalRows)}</div> : null}
            {job.tableProgress?.length ? (
              <div className="operations-table-progress">
                {job.tableProgress.map((table) => (
                  <span key={`${table.sourceSchema}.${table.sourceTable}`}>
                    {table.sourceSchema}.{table.sourceTable}: {t("operations_rows_progress", table.transferredRows || 0, table.expectedRows || 0)}{table.validated ? " ✓" : ""}
                  </span>
                ))}
              </div>
            ) : null}
            {log && !job.error ? <div className="operations-job-log">{log}</div> : null}
            {job.error ? <div className="operations-error">{job.error}</div> : null}
            <div className="operations-job-actions">
              {job.cancellable ? <button type="button" className="btn btn-ghost btn-sm" onClick={() => cancelOperationJob(job.id)}>{t("operations_cancel")}</button> : null}
              {["failed", "cancelled", "interrupted"].includes(job.status) && job.kind === "transfer" ? (
                <button type="button" className="btn btn-ghost btn-sm" disabled={workingId === job.id} onClick={async () => {
                  setWorkingId(job.id);
                  try { await resumeOperationJob(job.id); } finally { setWorkingId(""); }
                }}>{t("operations_resume")}</button>
              ) : null}
              {isTerminalJob(job.status) ? (
                <button type="button" className="btn btn-ghost btn-sm" disabled={workingId === job.id} onClick={() => downloadDiagnostic(job)}>{t("operations_support")}</button>
              ) : null}
              <button type="button" className="btn btn-ghost btn-sm" onClick={onRefresh}>{t("operations_refresh_job")}</button>
            </div>
          </article>
        );
      })}
    </div>
  );
}

function DatabaseSelect({ label, value, options, loading, onChange, placeholder }) {
  return (
    <label>
      {label}
      <select value={value} onChange={(event) => onChange(event.target.value)} disabled={loading || !options.length}>
        <option value="">{loading ? "…" : placeholder}</option>
        {options.map((database) => <option key={database} value={database}>{database}</option>)}
      </select>
    </label>
  );
}

export default function OperationsWorkspace({ connections }) {
  const { t } = useT();
  const [kind, setKind] = useState("transfer");
  const [docsOpen, setDocsOpen] = useState(false);
  const [docsAnchor, setDocsAnchor] = useState("");
  const [transfer, setTransfer] = useState(EMPTY_TRANSFER);
  const [backup, setBackup] = useState(EMPTY_BACKUP);
  const [restore, setRestore] = useState(EMPTY_RESTORE);
  const [sourceDatabases, setSourceDatabases] = useState([]);
  const [targetDatabases, setTargetDatabases] = useState([]);
  const [backupDatabases, setBackupDatabases] = useState([]);
  const [sourceTables, setSourceTables] = useState([]);
  const [tableQuery, setTableQuery] = useState("");
  const [hideEmpty, setHideEmpty] = useState(false);
  const [loadingTables, setLoadingTables] = useState(false);
  const [report, setReport] = useState(null);
  const [jobs, setJobs] = useState([]);
  const [templates, setTemplates] = useState([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [templateName, setTemplateName] = useState("");
  const [pathBrowser, setPathBrowser] = useState(null);
  const [preview, setPreview] = useState(null);
  const [inspection, setInspection] = useState(null);
  const [inspecting, setInspecting] = useState(false);
  const [stagePlan, setStagePlan] = useState(null);
  const [staging, setStaging] = useState(false);

  const refresh = useCallback(async () => {
    const [nextJobs, nextTemplates] = await Promise.all([listOperationJobs(), listOperationTemplates()]);
    setJobs(nextJobs);
    setTemplates(nextTemplates);
  }, []);

  useEffect(() => {
    refresh().catch((reason) => setError(String(reason)));
    let cancelled = false;
    let stop;
    listen("operation_job_updated", (event) => {
      const job = event.payload;
      setJobs((current) => [job, ...current.filter((item) => item.id !== job.id)]
        .sort((left, right) => String(right.createdAt).localeCompare(String(left.createdAt))));
    }).then((unlisten) => {
      if (cancelled) unlisten();
      else stop = unlisten;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [refresh]);

  const loadDatabases = useCallback(async (connectionId, setter) => {
    if (!connectionId) {
      setter([]);
      return;
    }
    try {
      setter(await operationsListDatabases(connectionId));
    } catch (reason) {
      setError(String(reason));
      setter([]);
    }
  }, []);

  useEffect(() => { loadDatabases(transfer.sourceConnectionId, setSourceDatabases); }, [loadDatabases, transfer.sourceConnectionId]);
  useEffect(() => { loadDatabases(transfer.targetConnectionId, setTargetDatabases); }, [loadDatabases, transfer.targetConnectionId]);
  useEffect(() => { loadDatabases(backup.connectionId, setBackupDatabases); }, [backup.connectionId, loadDatabases]);

  useEffect(() => {
    if (!restore.connectionId || !restore.backupPath.toLowerCase().endsWith(".bak")) {
      setInspection(null);
      setInspecting(false);
      return;
    }
    setInspecting(true);
    inspectBackup(restore.connectionId, restore.backupPath, restore.extraBackupPaths)
      .then((next) => {
        setInspection(next);
        setRestore((current) => {
          if (current.targetDatabase && salvageTargetAllowed(current.targetDatabase)) return current;
          if (next.suggestedTarget) return { ...current, targetDatabase: next.suggestedTarget, salvage: next.salvageable || current.salvage };
          return current;
        });
        if (next.classification === "access_denied" || isUserProfilePath(restore.backupPath)) {
          previewBackupStage(restore.connectionId, restore.backupPath).then(setStagePlan).catch(() => {});
        }
      })
      .catch((reason) => {
        setInspection(null);
        setError(String(reason));
      })
      .finally(() => setInspecting(false));
  }, [restore.backupPath, restore.connectionId, restore.extraBackupPaths]);

  useEffect(() => {
    if (!backup.connectionId || !backup.database) return;
    listSqlServerPaths(backup.connectionId, null).then((listing) => {
      const nextPath = defaultBackupFileName(backup.database, listing.defaultBackupDirectory || "");
      setBackup((current) => {
        if (current.backupPath && !isUserProfilePath(current.backupPath)) return current;
        return { ...current, backupPath: nextPath };
      });
    }).catch(() => {});
  }, [backup.connectionId, backup.database]);

  useEffect(() => {
    if (!transfer.sourceConnectionId || !transfer.sourceDatabase) {
      setSourceTables([]);
      return;
    }
    setLoadingTables(true);
    operationsListTables(transfer.sourceConnectionId, transfer.sourceDatabase)
      .then(setSourceTables)
      .catch((reason) => setError(String(reason)))
      .finally(() => setLoadingTables(false));
  }, [transfer.sourceConnectionId, transfer.sourceDatabase]);

  const currentSpec = useMemo(() => {
    if (kind === "transfer") return transfer;
    if (kind === "backup") return backup;
    return restore;
  }, [backup, kind, restore, transfer]);

  const visibleTables = useMemo(
    () => visibleOperationTables(sourceTables, tableQuery, hideEmpty),
    [hideEmpty, sourceTables, tableQuery],
  );
  const groupedTables = useMemo(() => groupTablesBySchema(visibleTables), [visibleTables]);
  const advancedOpen = showAdvancedTools(transfer);
  const backupConnection = connections.find((connection) => connection.id === backup.connectionId);
  const restoreConnection = connections.find((connection) => connection.id === restore.connectionId);

  const preflight = async () => {
    setBusy(true);
    setError("");
    setReport(null);
    try {
      const next = kind === "transfer"
        ? await preflightTransfer(currentSpec)
        : kind === "backup"
          ? await preflightBackup(currentSpec)
          : await preflightRestore(currentSpec);
      setReport(next);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const start = async () => {
    if (!report?.allowed) return;
    setBusy(true);
    setError("");
    try {
      if (kind === "transfer") await startTransfer(transfer);
      if (kind === "backup") await startBackup(backup);
      if (kind === "restore") await startRestore(restore);
      await refresh();
      setReport(null);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  const openDocs = (anchor = "") => {
    setDocsAnchor(anchor);
    setDocsOpen(true);
  };

  if (docsOpen) {
    return <OperationsDocumentation initialAnchor={docsAnchor} onBack={() => setDocsOpen(false)} />;
  }

  return (
    <div className="operations-workspace">
      <header className="operations-header">
        <div>
          <h2>{t("operations_title")}</h2>
          <p>{t("operations_subtitle")}</p>
        </div>
        <div className="operations-header-actions">
          <button type="button" className="btn btn-ghost btn-sm" onClick={() => openDocs()}>{t("operations_help")}</button>
          <button type="button" className="btn btn-ghost btn-sm" onClick={() => refresh().catch((reason) => setError(String(reason)))}>{t("operations_refresh")}</button>
        </div>
      </header>

      <div className="operations-tabs" role="tablist">
        {["transfer", "backup", "restore"].map((item) => (
          <button type="button" key={item} className={kind === item ? "active" : ""} onClick={() => { setKind(item); setReport(null); setError(""); }}>
            {kindLabel(t, item)}
          </button>
        ))}
      </div>

      <section className="operations-card">
        {!connections.length ? <div className="operations-warning">{t("operations_no_connections")}</div> : null}

        {kind === "transfer" ? (
          <>
            <div className="operations-form-grid">
              <label>
                {t("operations_source")}
                <select value={transfer.sourceConnectionId} onChange={(event) => setTransfer((current) => ({ ...current, sourceConnectionId: event.target.value, sourceDatabase: "", tables: [], confirmationText: "" }))}>
                  <option value="">{t("operations_select_source")}</option>
                  {connections.map((connection) => <option key={connection.id} value={connection.id}>{connectionLabel(connection)}</option>)}
                </select>
              </label>
              <DatabaseSelect
                label={t("operations_source_database")}
                value={transfer.sourceDatabase}
                options={sourceDatabases}
                loading={false}
                placeholder={t("operations_select_database")}
                onChange={(sourceDatabase) => setTransfer((current) => ({ ...current, sourceDatabase, tables: [], confirmationText: "" }))}
              />
              <label>
                {t("operations_target")}
                <select value={transfer.targetConnectionId} onChange={(event) => setTransfer((current) => ({ ...current, targetConnectionId: event.target.value, targetDatabase: "", confirmationText: "" }))}>
                  <option value="">{t("operations_select_target")}</option>
                  {connections.map((connection) => <option key={connection.id} value={connection.id}>{connectionLabel(connection)}</option>)}
                </select>
              </label>
              <DatabaseSelect
                label={t("operations_target_database")}
                value={transfer.targetDatabase}
                options={targetDatabases}
                loading={false}
                placeholder={t("operations_select_database")}
                onChange={(targetDatabase) => setTransfer((current) => ({ ...current, targetDatabase, confirmationText: "" }))}
              />
              <label>
                {t("operations_batch_size")}
                <input type="number" min="1" max="500" value={transfer.batchSize} onChange={(event) => setTransfer((current) => ({ ...current, batchSize: Math.min(500, Math.max(1, Number(event.target.value) || 1)), confirmationText: "" }))} />
              </label>
              <div className="operations-safe-mode">
                <strong>{t("operations_safe_mode")}</strong>
                <span>{t("operations_safe_mode_hint")}</span>
              </div>
            </div>
            {transferEndpointsClash(transfer) ? <div className="operations-error">{t("operations_same_database")}</div> : null}

            {transfer.sourceConnectionId && transfer.sourceDatabase && transfer.targetConnectionId && transfer.targetDatabase ? (
              <div className="operations-table-picker">
                <div>
                  <strong>{t("operations_select_tables")}</strong>
                  <span>{loadingTables ? t("operations_loading_tables") : t("operations_selected_count", transfer.tables.length)}</span>
                </div>
                <div className="operations-table-toolbar">
                  <input value={tableQuery} onChange={(event) => setTableQuery(event.target.value)} placeholder={t("operations_search_tables")} aria-label={t("operations_search_tables")} />
                  <label className="operations-checkbox"><input type="checkbox" checked={hideEmpty} onChange={(event) => setHideEmpty(event.target.checked)} />{t("operations_hide_empty")}</label>
                  <button type="button" className="btn btn-ghost btn-sm" disabled={!visibleTables.length} onClick={() => {
                    setTransfer((current) => ({ ...current, tables: replaceVisibleTransferTables(current.tables, visibleTables, true), confirmationText: "" }));
                    setReport(null);
                  }}>{t("operations_select_all")}</button>
                  <button type="button" className="btn btn-ghost btn-sm" disabled={!transfer.tables.length} onClick={() => {
                    setTransfer((current) => ({ ...current, tables: [], confirmationText: "" }));
                    setReport(null);
                  }}>{t("operations_clear_tables")}</button>
                </div>
                {Object.entries(groupedTables).map(([schema, tables]) => (
                  <div key={schema} className="operations-schema-group">
                    <strong>{schema}</strong>
                    {tables.map((table) => {
                      const selected = transfer.tables.some((item) => selectedTableKey(item) === sourceTableKey(table));
                      return (
                        <label key={sourceTableKey(table)} className="operations-table-option">
                          <input type="checkbox" checked={selected} onChange={() => {
                            setTransfer((current) => ({ ...current, tables: toggleTransferTable(current.tables, table), confirmationText: "" }));
                            setReport(null);
                          }} />
                          {table.schema}.{table.name}
                          <span>
                            {t("operations_table_rows", table.row_count || 0)}
                            {table.has_primary_key === false || table.hasPrimaryKey === false ? ` · ${t("operations_no_pk")}` : ""}
                          </span>
                        </label>
                      );
                    })}
                  </div>
                ))}
                {!visibleTables.length ? <div className="operations-empty">{t("operations_no_tables")}</div> : null}
              </div>
            ) : <div className="operations-empty">{t("operations_pick_databases_first")}</div>}

            {advancedOpen ? (
              <div className="operations-advanced">
                <h3>{t("operations_advanced")}</h3>
                <p>{t("operations_advanced_hint")}</p>
                <div className="operations-advanced-actions">
                  {transfer.tables.slice(0, 8).map((table) => (
                    <button type="button" key={`${table.schema}.${table.table}`} className="btn btn-ghost btn-sm" onClick={async () => {
                      try {
                        setPreview(await previewTransferTable(transfer.sourceConnectionId, transfer.sourceDatabase, table.schema, table.table));
                      } catch (reason) {
                        setError(String(reason));
                      }
                    }}>{t("operations_preview")} {table.schema}.{table.table}</button>
                  ))}
                </div>
                {preview?.columns ? (
                  <div className="operations-preview">
                    <strong>{t("operations_preview")}</strong>
                    <table>
                      <thead>
                        <tr>{preview.columns.map((column) => <th key={column.name}>{column.name}</th>)}</tr>
                      </thead>
                      <tbody>
                        {preview.rows.slice(0, 20).map((row, index) => (
                          <tr key={index}>{row.map((cell, cellIndex) => <td key={cellIndex}>{cell == null ? "NULL" : String(cell)}</td>)}</tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                ) : null}
              </div>
            ) : null}
          </>
        ) : kind === "backup" ? (
          <div className="operations-form-grid">
            <label>
              {t("operations_select_connection")}
              <select value={backup.connectionId} onChange={(event) => setBackup((current) => ({ ...current, connectionId: event.target.value, database: "", confirmationText: "" }))}>
                <option value="">{t("operations_select_connection")}</option>
                {connections.map((connection) => <option key={connection.id} value={connection.id}>{connectionLabel(connection)}</option>)}
              </select>
            </label>
            <DatabaseSelect
              label={t("operations_target_database")}
              value={backup.database}
              options={backupDatabases}
              loading={false}
              placeholder={t("operations_select_database")}
              onChange={(database) => setBackup((current) => ({
                ...current,
                database,
                backupPath: current.backupPath || defaultBackupFileName(database),
                confirmationText: "",
              }))}
            />
            <label className="operations-span-2">
              {t("operations_backup_path")}
              <span className="operations-path-row">
                <input placeholder="D:\\SQLBackups\\company-2026-09-09.bak" value={backup.backupPath} onChange={(event) => setBackup((current) => ({ ...current, backupPath: event.target.value, confirmationText: "" }))} />
                <button type="button" className="btn btn-ghost" disabled={!backup.connectionId} onClick={() => setPathBrowser({ mode: "save", kind: "backup" })}>{t("operations_browse")}</button>
              </span>
              <small>{t("operations_path_caption", backupConnection?.host || "SQL Server")}</small>
              {isUserProfilePath(backup.backupPath) ? <div className="operations-error">{t("operations_user_profile_path")}</div> : null}
            </label>
            <label className="operations-checkbox">
              <input type="checkbox" checked={backup.copyOnly} onChange={(event) => setBackup((current) => ({ ...current, copyOnly: event.target.checked }))} />
              {t("operations_copy_only")}
            </label>
            <div className="operations-safe-mode">
              <strong>{t("operations_server_path")}</strong>
              <span>{t("operations_server_path_hint")}</span>
            </div>
          </div>
        ) : (
          <div className="operations-form-grid">
            <label>
              {t("operations_restore_connection")}
              <select value={restore.connectionId} onChange={(event) => setRestore((current) => ({ ...current, connectionId: event.target.value, confirmationText: "" }))}>
                <option value="">{t("operations_select_connection")}</option>
                {connections.map((connection) => <option key={connection.id} value={connection.id}>{connectionLabel(connection)}</option>)}
              </select>
            </label>
            <label>
              {t("operations_target_database")}
              <input value={restore.targetDatabase} onChange={(event) => setRestore((current) => ({ ...current, targetDatabase: event.target.value, confirmationText: "" }))} />
            </label>
            <label className="operations-span-2">
              {t("operations_backup_path")}
              <span className="operations-path-row">
                <input placeholder="D:\\SQLBackups\\company-2026-09-09.bak" value={restore.backupPath} onChange={(event) => setRestore((current) => ({ ...current, backupPath: event.target.value, confirmationText: "" }))} />
                <button type="button" className="btn btn-ghost" disabled={!restore.connectionId} onClick={() => setPathBrowser({ mode: "open", kind: "restore" })}>{t("operations_browse")}</button>
              </span>
              <small>{t("operations_path_caption", restoreConnection?.host || "SQL Server")}</small>
            </label>
            <label className="operations-checkbox danger">
              <input type="checkbox" checked={restore.replaceExisting} onChange={(event) => setRestore((current) => ({ ...current, replaceExisting: event.target.checked }))} />
              {t("operations_replace")}
            </label>
            <div className="operations-safe-mode">
              <strong>{t("operations_recovery")}</strong>
              <span>{t("operations_recovery_hint")}</span>
            </div>
            <SalvageRescuePanel
              restore={restore}
              setRestore={setRestore}
              inspection={inspection}
              inspecting={inspecting}
              stagePlan={stagePlan}
              staging={staging}
              onAddStripe={(path) => {
                setRestore((current) => {
                  const extras = current.extraBackupPaths || [];
                  if (!path || current.backupPath === path || extras.includes(path)) return current;
                  return { ...current, extraBackupPaths: [...extras, path], confirmationText: "" };
                });
                setReport(null);
                if (restore.connectionId && isUserProfilePath(path)) {
                  previewBackupStage(restore.connectionId, path).then(setStagePlan).catch((reason) => setError(String(reason)));
                }
              }}
              onRemoveStripe={(path) => {
                setRestore((current) => ({
                  ...current,
                  extraBackupPaths: (current.extraBackupPaths || []).filter((item) => item !== path),
                  confirmationText: "",
                }));
                setReport(null);
              }}
              onBrowseSql={() => setPathBrowser({ mode: "open", kind: "restore" })}
              onBrowseStripe={() => setPathBrowser({ mode: "open", kind: "restore-stripe" })}
              onPickLocal={(path) => {
                setRestore((current) => ({ ...current, backupPath: path, extraBackupPaths: [], confirmationText: "" }));
                setStagePlan(null);
                setReport(null);
                if (restore.connectionId && isUserProfilePath(path)) {
                  previewBackupStage(restore.connectionId, path).then(setStagePlan).catch((reason) => setError(String(reason)));
                }
              }}
              onPreviewStage={async () => {
                if (!restore.connectionId || !restore.backupPath) return;
                setStaging(true);
                try {
                  setStagePlan(await previewBackupStage(restore.connectionId, restore.backupPath));
                } catch (reason) {
                  setError(String(reason));
                } finally {
                  setStaging(false);
                }
              }}
              onStagePath={async (path) => {
                if (!restore.connectionId || !path) return;
                setStaging(true);
                try {
                  setStagePlan(await previewBackupStage(restore.connectionId, path));
                } catch (reason) {
                  setError(String(reason));
                } finally {
                  setStaging(false);
                }
              }}
              onConfirmStage={async () => {
                const sourcePath = stagePlan?.sourcePath || restore.backupPath;
                if (!restore.connectionId || !sourcePath) return;
                setStaging(true);
                try {
                  const next = await confirmBackupStage(restore.connectionId, sourcePath);
                  setStagePlan(next);
                  setRestore((current) => {
                    if (current.backupPath === next.sourcePath) {
                      return { ...current, backupPath: next.destinationPath, confirmationText: "" };
                    }
                    return {
                      ...current,
                      extraBackupPaths: current.extraBackupPaths.map((path) => path === next.sourcePath ? next.destinationPath : path),
                      confirmationText: "",
                    };
                  });
                  setReport(null);
                } catch (reason) {
                  setError(String(reason));
                } finally {
                  setStaging(false);
                }
              }}
            />
          </div>
        )}

        <div className="operations-actions">
          <button type="button" className="btn btn-accent" disabled={busy} onClick={preflight}>
            {busy ? t("operations_working") : t("operations_preflight")}
          </button>
          <input aria-label={t("operations_template_ph")} placeholder={t("operations_template_ph")} value={templateName} onChange={(event) => setTemplateName(event.target.value)} />
          <button type="button" className="btn btn-ghost" disabled={busy || !templateName.trim()} onClick={async () => {
            if (!templateName.trim()) return;
            setBusy(true);
            try {
              await saveOperationTemplate({ id: "", kind, label: templateName.trim(), spec: { ...currentSpec, confirmationText: "" }, updatedAt: "" });
              setTemplateName("");
              await refresh();
            } catch (reason) {
              setError(String(reason));
            } finally {
              setBusy(false);
            }
          }}>{t("operations_save_template")}</button>
        </div>

        {error ? <div className="operations-error" role="alert">{error}</div> : null}
        {report ? (
          <div className={`operations-report ${report.allowed ? "allowed" : "blocked"}`}>
            <strong>{report.allowed ? t("operations_preflight_ok") : t("operations_preflight_blocked")}</strong>
            <p>{report.summary}</p>
            {report.sourceEdition || report.targetEdition ? <p>{t("operations_editions", report.sourceEdition || "—", report.targetEdition || "—")}</p> : null}
            {report.backupHeader?.databaseName ? <p>{t("operations_backup_header", report.backupHeader.databaseName, report.backupHeader.backupFinishDate || "")}</p> : null}
            {report.warnings?.map((warning) => <div className="operations-warning" key={warning}>{warning}</div>)}
            {report.errors?.map((item) => <div className="operations-error" key={item}>{item}</div>)}
            {report.suggestedPath ? (
              <button type="button" className="btn btn-accent btn-sm" onClick={() => {
                if (kind === "backup") setBackup((current) => ({ ...current, backupPath: report.suggestedPath, confirmationText: "" }));
                if (kind === "restore") setRestore((current) => ({ ...current, backupPath: report.suggestedPath, confirmationText: "" }));
                setReport(null);
              }}>{t("operations_use_suggested_path")}</button>
            ) : null}
            {!report.allowed ? <button type="button" className="btn btn-ghost btn-sm" onClick={() => openDocs("paths")}>{t("operations_why_blocked")}</button> : null}
            {report.tables?.length ? (
              <div className="operations-checks">
                {report.tables.map((table) => (
                  <span key={`${table.sourceSchema}.${table.sourceTable}`} className={table.compatible ? "ok" : "bad"}>
                    {table.sourceSchema}.{table.sourceTable} → {table.targetSchema}.{table.targetTable} · {t("operations_table_rows", table.sourceRows)}
                    {table.missingParent ? ` · ${t("operations_missing_parent")}` : ""}
                  </span>
                ))}
              </div>
            ) : null}
            {report.orderedTables?.length ? <p>{t("operations_copy_order")}: {report.orderedTables.map((table) => `${table.schema}.${table.table}`).join(" → ")}</p> : null}
            {report.allowed ? (
              <div className="operations-confirm">
                <label>
                  {t("operations_confirm_label", report.requiredConfirmation)}
                  <input value={currentSpec.confirmationText} onChange={(event) => {
                    const confirmationText = event.target.value;
                    if (kind === "transfer") setTransfer((current) => ({ ...current, confirmationText }));
                    if (kind === "backup") setBackup((current) => ({ ...current, confirmationText }));
                    if (kind === "restore") setRestore((current) => ({ ...current, confirmationText }));
                  }} />
                </label>
                <button type="button" className="btn btn-danger" disabled={busy || !confirmationMatches(currentSpec.confirmationText, report.requiredConfirmation)} onClick={start}>
                  {t("operations_start", kindLabel(t, kind))}
                </button>
              </div>
            ) : null}
          </div>
        ) : null}
      </section>

      {templates.length ? (
        <section className="operations-card operations-templates">
          <h3>{t("operations_templates")}</h3>
          {templates.map((template) => (
            <div key={template.id}>
              <span>{template.label} · {kindLabel(t, template.kind)}</span>
              <button type="button" className="btn btn-ghost btn-sm" onClick={() => {
                const spec = { ...template.spec, confirmationText: "" };
                setKind(template.kind);
                if (template.kind === "transfer") setTransfer({ ...EMPTY_TRANSFER, ...spec });
                if (template.kind === "backup") setBackup({ ...EMPTY_BACKUP, ...spec });
                if (template.kind === "restore") setRestore({ ...EMPTY_RESTORE, ...spec, extraBackupPaths: spec.extraBackupPaths || [] });
                setReport(null);
                setError("");
              }}>{t("operations_load")}</button>
              <button type="button" className="btn btn-ghost btn-sm" onClick={async () => { await deleteOperationTemplate(template.id); await refresh(); }}>{t("operations_delete")}</button>
            </div>
          ))}
        </section>
      ) : null}

      <section className="operations-history">
        <h3>{t("operations_history")}</h3>
        <OperationJobs jobs={jobs} onRefresh={() => refresh().catch((reason) => setError(String(reason)))} />
      </section>

      {pathBrowser ? (
        <SqlPathBrowser
          connectionId={pathBrowser.kind === "backup" ? backup.connectionId : restore.connectionId}
          mode={pathBrowser.mode}
          currentPath={pathBrowser.kind === "backup" ? backup.backupPath : restore.backupPath}
          databaseName={pathBrowser.kind === "backup" ? backup.database : restore.targetDatabase}
          onClose={() => setPathBrowser(null)}
          onSelect={(path) => {
            if (pathBrowser.kind === "backup") setBackup((current) => ({ ...current, backupPath: path, confirmationText: "" }));
            else if (pathBrowser.kind === "restore-stripe") {
              setRestore((current) => {
                const extras = current.extraBackupPaths || [];
                if (!path || current.backupPath === path || extras.includes(path)) return current;
                return { ...current, extraBackupPaths: [...extras, path], confirmationText: "" };
              });
            } else setRestore((current) => ({ ...current, backupPath: path, confirmationText: "" }));
            setPathBrowser(null);
            setReport(null);
          }}
        />
      ) : null}
    </div>
  );
}
