import { useEffect, useMemo, useState } from "react";
import { checkUpdate, installUpdate, onUpdaterEvent } from "@tauri-apps/api/updater";
import { relaunch } from "@tauri-apps/api/process";

const isTauriRuntime = () => typeof window !== "undefined" && typeof window.__TAURI_IPC__ === "function";

function formatStatus(status) {
  switch (status) {
    case "PENDING":
      return "Preparing update...";
    case "DONE":
      return "Update installed. Restarting...";
    case "UPTODATE":
      return "Sage Data Bridge is up to date.";
    case "ERROR":
      return "The update could not be installed.";
    default:
      return "Checking for updates...";
  }
}

function formatDate(value) {
  if (!value) return "";
  const parsed = new Date(value);
  if (Number.isNaN(parsed.getTime())) return value;
  return parsed.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

export default function UpdatePrompter() {
  const [visible, setVisible] = useState(false);
  const [manifest, setManifest] = useState(null);
  const [status, setStatus] = useState("CHECKING");
  const [error, setError] = useState("");
  const [installing, setInstalling] = useState(false);

  useEffect(() => {
    if (!isTauriRuntime()) return undefined;

    let cancelled = false;
    let stopUpdaterEvents = null;

    async function initializeUpdater() {
      try {
        stopUpdaterEvents = await onUpdaterEvent(({ error: updaterError, status: nextStatus }) => {
          if (cancelled) return;
          setStatus(nextStatus);
          if (updaterError) {
            setError(updaterError);
            setInstalling(false);
            setVisible(true);
          }
          if (nextStatus === "DONE") {
            relaunch().catch((relaunchError) => {
              setError(String(relaunchError));
              setInstalling(false);
              setVisible(true);
            });
          }
        });

        const update = await checkUpdate();
        if (cancelled) return;

        if (update.shouldUpdate) {
          setManifest(update.manifest ?? null);
          setStatus("PENDING");
          setVisible(true);
        } else {
          setStatus("UPTODATE");
        }
      } catch (updateError) {
        if (cancelled) return;
        console.error("Failed to check for updates", updateError);
      }
    }

    initializeUpdater();

    return () => {
      cancelled = true;
      if (typeof stopUpdaterEvents === "function") stopUpdaterEvents();
    };
  }, []);

  const releaseDate = useMemo(() => formatDate(manifest?.date), [manifest?.date]);

  async function handleInstall() {
    setInstalling(true);
    setError("");
    setStatus("PENDING");

    try {
      await installUpdate();
      await relaunch();
    } catch (installError) {
      setError(String(installError));
      setInstalling(false);
      setVisible(true);
    }
  }

  if (!visible) return null;

  const releaseNotes = manifest?.body?.trim();

  return (
    <div className="modal-overlay update-prompter-overlay" role="presentation">
      <div className="modal update-prompter-modal" role="dialog" aria-modal="true" aria-labelledby="update-prompter-title">
        <div className="modal-title" id="update-prompter-title">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M21 12a9 9 0 1 1-2.64-6.36" />
            <path d="M21 3v6h-6" />
          </svg>
          Update available
        </div>

        <div className="modal-body">
          <div className="update-prompter-summary">
            <span className="update-prompter-badge">Sage Data Bridge</span>
            <h3>{manifest?.version ? `Version ${manifest.version}` : "A new version is ready"}</h3>
            {releaseDate ? <p>Published {releaseDate}</p> : null}
          </div>

          {releaseNotes ? (
            <div className="update-prompter-notes">
              <strong>Release notes</strong>
              <p>{releaseNotes}</p>
            </div>
          ) : (
            <p className="update-prompter-muted">Install this update to get the latest fixes and improvements.</p>
          )}

          <div className={`update-prompter-status ${error ? "error" : ""}`}>
            {installing ? <span className="mini-spinner" aria-hidden="true" /> : null}
            <span>{error || formatStatus(status)}</span>
          </div>
        </div>

        <div className="modal-actions">
          <button type="button" className="btn" onClick={() => setVisible(false)} disabled={installing}>
            Later
          </button>
          <button type="button" className="btn btn-accent" onClick={handleInstall} disabled={installing}>
            {installing ? "Installing..." : "Install update"}
          </button>
        </div>
      </div>
    </div>
  );
}
