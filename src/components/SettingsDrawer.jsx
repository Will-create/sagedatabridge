import { useEffect, useState } from "react";
import {
  getSettings,
  hasAdminPassword,
  hasPin,
  removeAdminPassword,
  removePin,
  saveSettings,
  setAdminPassword,
  setPin,
  verifyAdminPassword,
  verifyPin,
} from "../hooks/useTauri";
import { useT } from "../i18n";

function DrawerToggle({ label, hint, checked, disabled = false, onChange }) {
  return (
    <button
      type="button"
      className={`settings-toggle ${checked ? "active" : ""} ${disabled ? "disabled" : ""}`}
      onClick={() => !disabled && onChange(!checked)}
      disabled={disabled}
    >
      <div className="settings-toggle-copy">
        <strong>{label}</strong>
        {hint ? <span>{hint}</span> : null}
      </div>
      <span className="settings-switch" aria-hidden="true">
        <span />
      </span>
    </button>
  );
}

function SecretField({ label, value, onChange, placeholder, inputMode = undefined, type = "password" }) {
  return (
    <label className="settings-secret-field">
      <span>{label}</span>
      <input
        type={type}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={placeholder}
        inputMode={inputMode}
      />
    </label>
  );
}

function validatePin(pin, confirmPin, t) {
  if (!/^\d{4,12}$/.test(pin)) return t("settings_pin_error_format");
  if (pin !== confirmPin) return t("settings_pin_error_match");
  return "";
}

function validatePassword(password, confirmPassword, t) {
  if (password.length < 4) return t("settings_admin_error_short");
  if (password !== confirmPassword) return t("settings_admin_error_match");
  return "";
}

function TabButton({ active, label, onClick }) {
  return (
    <button type="button" className={`settings-tab-btn ${active ? "active" : ""}`} onClick={onClick}>
      {label}
    </button>
  );
}

