import { useEffect, useMemo, useRef, useState } from "react";
import { Upload, Palette, Printer, RotateCcw, Save, Sparkles, X } from "lucide-react";

import {
  getInvoiceTemplates,
  renderInvoiceHtmlPreview,
  saveInvoiceTemplate,
} from "../../hooks/useTauri";
import { useT } from "../../i18n";

const COLOR_PRESETS = [
  { label: "Sage Teal", value: "#1de8c8" },
  { label: "Bleu Pro", value: "#2563eb" },
  { label: "Vert Foret", value: "#16a34a" },
  { label: "Rouge Corp", value: "#dc2626" },
  { label: "Marine", value: "#1e3a5f" },
  { label: "Gris Elegant", value: "#6b7280" },
];

const THEME_PRESETS = [
  {
    id: "modern",
    name: "Moderne",
    primary_color: "#1e3a5f",
    secondary_color: "#2563eb",
    font_family: "Inter",
    font_size_px: 10,
    footer_text: "Merci de votre confiance",
  },
  {
    id: "classic",
    name: "Classique",
    primary_color: "#21486a",
    secondary_color: "#6b7280",
    font_family: "Georgia",
    font_size_px: 10,
    footer_text: "Document genere par Sage Data Bridge",
  },
  {
    id: "minimal",
    name: "Minimaliste",
    primary_color: "#111827",
    secondary_color: "#d1d5db",
    font_family: "Arial",
    font_size_px: 10,
    footer_text: "Merci de votre confiance",
  },
  {
    id: "corporate",
    name: "Corporate",
    primary_color: "#0f8f7c",
    secondary_color: "#1de8c8",
    font_family: "Helvetica",
    font_size_px: 10,
    footer_text: "Votre partenaire de gestion",
  },
];

const SAMPLE_INVOICE = {
  id: "preview-invoice",
  numero: "FAC-202605-0042",
  date: "2026-05-15",
  date_echeance: "2026-06-14",
  tiers_id: "CL-001",
  tiers_code: "CL-001",
  tiers_nom: "Atelier Horizon",
  tiers_adresse: "42 avenue des Tilleuls",
  tiers_cp: "75011",
  tiers_ville: "Paris",
  tiers_siret: "81234567800018",
  tiers_pays: "France",
  tiers_tva_intra: "FR82812345678",
  nature: "Facture",
  statut: "Facture",
  reference: "PO-2026-128",
  devise: "EUR",
  total_ht: 2950,
  total_tva: 590,
  total_ttc: 3540,
  remise_globale: 0,
  is_supplier: false,
  notes: "Installation et parametrage inclus.\nLivraison sous 5 jours ouvres.",
  conditions: "Paiement a 30 jours fin de mois.",
  lignes: [
    {
      id: "line-1",
      ordre: 1,
      article_id: "A-100",
      article_code: "CONSULT-01",
      libelle: "Mission de cadrage et migration comptable",
      quantite: 2,
      unite: "jour",
      prix_ht: 950,
      remise_pct: 0,
      montant_ht: 1900,
      taux_tva: 20,
      montant_tva: 380,
      montant_ttc: 2280,
    },
    {
      id: "line-2",
      ordre: 2,
      article_id: "A-101",
      article_code: "SUPPORT-01",
      libelle: "Pack support et transfert de competences",
      quantite: 3,
      unite: "h",
      prix_ht: 350,
      remise_pct: 0,
      montant_ht: 1050,
      taux_tva: 20,
      montant_tva: 210,
      montant_ttc: 1260,
    },
  ],
  statut_history: [],
};

