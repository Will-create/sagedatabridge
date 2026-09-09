import { useEffect, useState } from "react";
import { open, save } from "@tauri-apps/api/dialog";
import { listSqlServerPaths } from "../../hooks/useTauri";
import { useT } from "../../i18n";
import { defaultBackupFileName, isUserProfilePath } from "../operationsModel";

export default function SqlPathBrowser({
  connectionId,
  mode = "save",
  currentPath,
  databaseName,
  onSelect,
  onClose,
}) {
  const { t } = useT();
  const [listing, setListing] = useState(null);
  const [directory, setDirectory] = useState("");
  const [fileName, setFileName] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  const load = async (nextDirectory) => {
    if (!connectionId) return;
    setBusy(true);
    setError("");
    try {
      const next = await listSqlServerPaths(connectionId, nextDirectory || null);
      setListing(next);
      setDirectory(next.directory || "");
      if (mode === "save" && !fileName) {
        setFileName(defaultBackupFileName(databaseName || "database").split("\\").pop());
      }
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    const initialDir = currentPath ? currentPath.replace(/[^\\/]+$/, "").replace(/[\\/]+$/, "") : "";
    load(initialDir);
    if (currentPath?.toLowerCase().endsWith(".bak")) {
      setFileName(currentPath.split(/[\\/]/).pop());
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [connectionId]);

  const hostLabel = listing
    ? `${listing.host}${listing.instanceName ? `\\${listing.instanceName}` : ""}`
    : "";

  const chooseLocal = async () => {
    const options = {
      defaultPath: currentPath || defaultBackupFileName(databaseName || "database"),
      filters: [{ name: "SQL Server backup", extensions: ["bak"] }],
    };
    const path = mode === "save" ? await save(options) : await open({ ...options, multiple: false });
    if (!path) return;
    if (isUserProfilePath(path)) {
      setError("SQL Server cannot write to Desktop, Documents, or other user-profile folders. Pick the instance backup directory instead.");
      return;
    }
    onSelect(path);
  };

  const selectedPath = mode === "save"
    ? (directory ? `${directory.replace(/[\\/]+$/, "")}\\${fileName || defaultBackupFileName(databaseName || "database")}` : fileName)
    : currentPath;

  return (
    <div className="operations-path-overlay" role="dialog" aria-modal="true">
      <div className="operations-path-modal">
        <header>
          <div>
            <strong>{t("operations_browse_title")}</strong>
            <p>{t("operations_browse_host", hostLabel || "SQL Server")}</p>
          </div>
          <button type="button" className="btn btn-ghost btn-sm" onClick={onClose}>{t("operations_browse_close")}</button>
        </header>

        <div className="operations-path-toolbar">
          <button type="button" className="btn btn-ghost btn-sm" disabled={busy || !directory} onClick={() => load("")}>{t("operations_browse_root")}</button>
          <button type="button" className="btn btn-ghost btn-sm" disabled={busy || !directory} onClick={() => {
            const parent = directory.replace(/[\\/]+$/, "").replace(/\\[^\\]+$/, "");
            load(parent || "");
          }}>{t("operations_browse_up")}</button>
          {listing?.localHost ? <button type="button" className="btn btn-ghost btn-sm" onClick={chooseLocal}>{t("operations_browse_this_pc")}</button> : null}
          <span>{directory || listing?.defaultBackupDirectory || t("operations_browse_drives")}</span>
        </div>

        {error ? <div className="operations-error">{error}</div> : null}
        {listing?.warnings?.map((warning) => <div className="operations-warning" key={warning}>{warning}</div>)}

        <div className="operations-path-list">
          {(listing?.entries || []).map((entry) => (
            <button
              type="button"
              key={entry.path}
              className={entry.kind}
              onClick={() => {
                if (entry.kind === "file") {
                  if (mode === "open" || entry.isBackup) onSelect(entry.path);
                  else setFileName(entry.name);
                } else {
                  load(entry.path);
                }
              }}
            >
              <strong>{entry.name}</strong>
              <span>{entry.kind}{entry.isBackup ? " · .bak" : ""}</span>
            </button>
          ))}
          {busy ? <div className="operations-empty">{t("operations_working")}</div> : null}
          {!busy && listing && !listing.entries.length ? <div className="operations-empty">{t("operations_browse_empty")}</div> : null}
        </div>

        {mode === "save" ? (
          <label>
            {t("operations_browse_filename")}
            <input value={fileName} onChange={(event) => setFileName(event.target.value)} />
          </label>
        ) : null}

        <div className="operations-path-actions">
          <button type="button" className="btn btn-ghost" onClick={onClose}>{t("operations_browse_close")}</button>
          <button
            type="button"
            className="btn btn-accent"
            disabled={mode === "save" ? !fileName.toLowerCase().endsWith(".bak") : true}
            onClick={() => selectedPath && onSelect(selectedPath)}
          >
            {t("operations_browse_use")}
          </button>
        </div>
      </div>
    </div>
  );
}