export default function SettingsDrawer({
  open,
  onClose,
  adminMode,
  onAdminModeChange,
  showTablePanel,
  onShowTablePanelChange,
  showToolbar,
  onShowToolbarChange,
  appVersion,
  onOpenAbout,
  onLockNow,
  activeConnectionId,
  onOpenInvoiceAppearance,
}) {
  const { lang, setLang, t } = useT();
  const [activeTab, setActiveTab] = useState("general");
  const [loadingSecurity, setLoadingSecurity] = useState(true);
  const [pinConfigured, setPinConfigured] = useState(false);
  const [adminPasswordConfigured, setAdminPasswordConfigured] = useState(false);
  const [pinStatus, setPinStatus] = useState({ error: "", success: "", loading: false });
  const [adminStatus, setAdminStatus] = useState({ error: "", success: "", loading: false });
  const [adminUnlock, setAdminUnlock] = useState({ visible: false, password: "", error: "", loading: false });
  const [pinForm, setPinForm] = useState({ current: "", next: "", confirm: "" });
  const [adminForm, setAdminForm] = useState({ current: "", next: "", confirm: "" });

  const [settingsLoading, setSettingsLoading] = useState(false);
  const [settingsStatus, setSettingsStatus] = useState({ error: "", success: "" });
  const [settingsForm, setSettingsForm] = useState({
    query_timeout_secs: 60,
    dashboard_timeout_secs: 90,
    login_timeout_secs: 60,
    account_ar: "411000",
    account_sales: "701000",
    account_vat: "443000",
    invoice_units: ["Pce", "Kg", "L", "H", "Jour"],
    invoice_vat_rates: [18, 20, 10, 5.5, 0],
    invoice_default_unit: "Pce",
    invoice_default_vat_rate: 18,
    invoice_default_currency: "XOF",
    invoice_default_payment_terms: "",
    invoice_extra_taxes: [],
    tax_types: [
      { id: "vat", name: "TVA", rate: 18, account: "443000", active: true },
      { id: "bic", name: "BIC", rate: 0, account: "", active: false },
    ],
  });

  useEffect(() => {
    if (!open) return;

    let cancelled = false;
    setLoadingSecurity(true);
    setSettingsLoading(true);
    Promise.all([hasPin(), hasAdminPassword(), getSettings()])
      .then(([nextPinConfigured, nextAdminConfigured, currentSettings]) => {
        if (cancelled) return;
        setPinConfigured(nextPinConfigured);
        setAdminPasswordConfigured(nextAdminConfigured);
        setSettingsForm(currentSettings);
      })
      .finally(() => {
        if (!cancelled) {
          setLoadingSecurity(false);
          setSettingsLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [open]);

  useEffect(() => {
    if (!open) {
      setActiveTab("general");
      setPinStatus({ error: "", success: "", loading: false });
      setAdminStatus({ error: "", success: "", loading: false });
      setAdminUnlock({ visible: false, password: "", error: "", loading: false });
      setPinForm({ current: "", next: "", confirm: "" });
      setAdminForm({ current: "", next: "", confirm: "" });
      setSettingsStatus({ error: "", success: "" });
    }
  }, [open]);

  if (!open) return null;

  const handleAdminToggle = async (nextValue) => {
    if (!nextValue) {
      setAdminUnlock({ visible: false, password: "", error: "", loading: false });
      onAdminModeChange(false);
      return;
    }

    if (!adminPasswordConfigured) {
      setAdminUnlock({
        visible: true,
        password: "",
        error: t("settings_admin_enable_missing"),
        loading: false,
      });
      return;
    }

    setAdminUnlock({ visible: true, password: "", error: "", loading: false });
  };

  const submitAdminUnlock = async () => {
    if (!adminUnlock.password.trim()) {
      setAdminUnlock((current) => ({ ...current, error: t("settings_admin_unlock_required") }));
      return;
    }

    setAdminUnlock((current) => ({ ...current, loading: true, error: "" }));
    try {
      const ok = await verifyAdminPassword(adminUnlock.password);
      if (!ok) {
        setAdminUnlock((current) => ({
          ...current,
          password: "",
          loading: false,
          error: t("settings_admin_unlock_wrong"),
        }));
        return;
      }
      setAdminUnlock({ visible: false, password: "", error: "", loading: false });
      onAdminModeChange(true);
    } catch (error) {
      setAdminUnlock((current) => ({ ...current, loading: false, error: String(error) }));
    }
  };

  const savePin = async () => {
    const error = validatePin(pinForm.next, pinForm.confirm, t);
    if (error) {
      setPinStatus({ error, success: "", loading: false });
      return;
    }

    setPinStatus({ error: "", success: "", loading: true });

    try {
      if (pinConfigured) {
        const ok = await verifyPin(pinForm.current);
        if (!ok) {
          setPinStatus({ error: t("settings_pin_error_current"), success: "", loading: false });
          return;
        }
      }

      await setPin(pinForm.next);
      setPinConfigured(true);
      setPinForm({ current: "", next: "", confirm: "" });
      setPinStatus({ error: "", success: t("settings_pin_saved"), loading: false });
    } catch (error) {
      setPinStatus({ error: String(error), success: "", loading: false });
    }
  };

  const clearPin = async () => {
    if (!pinConfigured) return;
    setPinStatus({ error: "", success: "", loading: true });

    try {
      const ok = await verifyPin(pinForm.current);
      if (!ok) {
        setPinStatus({ error: t("settings_pin_error_current"), success: "", loading: false });
        return;
      }
      await removePin();
      setPinConfigured(false);
      setPinForm({ current: "", next: "", confirm: "" });
      setPinStatus({ error: "", success: t("settings_pin_removed"), loading: false });
    } catch (error) {
      setPinStatus({ error: String(error), success: "", loading: false });
    }
  };

  const saveAdminPassword = async () => {
    const error = validatePassword(adminForm.next, adminForm.confirm, t);
    if (error) {
      setAdminStatus({ error, success: "", loading: false });
      return;
    }

    setAdminStatus({ error: "", success: "", loading: true });

    try {
      if (adminPasswordConfigured) {
        const ok = await verifyAdminPassword(adminForm.current);
        if (!ok) {
          setAdminStatus({ error: t("settings_admin_error_current"), success: "", loading: false });
          return;
        }
      }

      await setAdminPassword(adminForm.next);
      setAdminPasswordConfigured(true);
      setAdminForm({ current: "", next: "", confirm: "" });
      setAdminStatus({ error: "", success: t("settings_admin_saved"), loading: false });
    } catch (error) {
      setAdminStatus({ error: String(error), success: "", loading: false });
    }
  };

  const clearAdminPassword = async () => {
    if (!adminPasswordConfigured) return;
    setAdminStatus({ error: "", success: "", loading: true });

    try {
      const ok = await verifyAdminPassword(adminForm.current);
      if (!ok) {
        setAdminStatus({ error: t("settings_admin_error_current"), success: "", loading: false });
        return;
      }

      await removeAdminPassword();
      setAdminPasswordConfigured(false);
      setAdminForm({ current: "", next: "", confirm: "" });
      setAdminUnlock({ visible: false, password: "", error: "", loading: false });
      if (adminMode) onAdminModeChange(false);
      setAdminStatus({ error: "", success: t("settings_admin_removed"), loading: false });
    } catch (error) {
      setAdminStatus({ error: String(error), success: "", loading: false });
    }
  };

  const handleSaveSettings = async () => {
    setSettingsLoading(true);
    setSettingsStatus({ error: "", success: "" });
    try {
      const parseList = (value) => Array.isArray(value)
        ? value.map((item) => String(item).trim()).filter(Boolean)
        : String(value || "").split(",").map((item) => item.trim()).filter(Boolean);
      const parseRates = (value) => (Array.isArray(value) ? value : String(value || "").split(","))
        .map((item) => Number(String(item).trim().replace(",", ".")))
        .filter((item) => Number.isFinite(item));
      const parseExtraTaxes = (value, fallback) => {
        if (!value) return Array.isArray(fallback) ? fallback : [];
        return String(value).split(",").map((item) => {
          const [name, rate, enabled] = item.split(":").map((part) => part.trim());
          return { name, rate: Number(String(rate || 0).replace(",", ".")), account: "", enabled: enabled !== "off" };
        }).filter((tax) => tax.name && Number.isFinite(tax.rate));
      };
      const parseTaxTypes = (value, fallback) => {
        if (!value) return Array.isArray(fallback) ? fallback : [];
        return String(value).split(",").map((item) => {
          const [id, name, rate, account, active] = item.split(":").map((part) => part.trim());
          return {
            id: id || name,
            name: name || id,
            rate: Number(String(rate || 0).replace(",", ".")),
            account: account || "",
            active: active !== "off",
          };
        }).filter((tax) => tax.id && tax.name && Number.isFinite(tax.rate));
      };
      await saveSettings({
        query_timeout_secs: Number(settingsForm.query_timeout_secs),
        dashboard_timeout_secs: Number(settingsForm.dashboard_timeout_secs),
        login_timeout_secs: Number(settingsForm.login_timeout_secs),
        account_ar: String(settingsForm.account_ar),
        account_sales: String(settingsForm.account_sales),
        account_vat: String(settingsForm.account_vat),
        invoice_units: parseList(settingsForm.invoice_units),
        invoice_vat_rates: parseRates(settingsForm.invoice_vat_rates),
        invoice_default_unit: String(settingsForm.invoice_default_unit || ""),
        invoice_default_vat_rate: Number(settingsForm.invoice_default_vat_rate),
        invoice_default_currency: String(settingsForm.invoice_default_currency || "XOF").toUpperCase(),
        invoice_default_payment_terms: String(settingsForm.invoice_default_payment_terms || ""),
        invoice_extra_taxes: parseExtraTaxes(settingsForm.invoice_extra_taxes_raw, settingsForm.invoice_extra_taxes),
        tax_types: parseTaxTypes(settingsForm.tax_types_raw, settingsForm.tax_types),
      });
      setSettingsStatus({ error: "", success: t("settings_saved") || "Settings saved!" });
    } catch (err) {
      setSettingsStatus({ error: String(err), success: "" });
    } finally {
      setSettingsLoading(false);
    }
  };

  return (
    <div className="settings-drawer-overlay" onClick={(event) => event.target === event.currentTarget && onClose()}>
      <aside className="settings-drawer settings-drawer-wide">
        <div className="settings-drawer-header">
          <div>
            <div className="settings-drawer-eyebrow">{t("settings_title")}</div>
            <h3>{t("settings_subtitle")}</h3>
          </div>
          <button type="button" className="panel-header-btn" onClick={onClose} title={t("close")}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M18 6L6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div className="settings-tabs">
          <TabButton active={activeTab === "general"} label={t("settings_tab_general")} onClick={() => setActiveTab("general")} />
          <TabButton active={activeTab === "security"} label={t("settings_tab_security")} onClick={() => setActiveTab("security")} />
          <TabButton active={activeTab === "advanced"} label={t("settings_tab_advanced") || "Advanced"} onClick={() => setActiveTab("advanced")} />
          <TabButton active={activeTab === "about"} label={t("settings_tab_about")} onClick={() => setActiveTab("about")} />
        </div>

        <div className="settings-drawer-body settings-drawer-body-tabbed">
          {activeTab === "general" ? (
            <div className="settings-panel-grid">
              <DrawerToggle
                label={t("settings_admin_mode")}
                hint={t("settings_admin_mode_hint")}
                checked={adminMode}
                onChange={handleAdminToggle}
              />

              {adminUnlock.visible ? (
                <div className="settings-secret-card">
                  <div className="settings-secret-copy">
                    <strong>{t("settings_admin_unlock_title")}</strong>
                    <span>{t("settings_admin_unlock_hint")}</span>
                  </div>
                  <SecretField
                    label={t("settings_admin_password")}
                    value={adminUnlock.password}
                    onChange={(value) => setAdminUnlock((current) => ({ ...current, password: value, error: "" }))}
                    placeholder={t("settings_admin_password_ph")}
                  />
                  {adminUnlock.error ? <div className="settings-feedback error">{adminUnlock.error}</div> : null}
                  <div className="settings-actions">
                    <button
                      type="button"
                      className="btn btn-ghost"
                      onClick={() => setAdminUnlock({ visible: false, password: "", error: "", loading: false })}
                    >
                      {t("cancel")}
                    </button>
                    <button type="button" className="btn btn-accent" onClick={submitAdminUnlock} disabled={adminUnlock.loading}>
                      {adminUnlock.loading ? t("settings_saving") : t("settings_admin_unlock_btn")}
                    </button>
                  </div>
                </div>
              ) : null}

              <DrawerToggle
                label={t("settings_show_tables")}
                hint={t("settings_show_tables_hint")}
                checked={showTablePanel}
                disabled={!adminMode}
                onChange={onShowTablePanelChange}
              />

              <DrawerToggle
                label={t("settings_show_toolbar")}
                hint={t("settings_show_toolbar_hint")}
                checked={showToolbar}
                disabled={!adminMode}
                onChange={onShowToolbarChange}
              />

              <div className="settings-language-card">
                <div className="settings-language-copy">
                  <strong>{t("settings_language")}</strong>
                  <span>{t("settings_language_hint")}</span>
                </div>
                <div className="settings-language-switch" role="group" aria-label={t("settings_language")}>
                  <button type="button" className={lang === "fr" ? "active" : ""} onClick={() => setLang("fr")}>
                    FR
                  </button>
                  <button type="button" className={lang === "en" ? "active" : ""} onClick={() => setLang("en")}>
                    EN
                  </button>
                </div>
              </div>

              <div className="settings-version-card">
                <span>{t("settings_version")}</span>
                <strong>v{appVersion}</strong>
              </div>

              <button
                type="button"
                className="settings-link-card"
                onClick={onOpenInvoiceAppearance}
                disabled={!activeConnectionId}
              >
                <div className="settings-secret-copy">
                  <strong>{t("invoice_template_appearance")}</strong>
                  <span>{activeConnectionId ? t("invoice_template_settings_hint") : t("invoice_template_connection_required")}</span>
                </div>
                <span className="settings-link-arrow">›</span>
              </button>
            </div>
          ) : null}

          {activeTab === "advanced" ? (
            <div className="settings-panel-grid">
              <div className="settings-secret-card">
                <div className="settings-secret-copy">
                  <strong>{t("settings_timeouts_title") || "Database Timeouts"}</strong>
                  <span>{t("settings_timeouts_hint") || "Configure request durations for large databases."}</span>
                </div>

                <div className="settings-field-row" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                  <SecretField
                    label={t("settings_query_timeout") || "Query Timeout (s)"}
                    value={settingsForm.query_timeout_secs}
                    type="number"
                    onChange={(val) => setSettingsForm(s => ({ ...s, query_timeout_secs: val }))}
                    inputMode="numeric"
                  />
                  <SecretField
                    label={t("settings_dashboard_timeout") || "Dashboard Timeout (s)"}
                    value={settingsForm.dashboard_timeout_secs}
                    type="number"
                    onChange={(val) => setSettingsForm(s => ({ ...s, dashboard_timeout_secs: val }))}
                    inputMode="numeric"
                  />
                  <SecretField
                    label={t("settings_login_timeout") || "Login Timeout (s)"}
                    value={settingsForm.login_timeout_secs}
                    type="number"
                    onChange={(val) => setSettingsForm(s => ({ ...s, login_timeout_secs: val }))}
                    inputMode="numeric"
                  />
                </div>
              </div>

              <div className="settings-secret-card">
                <div className="settings-secret-copy">
                  <strong>{t("settings_accounting_title") || "Accounting Integration"}</strong>
                  <span>{t("settings_accounting_hint") || "Configure ledger account codes for invoice posting."}</span>
                </div>

                <div className="settings-field-row" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                  <SecretField
                    label={t("settings_account_ar") || "Accounts Receivable (Client)"}
                    value={settingsForm.account_ar}
                    type="text"
                    onChange={(val) => setSettingsForm(s => ({ ...s, account_ar: val }))}
                  />
                  <SecretField
                    label={t("settings_account_sales") || "Sales Account (Vente)"}
                    value={settingsForm.account_sales}
                    type="text"
                    onChange={(val) => setSettingsForm(s => ({ ...s, account_sales: val }))}
                  />
                  <SecretField
                    label={t("settings_account_vat") || "VAT Account (TVA)"}
                    value={settingsForm.account_vat}
                    type="text"
                    onChange={(val) => setSettingsForm(s => ({ ...s, account_vat: val }))}
                  />
                </div>
              </div>

              <div className="settings-secret-card">
                <div className="settings-secret-copy">
                  <strong>Invoice defaults</strong>
                  <span>Default units, tax presets, currency and payment terms used when creating invoices.</span>
                </div>

                <div className="settings-field-row" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                  <SecretField
                    label="Units"
                    value={Array.isArray(settingsForm.invoice_units) ? settingsForm.invoice_units.join(", ") : settingsForm.invoice_units}
                    type="text"
                    onChange={(val) => setSettingsForm(s => ({ ...s, invoice_units: val }))}
                  />
                  <SecretField
                    label="VAT rates (%)"
                    value={Array.isArray(settingsForm.invoice_vat_rates) ? settingsForm.invoice_vat_rates.join(", ") : settingsForm.invoice_vat_rates}
                    type="text"
                    onChange={(val) => setSettingsForm(s => ({ ...s, invoice_vat_rates: val }))}
                  />
                  <SecretField
                    label="Default unit"
                    value={settingsForm.invoice_default_unit}
                    type="text"
                    onChange={(val) => setSettingsForm(s => ({ ...s, invoice_default_unit: val }))}
                  />
                  <SecretField
                    label="Default VAT rate"
                    value={settingsForm.invoice_default_vat_rate}
                    type="number"
                    onChange={(val) => setSettingsForm(s => ({ ...s, invoice_default_vat_rate: val }))}
                  />
                  <SecretField
                    label="Default currency"
                    value={settingsForm.invoice_default_currency}
                    type="text"
                    onChange={(val) => setSettingsForm(s => ({ ...s, invoice_default_currency: val }))}
                  />
                  <SecretField
                    label="Default payment terms"
                    value={settingsForm.invoice_default_payment_terms}
                    type="text"
                    onChange={(val) => setSettingsForm(s => ({ ...s, invoice_default_payment_terms: val }))}
                  />
                  <SecretField
                    label="Extra taxes placeholder"
                    value={settingsForm.invoice_extra_taxes_raw ?? (Array.isArray(settingsForm.invoice_extra_taxes)
                      ? settingsForm.invoice_extra_taxes.map((tax) => `${tax.name}:${tax.rate}:${tax.enabled ? "on" : "off"}`).join(", ")
                      : "")}
                    type="text"
                    onChange={(val) => setSettingsForm(s => ({ ...s, invoice_extra_taxes_raw: val }))}
                  />
                  <SecretField
                    label={t("settings_tax_types") || "Tax types"}
                    value={settingsForm.tax_types_raw ?? (Array.isArray(settingsForm.tax_types)
                      ? settingsForm.tax_types.map((tax) => `${tax.id}:${tax.name}:${tax.rate}:${tax.account || ""}:${tax.active === false ? "off" : "on"}`).join(", ")
                      : "")}
                    type="text"
                    onChange={(val) => setSettingsForm(s => ({ ...s, tax_types_raw: val }))}
                  />
                </div>
              </div>

                {settingsStatus.error ? <div className="settings-feedback error">{settingsStatus.error}</div> : null}
                {settingsStatus.success ? <div className="settings-feedback success">{settingsStatus.success}</div> : null}

                <div className="settings-actions">
                  <span />
                  <button type="button" className="btn btn-accent" onClick={handleSaveSettings} disabled={settingsLoading}>
                    {settingsLoading ? t("saving") : t("save")}
                  </button>
                </div>
            </div>
          ) : null}

          {activeTab === "security" ? (
            <div className="settings-panel-grid">
              <div className="settings-secret-card">
                <div className="settings-secret-copy">
                  <strong>{t("settings_pin_title")}</strong>
                  <span>{loadingSecurity ? t("loading") : pinConfigured ? t("settings_pin_configured") : t("settings_pin_not_configured")}</span>
                </div>

                {pinConfigured ? (
                  <SecretField
                    label={t("settings_pin_current")}
                    value={pinForm.current}
                    onChange={(value) => setPinForm((current) => ({ ...current, current: value }))}
                    placeholder={t("settings_pin_current_ph")}
                    inputMode="numeric"
                  />
                ) : null}

                <SecretField
                  label={t("settings_pin_new")}
                  value={pinForm.next}
                  onChange={(value) => setPinForm((current) => ({ ...current, next: value }))}
                  placeholder={t("settings_pin_new_ph")}
                  inputMode="numeric"
                />

                <SecretField
                  label={t("settings_pin_confirm")}
                  value={pinForm.confirm}
                  onChange={(value) => setPinForm((current) => ({ ...current, confirm: value }))}
                  placeholder={t("settings_pin_confirm_ph")}
                  inputMode="numeric"
                />

                {pinStatus.error ? <div className="settings-feedback error">{pinStatus.error}</div> : null}
                {pinStatus.success ? <div className="settings-feedback success">{pinStatus.success}</div> : null}

                <div className="settings-actions">
                  {pinConfigured ? (
                    <button type="button" className="btn btn-ghost btn-danger" onClick={clearPin} disabled={pinStatus.loading}>
                      {t("settings_pin_remove")}
                    </button>
                  ) : <span />}
                  <div className="settings-actions-right">
                    <button type="button" className="btn" onClick={onLockNow} disabled={!pinConfigured || pinStatus.loading}>
                      {t("settings_lock_now")}
                    </button>
                    <button type="button" className="btn btn-accent" onClick={savePin} disabled={pinStatus.loading || loadingSecurity}>
                      {pinStatus.loading ? t("settings_saving") : pinConfigured ? t("settings_pin_update") : t("settings_pin_save")}
                    </button>
                  </div>
                </div>
              </div>

              <div className="settings-secret-card">
                <div className="settings-secret-copy">
                  <strong>{t("settings_admin_password_title")}</strong>
                  <span>{loadingSecurity ? t("loading") : adminPasswordConfigured ? t("settings_admin_configured") : t("settings_admin_not_configured")}</span>
                </div>

                {adminPasswordConfigured ? (
                  <SecretField
                    label={t("settings_admin_current")}
                    value={adminForm.current}
                    onChange={(value) => setAdminForm((current) => ({ ...current, current: value }))}
                    placeholder={t("settings_admin_current_ph")}
                  />
                ) : null}

                <SecretField
                  label={t("settings_admin_new")}
                  value={adminForm.next}
                  onChange={(value) => setAdminForm((current) => ({ ...current, next: value }))}
                  placeholder={t("settings_admin_new_ph")}
                />

                <SecretField
                  label={t("settings_admin_confirm")}
                  value={adminForm.confirm}
                  onChange={(value) => setAdminForm((current) => ({ ...current, confirm: value }))}
                  placeholder={t("settings_admin_confirm_ph")}
                />

                {adminStatus.error ? <div className="settings-feedback error">{adminStatus.error}</div> : null}
                {adminStatus.success ? <div className="settings-feedback success">{adminStatus.success}</div> : null}

                <div className="settings-actions">
                  {adminPasswordConfigured ? (
                    <button type="button" className="btn btn-ghost btn-danger" onClick={clearAdminPassword} disabled={adminStatus.loading}>
                      {t("settings_admin_remove")}
                    </button>
                  ) : <span />}
                  <button type="button" className="btn btn-accent" onClick={saveAdminPassword} disabled={adminStatus.loading || loadingSecurity}>
                    {adminStatus.loading ? t("settings_saving") : adminPasswordConfigured ? t("settings_admin_update") : t("settings_admin_save")}
                  </button>
                </div>
              </div>
            </div>
          ) : null}

          {activeTab === "about" ? (
            <div className="settings-panel-grid">
              <div className="settings-about-card">
                <div className="settings-secret-copy">
                  <strong>{t("settings_about_title")}</strong>
                  <span>{t("settings_about_hint")}</span>
                </div>
                <button type="button" className="btn btn-accent" onClick={onOpenAbout}>
                  {t("settings_about_open")}
                </button>
              </div>

              <div className="settings-version-card">
                <span>{t("settings_version")}</span>
                <strong>v{appVersion}</strong>
              </div>
            </div>
          ) : null}
        </div>
      </aside>

    </div>
  );
}