function createDraft(template = {}) {
  const preset = THEME_PRESETS.find((entry) => entry.id === template.layout) ?? THEME_PRESETS[0];

  return {
    id: template.id ?? "",
    connection_id: template.connection_id ?? "",
    name: template.name ?? "Moderne Bleu",
    primary_color: template.primary_color ?? preset.primary_color,
    secondary_color: template.secondary_color ?? preset.secondary_color,
    font_family: template.font_family ?? preset.font_family,
    font_size_px: template.font_size_px ?? preset.font_size_px,
    show_logo: template.show_logo ?? false,
    logo_base64: template.logo_base64 ?? "",
    company_name: template.company_name ?? "Sage Data Bridge",
    company_address: template.company_address ?? "",
    company_siret: template.company_siret ?? "",
    company_tva: template.company_tva ?? "",
    company_phone: template.company_phone ?? "",
    company_email: template.company_email ?? "",
    company_website: template.company_website ?? "",
    footer_text: template.footer_text ?? preset.footer_text,
    legal_mentions: template.legal_mentions ?? "",
    show_bank_details: template.show_bank_details ?? false,
    bank_iban: template.bank_iban ?? "",
    bank_bic: template.bank_bic ?? "",
    conditions_paiement: template.conditions_paiement ?? "Paiement a 30 jours.",
    mention_tva: template.mention_tva ?? "",
    layout: template.layout ?? preset.id,
  };
}

function dataUrlToBase64(dataUrl) {
  const [, base64 = ""] = dataUrl.split(",");
  return base64;
}

function readFileAsDataUrl(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || ""));
    reader.onerror = () => reject(reader.error || new Error("Unable to read file"));
    reader.readAsDataURL(file);
  });
}

function printIframe(iframe) {
  const win = iframe?.contentWindow;
  if (!win) return;
  win.focus();
  win.print();
}

function PresetCard({ preset, active, onClick }) {
  return (
    <button
      type="button"
      className={`invoice-template-preset ${active ? "active" : ""}`}
      onClick={onClick}
    >
      <div className="invoice-template-preset-thumb">
        <div style={{ background: preset.primary_color }} />
        <div style={{ background: preset.secondary_color }} />
      </div>
      <div>
        <strong>{preset.name}</strong>
        <span>{preset.id}</span>
      </div>
    </button>
  );
}

function TemplateField({ label, children }) {
  return (
    <label className="invoice-template-field">
      <span>{label}</span>
      {children}
    </label>
  );
}

