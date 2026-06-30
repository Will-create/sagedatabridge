import { useEffect, useMemo, useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import {
  connectDb,
  detectSageEdition,
  discoverDatabases,
  getDatabases,
  revealConnectionPassword,
  saveConnection,
  saveFieldHistory,
  scanNetworkForSqlServers,
  testConnection,
  testMapping,
} from "../hooks/useTauri";
import { useT } from "../i18n";
import AutocompleteInput from "./AutocompleteInput";

const MASKED_PASSWORD = "••••••••";

const DEFAULT_CONN = {
  id: "",
  name: "",
  host: "localhost",
  instance_name: "",
  port: 1433,
  database: "",
  username: "sa",
  password: "",
  use_windows_auth: false,
  trust_cert: true,
  encrypt: false,
  sage_edition: "auto",
  custom_schema: null,
};

function buildConnectionLabel(form, database) {
  const host = form.host.trim();
  const instance = form.instance_name.trim();
  const fullHost = instance ? `${host}\\${instance}` : host;
  const label = form.name.trim();
  if (label) return label;
  if (fullHost && database) return `${fullHost} / ${database}`;
  return fullHost || database || "SQL Server";
}

function normalizeConnection(form, databaseOverride = "") {
  return {
    ...form,
    name: form.name.trim(),
    host: form.host.trim(),
    instance_name: form.instance_name.trim(),
    database: databaseOverride || form.database.trim(),
  };
}

function canReuseSavedConnection(form, existing) {
  if (!existing?.id) return false;

  return (
    form.host.trim() === existing.host.trim()
    && form.instance_name.trim() === (existing.instance_name || "").trim()
    && Number(form.port) === Number(existing.port)
    && Boolean(form.use_windows_auth) === Boolean(existing.use_windows_auth)
    && form.username.trim() === existing.username.trim()
    && Boolean(form.trust_cert) === Boolean(existing.trust_cert)
    && Boolean(form.encrypt) === Boolean(existing.encrypt)
  );
}

const EDITION_LABELS = {
  en: { auto: "Auto-detect", sage100: "Sage 100", sage1000: "Sage 1000", sagex3: "Sage X3", custom: "Custom" },
  fr: { auto: "Auto-détection", sage100: "Sage 100", sage1000: "Sage 1000", sagex3: "Sage X3", custom: "Personnalisé" },
};

const EDITION_OPTIONS = ["auto", "sage100", "sage1000", "sagex3", "custom"];

const CUSTOM_SCHEMA_DEFAULTS = {
  table_ecritures: "", table_comptes: "", table_tiers: "", table_journaux: "",
  col_date: "", col_journal: "", col_compte: "", col_libelle: "",
  col_piece: "", col_lettrage: "", col_debit: "", col_credit: "",
  col_tiers: "", col_compte_num: "", col_compte_lib: "", col_compte_type: "",
  col_tiers_code: "", col_tiers_nom: "", col_tiers_type: "",
};

export default function ConnectionModal({ existing, onSave, onClose }) {
  const { t, lang } = useT();
  const [form, setForm] = useState({ ...DEFAULT_CONN, ...existing });
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);
  const [loadingDatabases, setLoadingDatabases] = useState(false);
  const [statusCard, setStatusCard] = useState(null);
  const [databaseSearch, setDatabaseSearch] = useState("");
  const [databases, setDatabases] = useState(existing?.database ? [existing.database] : []);
  const [selectedDatabase, setSelectedDatabase] = useState(existing?.database || "");
  const [detectionResult, setDetectionResult] = useState(null);
  const [customSchema, setCustomSchema] = useState(existing?.custom_schema || { ...CUSTOM_SCHEMA_DEFAULTS });
  const [mappingPreview, setMappingPreview] = useState(null);
  const [testingMapping, setTestingMapping] = useState(false);
  const [scanningNetwork, setScanningNetwork] = useState(false);
  const [scanResults, setScanResults] = useState(null);
  const [passwordVisible, setPasswordVisible] = useState(false);
  const [revealUnlock, setRevealUnlock] = useState({ visible: false, password: "", loading: false, error: "" });
  const isEdit = !!existing?.id;

  const normalizedHost = form.host.trim();
  const hasNamedInstance =
    form.instance_name.trim() !== ""
    || (normalizedHost.includes("\\") && normalizedHost.split("\\")[1]?.trim() !== "")
    || normalizedHost.toUpperCase() === "SQLEXPRESS";
  const usingSavedConnection = canReuseSavedConnection(form, existing);

  useEffect(() => {
    const handler = (e) => {
      if (scanResults && !e.target.closest(".scan-result-item")) {
        setScanResults(null);
      }
    };
    window.addEventListener("mousedown", handler);
    return () => window.removeEventListener("mousedown", handler);
  }, [scanResults]);

  const filteredDatabases = useMemo(() => {
    const query = databaseSearch.trim().toLowerCase();
    if (!query) return databases;
    return databases.filter((database) => database.toLowerCase().includes(query));
  }, [databaseSearch, databases]);

  const set = (key, value) => {
    setForm((current) => ({ ...current, [key]: value }));
    setStatusCard(null);

    if (!["name", "database", "sage_edition"].includes(key)) {
      setDatabases([]);
      setSelectedDatabase("");
      setDatabaseSearch("");
    }
  };

  const ensureReusableCredentials = () => {
    if (form.use_windows_auth) return true;
    if (form.password !== MASKED_PASSWORD) return true;
    if (usingSavedConnection) return true;

    setStatusCard({ ok: false, msg: t("conn_reenter_password") });
    return false;
  };

  const handlePasswordToggle = () => {
    if (passwordVisible) {
      setPasswordVisible(false);
      return;
    }

    if (isEdit && form.password === MASKED_PASSWORD) {
      setRevealUnlock({ visible: true, password: "", loading: false, error: "" });
      return;
    }

    setPasswordVisible(true);
  };

  const handleRevealSavedPassword = async () => {
    if (!existing?.id || revealUnlock.loading) return;
    if (!revealUnlock.password.trim()) {
      setRevealUnlock((current) => ({ ...current, error: t("conn_password_reveal_required") }));
      return;
    }

    setRevealUnlock((current) => ({ ...current, loading: true, error: "" }));
    try {
      const password = await revealConnectionPassword(existing.id, revealUnlock.password);
      setForm((current) => ({ ...current, password }));
      setPasswordVisible(true);
      setRevealUnlock({ visible: false, password: "", loading: false, error: "" });
      setStatusCard(null);
    } catch (error) {
      setPasswordVisible(false);
      setRevealUnlock((current) => ({
        ...current,
        loading: false,
        error: String(error),
      }));
    }
  };

  const handleTest = async () => {
    if (!form.host.trim()) {
      setStatusCard({ ok: false, msg: t("conn_err_host") });
      return;
    }

    if (!ensureReusableCredentials()) return;

    setTesting(true);
    setStatusCard(null);
    setDetectionResult(null);

    try {
      const message = usingSavedConnection && form.password === MASKED_PASSWORD
        ? await connectDb(existing.id)
        : await testConnection(normalizeConnection(form));

      setStatusCard({ ok: true, msg: message });

      // Run detection silently after successful test on saved connections
      if (existing?.id && form.sage_edition === "auto") {
        try {
          const result = await detectSageEdition(existing.id);
          setDetectionResult(result);
        } catch {}
      }
    } catch (error) {
      setStatusCard({ ok: false, msg: String(error) });
    } finally {
      setTesting(false);
    }
  };

  const handleTestMapping = async () => {
    if (!form.host.trim() || !selectedDatabase) {
      alert(t("conn_err_database"));
      return;
    }

    setTestingMapping(true);
    setMappingPreview(null);
    try {
      const payload = normalizeConnection(form, selectedDatabase);
      const data = await testMapping(payload, customSchema);
      setMappingPreview(data);
    } catch (error) {
      alert(t("error") + ": " + String(error));
    } finally {
      setTestingMapping(false);
    }
  };

  const handleScan = async () => {
    setScanningNetwork(true);
    setScanResults(null);
    try {
      const results = await scanNetworkForSqlServers();
      setScanResults(results);
    } catch (error) {
      alert(t("error") + ": " + String(error));
    } finally {
      setScanningNetwork(false);
    }
  };

  const handleLoadDatabases = async () => {
    if (!form.host.trim()) {
      setStatusCard({ ok: false, msg: t("conn_err_host") });
      return;
    }

    if (!ensureReusableCredentials()) return;

    setLoadingDatabases(true);
    setStatusCard(null);

    try {
      const nextDatabases = usingSavedConnection && form.password === MASKED_PASSWORD
        ? await (async () => {
          await connectDb(existing.id);
          return getDatabases(existing.id);
        })()
        : await discoverDatabases(normalizeConnection(form));

      setDatabases(nextDatabases);
      setSelectedDatabase((current) => (
        nextDatabases.includes(current)
          ? current
          : nextDatabases.includes(existing?.database || "")
            ? existing.database
            : nextDatabases[0] || ""
      ));
      setStatusCard({
        ok: true,
        msg: nextDatabases.length
          ? t("conn_server_ready", nextDatabases.length)
          : t("conn_database_empty"),
      });
    } catch (error) {
      setDatabases([]);
      setSelectedDatabase("");
      setStatusCard({ ok: false, msg: String(error) });
    } finally {
      setLoadingDatabases(false);
    }
  };

  const handleSave = async (event) => {
    event.preventDefault();

    if (!form.host.trim()) {
      alert(t("conn_err_host"));
      return;
    }

    if (!selectedDatabase) {
      alert(t("conn_err_database"));
      return;
    }

    if (!ensureReusableCredentials()) return;

    const payload = normalizeConnection(form, selectedDatabase);
    payload.name = buildConnectionLabel(payload, selectedDatabase);
    if (form.sage_edition === "custom") {
      payload.custom_schema = customSchema;
    }

    setSaving(true);
    try {
      const saved = await saveConnection(payload);
      onSave(saved);
    } catch (error) {
      alert(t("conn_err_save") + String(error));
    } finally {
      setSaving(false);
    }
  };

  const editionLabels = EDITION_LABELS[lang] || EDITION_LABELS.en;

  const detectionBanner = detectionResult ? (
    <div
      style={{
        marginTop: 8,
        padding: "8px 12px",
        borderRadius: "var(--r-md)",
        fontSize: 12,
        background: detectionResult.edition !== "generic"
          ? "rgba(56,189,140,0.1)"
          : "rgba(234,179,8,0.1)",
        border: `1px solid ${detectionResult.edition !== "generic" ? "rgba(56,189,140,0.3)" : "rgba(234,179,8,0.3)"}`,
        color: detectionResult.edition !== "generic" ? "var(--accent)" : "#ca8a04",
      }}
    >
      {detectionResult.edition !== "generic"
        ? t("conn_sage_detected", editionLabels[detectionResult.edition] ?? detectionResult.edition, detectionResult.confidence)
        : t("conn_sage_not_detected")}
      {detectionResult.evidence.length > 0 && (
        <div style={{ marginTop: 6, display: "flex", flexWrap: "wrap", gap: 4 }}>
          {detectionResult.evidence.map((tbl) => (
            <span
              key={tbl}
              style={{
                fontSize: 10.5,
                padding: "1px 6px",
                borderRadius: 4,
                background: "rgba(255,255,255,0.06)",
                fontFamily: "var(--font-mono)",
                opacity: 0.85,
              }}
            >
              {tbl}
            </span>
          ))}
        </div>
      )}
    </div>
  ) : null;

  return (
    <div className="modal-overlay" onClick={(event) => event.target === event.currentTarget && onClose()}>
      <div className="modal" style={{ "--modal-width": "800px", "--modal-min-width": "560px" }}>
        <div className="modal-title">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <ellipse cx="12" cy="5" rx="9" ry="3" />
            <path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3" />
            <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5" />
          </svg>
          {isEdit ? t("conn_title_edit") : t("conn_title_new")}
        </div>

        <form onSubmit={handleSave} className="modal-form">
          <div className="modal-body">
            <div className="connection-step-card">
              <div className="connection-step-header">
                <span className="connection-step-index">1</span>
                <div className="connection-step-copy">
                  <strong>{t("conn_step_server")}</strong>
                  <span>{t("conn_step_server_hint")}</span>
                </div>
              </div>

              <div className="form-row">
                <div className="form-group" style={{ position: "relative" }}>
                  <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-end", marginBottom: 4 }}>
                    <label style={{ marginBottom: 0 }}>{t("conn_host")}</label>
                    <button
                      type="button"
                      className="btn"
                      onClick={handleScan}
                      disabled={scanningNetwork || saving}
                      style={{ fontSize: 10, padding: "2px 8px", height: 20, display: "flex", alignItems: "center", gap: 4 }}
                    >
                      {scanningNetwork ? (
                        <><span className="spinner" style={{ width: 10, height: 10 }} /> {t("scan_scanning")}</>
                      ) : (
                        <>
                          <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                            <circle cx="12" cy="12" r="3" />
                            <path d="M19.07 4.93a10 10 0 0 1 0 14.14M15.54 8.46a5 5 0 0 1 0 7.08" />
                          </svg>
                          {t("scan_btn")}
                        </>
                      )}
                    </button>
                  </div>
                  <AutocompleteInput
                    fieldKey="conn_host"
                    value={form.host}
                    onChange={(val) => set("host", val)}
                    placeholder={t("conn_host_ph")}
                    autoFocus
                  />
                  
                  {scanResults && (
                    <div
                      style={{
                        position: "absolute",
                        top: "100%",
                        left: 0,
                        width: "180%",
                        minWidth: "500px",
                        zIndex: 210,
                        background: "var(--bg-modal)",
                        border: "1px solid var(--border-mid)",
                        borderRadius: "var(--r-md)",
                        boxShadow: "0 8px 32px rgba(0,0,0,0.6)",
                        marginTop: 4,
                        overflow: "hidden",
                      }}
                    >
                      <div style={{ 
                        padding: "4px 10px", 
                        fontSize: 9, 
                        fontWeight: 700,
                        textTransform: "uppercase",
                        letterSpacing: "0.05em",
                        color: "var(--text-lo)", 
                        borderBottom: "1px solid var(--border-mid)", 
                        background: "rgba(255,255,255,0.03)",
                        display: "flex",
                        justifyContent: "space-between"
                      }}>
                        <span>{scanResults.length > 0 ? t("scan_found", scanResults.length) : t("scan_no_results")}</span>
                        <span onClick={() => setScanResults(null)} style={{ cursor: "pointer", opacity: 0.6 }}>×</span>
                      </div>
                      
                      <div style={{ 
                        display: "grid", 
                        gridTemplateColumns: "repeat(3, 1fr)", 
                        maxHeight: 280,
                        overflowY: "auto",
                        background: "var(--bg-modal)"
                      }}>
                        {scanResults.map((inst, idx) => (
                          <div
                            key={`${inst.host}-${inst.instance_name}-${idx}`}
                            onClick={() => {
                              set("host", inst.host);
                              set("instance_name", inst.instance_name);
                              set("port", inst.port);
                              setScanResults(null);
                            }}
                            style={{
                              padding: "6px 10px",
                              fontSize: "11px",
                              cursor: "pointer",
                              borderRight: (idx + 1) % 3 === 0 ? "none" : "1px solid rgba(255,255,255,0.04)",
                              borderBottom: "1px solid rgba(255,255,255,0.04)",
                              transition: "background 0.1s"
                            }}
                            className="scan-result-item"
                            onMouseEnter={(e) => {
                              e.currentTarget.style.background = "var(--bg-hover)";
                            }}
                            onMouseLeave={(e) => {
                              e.currentTarget.style.background = "transparent";
                            }}
                          >
                            <div style={{ fontWeight: 600, color: "var(--text-hi)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                              {inst.host}{inst.instance_name ? `\\${inst.instance_name}` : ""}
                            </div>
                            <div style={{ fontSize: 9, opacity: 0.5, marginTop: 1, display: "flex", justifyContent: "space-between" }}>
                              <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", marginRight: 4 }}>{inst.version.replace("SQL Server ", "")}</span>
                              <span style={{ flexShrink: 0 }}>:{inst.port}</span>
                            </div>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
                <div className="form-group">
                  <label>{t("conn_instance")}</label>
                  <AutocompleteInput
                    fieldKey="conn_instance"
                    value={form.instance_name}
                    onChange={(val) => set("instance_name", val)}
                    placeholder={t("conn_instance_ph")}
                  />
                </div>
              </div>

              <div className="form-row">
                <div className="form-group">
                  <label>{t("conn_port")}</label>
                  <input
                    type="number"
                    value={form.port}
                    onChange={(event) => set("port", parseInt(event.target.value, 10) || 1433)}
                    min={1}
                    max={65535}
                    disabled={hasNamedInstance}
                  />
                  {hasNamedInstance ? (
                    <div className="connection-field-note">{t("conn_port_auto_note")}</div>
                  ) : null}
                </div>
                <div className="form-group">
                  <label>{t("conn_name")}</label>
                  <input
                    type="text"
                    placeholder={t("conn_name_ph")}
                    value={form.name}
                    onChange={(event) => set("name", event.target.value)}
                  />
                </div>
              </div>

              <div className="form-group">
                <label>{t("conn_database")}</label>
                <AutocompleteInput
                  fieldKey="conn_database"
                  value={selectedDatabase}
                  onChange={(val) => setSelectedDatabase(val)}
                  placeholder={t("conn_database_ph")}
                />
                <div className="connection-field-note">{t("conn_database_modal_hint")}</div>
              </div>

              <div className="form-group">
                <label className="checkbox-group" style={{ textTransform: "none", fontWeight: "normal" }}>
                  <input
                    type="checkbox"
                    checked={form.use_windows_auth}
                    onChange={(event) => set("use_windows_auth", event.target.checked)}
                  />
                  {t("conn_windows_auth")}
                </label>
              </div>

              {!form.use_windows_auth ? (
                <div className="form-row">
                  <div className="form-group">
                    <label>{t("conn_username")}</label>
                    <AutocompleteInput
                      fieldKey="conn_username"
                      value={form.username}
                      onChange={(val) => set("username", val)}
                    />
                  </div>
                  <div className="form-group">
                    <label>{t("conn_password")}</label>
                    <div className="password-input-wrap">
                      <input
                        type={passwordVisible ? "text" : "password"}
                        value={form.password}
                        onChange={(event) => set("password", event.target.value)}
                        autoComplete="new-password"
                      />
                      <button
                        type="button"
                        className="btn btn-ghost btn-icon password-toggle"
                        onClick={handlePasswordToggle}
                        disabled={revealUnlock.loading}
                        aria-label={passwordVisible ? "Hide password" : "Show password"}
                        title={passwordVisible ? "Hide password" : "Show password"}
                      >
                        {passwordVisible ? <EyeOff size={15} /> : <Eye size={15} />}
                      </button>
                    </div>
                    {revealUnlock.visible ? (
                      <div className="password-reveal-panel">
                        <input
                          type="password"
                          value={revealUnlock.password}
                          onChange={(event) => setRevealUnlock((current) => ({
                            ...current,
                            password: event.target.value,
                            error: "",
                          }))}
                          placeholder={t("conn_password_reveal_ph")}
                          autoComplete="current-password"
                        />
                        <button
                          type="button"
                          className="btn btn-sm"
                          onClick={handleRevealSavedPassword}
                          disabled={revealUnlock.loading}
                        >
                          {revealUnlock.loading ? t("loading") : t("conn_password_reveal")}
                        </button>
                        <button
                          type="button"
                          className="btn btn-sm btn-ghost"
                          onClick={() => setRevealUnlock({ visible: false, password: "", loading: false, error: "" })}
                          disabled={revealUnlock.loading}
                        >
                          {t("cancel")}
                        </button>
                        {revealUnlock.error ? <div className="password-reveal-error">{revealUnlock.error}</div> : null}
                      </div>
                    ) : null}
                  </div>
                </div>
              ) : null}

              <div className="connection-check-row">
                <label className="checkbox-group" style={{ textTransform: "none", fontWeight: "normal" }}>
                  <input
                    type="checkbox"
                    checked={form.trust_cert}
                    onChange={(event) => set("trust_cert", event.target.checked)}
                  />
                  {t("conn_trust_cert")}
                </label>
                <label className="checkbox-group" style={{ textTransform: "none", fontWeight: "normal" }}>
                  <input
                    type="checkbox"
                    checked={form.encrypt}
                    onChange={(event) => set("encrypt", event.target.checked)}
                  />
                  {t("conn_encrypt")}
                </label>
              </div>

              <div className="connection-step-actions">
                <button type="button" className="btn" onClick={handleTest} disabled={testing || saving || loadingDatabases}>
                  {testing ? <><span className="spinner" style={{ width: 12, height: 12 }} /> {t("conn_testing")}</> : t("conn_test")}
                </button>
                <button type="button" className="btn btn-accent" onClick={handleLoadDatabases} disabled={saving || loadingDatabases}>
                  {loadingDatabases ? t("conn_discovering") : t("conn_discover")}
                </button>
              </div>
            </div>

            {/* ─── Sage Compatibility ─────────────────────────────────── */}
            <div className="connection-step-card">
              <div className="connection-step-header">
                <span className="connection-step-index" style={{ fontSize: 10, letterSpacing: 0 }}>S</span>
                <div className="connection-step-copy">
                  <strong>{t("conn_sage_section")}</strong>
                  <span>{t("conn_sage_section_hint")}</span>
                </div>
              </div>

              {/* Edition segmented selector */}
              <div className="form-group">
                <label style={{ marginBottom: 6 }}>{t("conn_sage_edition")}</label>
                <div style={{ display: "flex", gap: 4, flexWrap: "wrap" }}>
                  {EDITION_OPTIONS.map((opt) => (
                    <button
                      key={opt}
                      type="button"
                      onClick={() => set("sage_edition", opt)}
                      style={{
                        padding: "5px 12px",
                        fontSize: 12,
                        borderRadius: "var(--r-md)",
                        border: `1px solid ${form.sage_edition === opt ? "var(--accent)" : "var(--border-mid)"}`,
                        background: form.sage_edition === opt ? "var(--accent-mute)" : "var(--bg-input)",
                        color: form.sage_edition === opt ? "var(--accent)" : "var(--text-mid)",
                        cursor: "pointer",
                        transition: "all 0.1s",
                      }}
                    >
                      {editionLabels[opt]}
                    </button>
                  ))}
                </div>
              </div>

              {/* Detection result banner — only after Test Connection on a saved connection */}
              {detectionBanner}

              {/* Custom mapping — shown only when sage_edition = "custom" */}
              {form.sage_edition === "custom" && (
                <div style={{ marginTop: 12 }}>
                  <div style={{ fontSize: 11, fontWeight: 600, color: "var(--text-lo)", textTransform: "uppercase", letterSpacing: "0.06em", marginBottom: 10 }}>
                    {t("conn_sage_mapping_title")}
                  </div>
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "8px 16px" }}>
                    {[
                      ["table_ecritures", "conn_sage_table_ecritures"],
                      ["table_comptes", "conn_sage_table_comptes"],
                      ["table_tiers", "conn_sage_table_tiers"],
                      ["table_journaux", "conn_sage_table_journaux"],
                      ["col_date", "conn_sage_col_date"],
                      ["col_journal", "conn_sage_col_journal"],
                      ["col_compte", "conn_sage_col_compte"],
                      ["col_libelle", "conn_sage_col_libelle"],
                      ["col_piece", "conn_sage_col_piece"],
                      ["col_lettrage", "conn_sage_col_lettrage"],
                      ["col_debit", "conn_sage_col_debit"],
                      ["col_credit", "conn_sage_col_credit"],
                    ].map(([field, labelKey]) => (
                      <div key={field} className="form-group" style={{ marginBottom: 0 }}>
                        <label style={{ fontSize: 11 }}>{t(labelKey)}</label>
                        <input
                          type="text"
                          value={customSchema[field] || ""}
                          onChange={(e) => setCustomSchema((s) => ({ ...s, [field]: e.target.value }))}
                          placeholder={field}
                          style={{ fontSize: 12, fontFamily: "var(--font-mono)" }}
                        />
                      </div>
                    ))}
                  </div>

                  <div style={{ marginTop: 16 }}>
                    <button
                      type="button"
                      className="btn"
                      onClick={handleTestMapping}
                      disabled={testingMapping || saving}
                      style={{ fontSize: 11, padding: "4px 10px" }}
                    >
                      {testingMapping ? t("loading") : t("conn_sage_test_mapping")}
                    </button>
                  </div>

                  {mappingPreview && (
                    <div
                      style={{
                        marginTop: 12,
                        maxHeight: 180,
                        overflow: "auto",
                        border: "1px solid var(--border-mid)",
                        borderRadius: "var(--r-sm)",
                        background: "rgba(0,0,0,0.1)",
                      }}
                    >
                      <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 10.5, fontFamily: "var(--font-mono)" }}>
                        <thead>
                          <tr style={{ textAlign: "left", background: "rgba(255,255,255,0.03)" }}>
                            {mappingPreview.columns.map((c) => (
                              <th key={c.name} style={{ padding: "4px 8px", borderBottom: "1px solid var(--border-mid)", fontWeight: 600 }}>
                                {c.name}
                              </th>
                            ))}
                          </tr>
                        </thead>
                        <tbody>
                          {mappingPreview.rows.map((row, i) => (
                            <tr key={i}>
                              {row.map((val, j) => (
                                <td key={j} style={{ padding: "3px 8px", borderBottom: "1px solid rgba(255,255,255,0.02)", whiteSpace: "nowrap" }}>
                                  {val === null ? <span style={{ opacity: 0.3 }}>NULL</span> : String(val)}
                                </td>
                              ))}
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  )}
                </div>
              )}
            </div>

            {/* ─── Database selection ─────────────────────────────────── */}
            <div className="connection-step-card">
              <div className="connection-step-header">
                <span className="connection-step-index">2</span>
                <div className="connection-step-copy">
                  <strong>{t("conn_step_database")}</strong>
                  <span>{t("conn_step_database_hint")}</span>
                </div>
              </div>

              <div className="form-group">
                <label>{t("conn_database_search")}</label>
                <input
                  type="text"
                  placeholder={t("conn_database_search_ph")}
                  value={databaseSearch}
                  onChange={(event) => setDatabaseSearch(event.target.value)}
                  disabled={databases.length === 0}
                />
              </div>

              <div className="connection-db-meta">
                {databases.length ? t("conn_database_found", databases.length) : t("conn_database_waiting")}
              </div>

              <div className="connection-db-list">
                {filteredDatabases.length ? filteredDatabases.map((database) => (
                  <button
                    key={database}
                    type="button"
                    className={`connection-db-item ${selectedDatabase === database ? "active" : ""}`}
                    onClick={() => setSelectedDatabase(database)}
                  >
                    <span className="connection-db-pill" />
                    <span>{database}</span>
                  </button>
                )) : (
                  <div className="connection-db-empty">
                    {databases.length ? t("conn_database_no_match") : t("conn_database_waiting")}
                  </div>
                )}
              </div>
            </div>

            {statusCard ? (
              <div className={`connection-status-card ${statusCard.ok ? "success" : "error"}`}>
                {statusCard.ok ? "✓ " : "✗ "}
                {statusCard.msg}
              </div>
            ) : null}
          </div>

          <div className="modal-actions">
            <div className="connection-modal-footer-hint">{t("conn_modal_footer_hint")}</div>
            <div className="settings-actions-right">
              <button type="button" className="btn btn-ghost" onClick={onClose}>
                {t("cancel")}
              </button>
              <button type="submit" className="btn btn-accent" disabled={saving || !selectedDatabase}>
                {saving ? t("conn_saving") : isEdit ? t("conn_open_edit") : t("conn_open")}
              </button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}