export default function TemplateDesigner({ open, connectionId, onClose }) {
  const { t } = useT();
  const iframeRef = useRef(null);
  const fileInputRef = useRef(null);

  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewHtml, setPreviewHtml] = useState("");
  const [message, setMessage] = useState({ type: "", text: "" });
  const [templates, setTemplates] = useState([]);
  const [selectedTemplateId, setSelectedTemplateId] = useState("builtin-modern");
  const [draft, setDraft] = useState(() => createDraft());

  const selectedTemplate = useMemo(
    () => templates.find((template) => template.id === selectedTemplateId) ?? null,
    [templates, selectedTemplateId],
  );

  useEffect(() => {
    if (!open) return;

    let cancelled = false;
    setLoading(true);
    setMessage({ type: "", text: "" });

    getInvoiceTemplates(connectionId || null)
      .then((items) => {
        if (cancelled) return;
        setTemplates(items);
        const initial = items[0] ?? createDraft();
        setSelectedTemplateId(initial.id || "builtin-modern");
        setDraft(createDraft(initial));
      })
      .catch((error) => {
        if (!cancelled) {
          setMessage({ type: "error", text: String(error) });
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [connectionId, open]);

  useEffect(() => {
    if (!open) return;

    let cancelled = false;
    setPreviewLoading(true);
    renderInvoiceHtmlPreview(SAMPLE_INVOICE, draft)
      .then((html) => {
        if (!cancelled) setPreviewHtml(html);
      })
      .catch((error) => {
        if (!cancelled) {
          setPreviewHtml("");
          setMessage({ type: "error", text: String(error) });
        }
      })
      .finally(() => {
        if (!cancelled) setPreviewLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [draft, open]);

  if (!open) return null;

  const updateDraft = (patch) => {
    setDraft((current) => ({ ...current, ...patch }));
    if (message.text) setMessage({ type: "", text: "" });
  };

  const applyPreset = (preset) => {
    updateDraft({
      layout: preset.id,
      name: draft.id.startsWith("builtin-") || !draft.name.trim() ? preset.name : draft.name,
      primary_color: preset.primary_color,
      secondary_color: preset.secondary_color,
      font_family: preset.font_family,
      font_size_px: preset.font_size_px,
      footer_text: draft.footer_text?.trim() ? draft.footer_text : preset.footer_text,
    });
  };

  const persistTemplate = async (nextTemplate) => {
    if (!connectionId) {
      setMessage({ type: "error", text: t("invoice_template_connection_required") });
      return;
    }

    setSaving(true);
    try {
      const saved = await saveInvoiceTemplate(connectionId, nextTemplate);
      const items = await getInvoiceTemplates(connectionId);
      setTemplates(items);
      setSelectedTemplateId(saved.id);
      setDraft(createDraft(saved));
      setMessage({ type: "success", text: t("invoice_template_saved_ok", saved.name) });
    } catch (error) {
      setMessage({ type: "error", text: String(error) });
    } finally {
      setSaving(false);
    }
  };

  const handleSave = async () => {
    await persistTemplate({ ...draft });
  };

  const handleSaveAs = async () => {
    const name = window.prompt(t("invoice_template_save_as_prompt"), `${draft.name} Copy`);
    if (!name) return;
    await persistTemplate({ ...draft, id: "", name: name.trim() });
  };

  const handleReset = () => {
    if (selectedTemplate) {
      setDraft(createDraft(selectedTemplate));
    } else {
      setDraft(createDraft());
    }
    setMessage({ type: "", text: "" });
  };

  const handleLogoUpload = async (file) => {
    if (!file) return;
    if (file.size > 2 * 1024 * 1024) {
      setMessage({ type: "error", text: t("invoice_template_logo_too_large") });
      return;
    }
    try {
      const dataUrl = await readFileAsDataUrl(file);
      updateDraft({
        show_logo: true,
        logo_base64: dataUrl.startsWith("data:") ? dataUrl : dataUrlToBase64(dataUrl),
      });
    } catch (error) {
      setMessage({ type: "error", text: String(error) });
    }
  };

  return (
    <div className="settings-drawer-overlay invoice-template-overlay" onClick={(event) => event.target === event.currentTarget && onClose()}>
      <div className="invoice-template-shell">
        <div className="invoice-template-header">
          <div>
            <div className="settings-drawer-eyebrow">{t("invoice_template_appearance")}</div>
            <h3>{t("invoice_template_title")}</h3>
          </div>
          <button type="button" className="panel-header-btn" onClick={onClose} title={t("close")}>
            <X size={14} />
          </button>
        </div>

        <div className="invoice-template-body">
          <aside className="invoice-template-controls">
            <section className="invoice-template-section">
              <div className="invoice-template-section-title">
                <Sparkles size={14} />
                <span>{t("invoice_template_saved")}</span>
              </div>
              <div className="invoice-template-saved-list">
                {loading ? <div className="sidebar-empty-copy">{t("loading")}</div> : templates.map((template) => (
                  <button
                    key={template.id}
                    type="button"
                    className={`invoice-template-saved-item ${selectedTemplateId === template.id ? "active" : ""}`}
                    onClick={() => {
                      setSelectedTemplateId(template.id);
                      setDraft(createDraft(template));
                    }}
                  >
                    <strong>{template.name}</strong>
                    <span>{template.layout}</span>
                  </button>
                ))}
              </div>
            </section>

            <section className="invoice-template-section">
              <div className="invoice-template-section-title">
                <Palette size={14} />
                <span>{t("invoice_template_base_theme")}</span>
              </div>
              <div className="invoice-template-presets">
                {THEME_PRESETS.map((preset) => (
                  <PresetCard
                    key={preset.id}
                    preset={preset}
                    active={draft.layout === preset.id}
                    onClick={() => applyPreset(preset)}
                  />
                ))}
              </div>
            </section>

            <section className="invoice-template-section">
              <div className="invoice-template-section-title">
                <Palette size={14} />
                <span>{t("invoice_template_colors")}</span>
              </div>
              <div className="invoice-template-grid-two">
                <TemplateField label={t("invoice_template_primary_color")}>
                  <input type="color" value={draft.primary_color} onChange={(event) => updateDraft({ primary_color: event.target.value })} />
                </TemplateField>
                <TemplateField label={t("invoice_template_secondary_color")}>
                  <input type="color" value={draft.secondary_color} onChange={(event) => updateDraft({ secondary_color: event.target.value })} />
                </TemplateField>
              </div>
              <div className="invoice-template-swatches">
                {COLOR_PRESETS.map((swatch) => (
                  <button
                    key={swatch.value}
                    type="button"
                    title={swatch.label}
                    className="invoice-template-swatch"
                    style={{ background: swatch.value }}
                    onClick={() => updateDraft({ primary_color: swatch.value })}
                  />
                ))}
              </div>
            </section>

            <section className="invoice-template-section">
              <div className="invoice-template-section-title">
                <Sparkles size={14} />
                <span>{t("invoice_template_typography")}</span>
              </div>
              <div className="invoice-template-grid-two">
                <TemplateField label={t("invoice_template_font_family")}>
                  <select value={draft.font_family} onChange={(event) => updateDraft({ font_family: event.target.value })}>
                    <option value="Inter">Inter</option>
                    <option value="Helvetica">Helvetica</option>
                    <option value="Georgia">Georgia</option>
                    <option value="Times New Roman">Times New Roman</option>
                    <option value="Arial">Arial</option>
                  </select>
                </TemplateField>
                <TemplateField label={t("invoice_template_font_size")}>
                  <input
                    type="range"
                    min="9"
                    max="12"
                    value={draft.font_size_px}
                    onChange={(event) => updateDraft({ font_size_px: Number(event.target.value) })}
                  />
                </TemplateField>
              </div>
            </section>

            <section className="invoice-template-section">
              <div className="invoice-template-section-title">
                <Sparkles size={14} />
                <span>{t("invoice_template_company_info")}</span>
              </div>
              <TemplateField label={t("invoice_template_theme_name")}>
                <input value={draft.name} onChange={(event) => updateDraft({ name: event.target.value })} />
              </TemplateField>
              <TemplateField label={t("invoice_template_company_name")}>
                <input value={draft.company_name} onChange={(event) => updateDraft({ company_name: event.target.value })} />
              </TemplateField>
              <TemplateField label={t("invoice_template_company_address")}>
                <textarea rows="3" value={draft.company_address} onChange={(event) => updateDraft({ company_address: event.target.value })} />
              </TemplateField>
              <div className="invoice-template-grid-two">
                <TemplateField label={t("invoice_template_company_siret")}>
                  <input value={draft.company_siret} onChange={(event) => updateDraft({ company_siret: event.target.value })} />
                </TemplateField>
                <TemplateField label={t("invoice_template_company_tva")}>
                  <input value={draft.company_tva} onChange={(event) => updateDraft({ company_tva: event.target.value })} />
                </TemplateField>
              </div>
              <div className="invoice-template-grid-two">
                <TemplateField label={t("invoice_template_company_phone")}>
                  <input value={draft.company_phone} onChange={(event) => updateDraft({ company_phone: event.target.value })} />
                </TemplateField>
                <TemplateField label={t("invoice_template_company_email")}>
                  <input value={draft.company_email} onChange={(event) => updateDraft({ company_email: event.target.value })} />
                </TemplateField>
              </div>
              <TemplateField label={t("invoice_template_company_website")}>
                <input value={draft.company_website} onChange={(event) => updateDraft({ company_website: event.target.value })} />
              </TemplateField>
              <button type="button" className="invoice-template-logo-dropzone" onClick={() => fileInputRef.current?.click()}>
                <Upload size={15} />
                <div>
                  <strong>{t("invoice_template_logo")}</strong>
                  <span>{t("invoice_template_logo_hint")}</span>
                </div>
                {draft.logo_base64 ? (
                  <img
                    className="invoice-template-logo-preview"
                    src={draft.logo_base64.startsWith("data:") ? draft.logo_base64 : `data:image/png;base64,${draft.logo_base64}`}
                    alt="logo preview"
                  />
                ) : null}
              </button>
              <input
                ref={fileInputRef}
                type="file"
                accept=".png,.jpg,.jpeg,.svg"
                hidden
                onChange={(event) => handleLogoUpload(event.target.files?.[0] ?? null)}
              />
            </section>

            <section className="invoice-template-section">
              <div className="invoice-template-section-title">
                <Sparkles size={14} />
                <span>{t("invoice_template_footer")}</span>
              </div>
              <TemplateField label={t("invoice_template_footer_text")}>
                <textarea rows="2" value={draft.footer_text} onChange={(event) => updateDraft({ footer_text: event.target.value })} />
              </TemplateField>
              <TemplateField label={t("invoice_template_legal_mentions")}>
                <textarea rows="3" value={draft.legal_mentions} onChange={(event) => updateDraft({ legal_mentions: event.target.value })} />
              </TemplateField>
              <TemplateField label={t("invoice_template_vat_notice")}>
                <textarea rows="2" value={draft.mention_tva} onChange={(event) => updateDraft({ mention_tva: event.target.value })} />
              </TemplateField>
              <TemplateField label={t("invoice_template_payment_conditions")}>
                <textarea rows="2" value={draft.conditions_paiement} onChange={(event) => updateDraft({ conditions_paiement: event.target.value })} />
              </TemplateField>
              <label className="invoice-template-toggle">
                <input
                  type="checkbox"
                  checked={draft.show_bank_details}
                  onChange={(event) => updateDraft({ show_bank_details: event.target.checked })}
                />
                <span>{t("invoice_template_bank_details")}</span>
              </label>
              {draft.show_bank_details ? (
                <div className="invoice-template-grid-two">
                  <TemplateField label="IBAN">
                    <input value={draft.bank_iban} onChange={(event) => updateDraft({ bank_iban: event.target.value })} />
                  </TemplateField>
                  <TemplateField label="BIC">
                    <input value={draft.bank_bic} onChange={(event) => updateDraft({ bank_bic: event.target.value })} />
                  </TemplateField>
                </div>
              ) : null}
            </section>

            {message.text ? (
              <div className={`settings-feedback ${message.type === "error" ? "error" : "success"}`}>
                {message.text}
              </div>
            ) : null}

            <section className="invoice-template-actions">
              <button type="button" className="btn btn-accent" onClick={handleSave} disabled={saving}>
                <Save size={14} />
                {saving ? t("settings_saving") : t("invoice_template_save")}
              </button>
              <button type="button" className="btn" onClick={handleSaveAs} disabled={saving}>
                <Sparkles size={14} />
                {t("invoice_template_save_as")}
              </button>
              <button type="button" className="btn" onClick={handleReset}>
                <RotateCcw size={14} />
                {t("invoice_template_reset")}
              </button>
              <button type="button" className="btn" onClick={() => printIframe(iframeRef.current)} disabled={!previewHtml}>
                <Printer size={14} />
                {t("invoice_template_print_test")}
              </button>
            </section>
          </aside>

          <section className="invoice-template-preview">
            <div className="invoice-template-preview-bar">
              <strong>{t("invoice_template_live_preview")}</strong>
              <span>{previewLoading ? t("loading") : draft.layout}</span>
            </div>
            <div className="invoice-template-preview-frame">
              {previewHtml ? (
                <iframe
                  ref={iframeRef}
                  title="invoice-template-preview"
                  srcDoc={previewHtml}
                  className="invoice-template-iframe"
                />
              ) : (
                <div className="empty-state">
                  <p>{previewLoading ? t("loading") : t("invoice_template_preview_empty")}</p>
                </div>
              )}
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
