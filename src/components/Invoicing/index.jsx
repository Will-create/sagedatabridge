import {
  startTransition,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { save } from "@tauri-apps/api/dialog";
import {
  ArrowLeft,
  BadgeEuro,
  Building2,
  ClipboardList,
  Eye,
  FileBox,
  FilePlus2,
  Files,
  FileText,
  Package2,
  PencilLine,
  Printer,
  Receipt,
  Search,
  Trash2,
  Users,
} from "lucide-react";

import {
  comptabiliserInvoice,
  createArticle,
  createInvoice,
  createTiers,
  deleteArticle,
  deleteInvoice,
  deleteTiers,
  exportInvoicePdf,
  getInvoice,
  getInvoiceTemplates,
  listArticles,
  listInvoices,
  listTiers,
  renderInvoiceHtml,
  updateArticle,
  updateInvoice,
  updateTiers,
  updateStatut,
} from "../../hooks/useTauri";
import { useT } from "../../i18n";

const DOCUMENT_TABS = [
  { id: "factures", nature: "Facture", icon: Receipt, labelKey: "invoice_tab_factures" },
  { id: "avoirs", nature: "Avoir", icon: FileText, labelKey: "invoice_tab_avoirs" },
  { id: "proformas", nature: "Proforma", icon: ClipboardList, labelKey: "invoice_tab_proformas" },
  { id: "commandes", nature: "Commande", icon: FileBox, labelKey: "invoice_tab_commandes" },
];

const DIRECTORY_TABS = [
  { id: "clients", type: "clients", icon: Users, labelKey: "invoice_tab_clients" },
  { id: "fournisseurs", type: "fournisseurs", icon: Building2, labelKey: "invoice_tab_fournisseurs" },
  { id: "articles", icon: Package2, labelKey: "invoice_tab_articles" },
];

const STATUS_COLORS = {
  Brouillon: "neutral",
  Proforma: "warning",
  Bon_Commande: "info",
  Facture: "success",
  Avoir: "orange",
  "Comptabilise": "accent",
  "Comptabilisé": "accent",
};

const STATUS_TRANSITIONS = {
  Brouillon: ["Proforma", "Facture", "Bon_Commande"],
  Proforma: ["Facture"],
  Facture: ["Avoir", "Comptabilise"],
  Comptabilise: [],
  Avoir: [],
};

const EMPTY_TEMPLATE_ID = "builtin-modern";

function round2(value) {
  return Math.round((Number(value) || 0) * 100) / 100;
}

function todayIso() {
  return new Date().toISOString().slice(0, 10);
}

function formatCurrency(value, devise = "EUR") {
  return new Intl.NumberFormat("fr-FR", {
    style: "currency",
    currency: devise || "EUR",
  }).format(Number(value) || 0);
}

function formatDate(value) {
  if (!value) return "—";
  return new Intl.DateTimeFormat("fr-FR").format(new Date(value));
}

function displayStatus(value) {
  if (value === "Comptabilise") return "Comptabilisé";
  return value || "Brouillon";
}

function sanitizeNumber(value) {
  if (value == null || value === "") return 0;
  return Number(String(value).replace(",", ".")) || 0;
}

function buildBlankLine(ordre = 1) {
  return {
    id: `line-${Date.now()}-${ordre}`,
    ordre,
    article_id: "",
    article_code: "",
    libelle: "",
    quantite: 1,
    unite: "",
    prix_ht: 0,
    remise_pct: 0,
    montant_ht: 0,
    taux_tva: 20,
    montant_tva: 0,
    montant_ttc: 0,
  };
}

function buildBlankInvoice(nature = "Facture", template = null) {
  return computeInvoice({
    id: "",
    numero: "",
    date: todayIso(),
    date_echeance: todayIso(),
    tiers_id: "",
    tiers_code: "",
    tiers_nom: "",
    tiers_adresse: "",
    tiers_cp: "",
    tiers_ville: "",
    tiers_siret: "",
    tiers_pays: "",
    tiers_tva_intra: "",
    nature,
    statut: nature === "Proforma" ? "Proforma" : nature === "Commande" ? "Bon_Commande" : "Brouillon",
    reference: "",
    devise: "EUR",
    total_ht: 0,
    total_tva: 0,
    total_ttc: 0,
    remise_globale: 0,
    is_supplier: false,
    lignes: [buildBlankLine(1)],
    notes: "",
    conditions: template?.conditions_paiement || "",
    statut_history: [],
  });
}

function computeInvoice(invoice) {
  const remiseGlobale = Math.min(Math.max(sanitizeNumber(invoice.remise_globale), 0), 100);
  const lignes = (invoice.lignes || []).map((line, index) => {
    const quantite = sanitizeNumber(line.quantite);
    const prixHt = sanitizeNumber(line.prix_ht);
    const remisePct = Math.min(Math.max(sanitizeNumber(line.remise_pct), 0), 100);
    const tauxTva = sanitizeNumber(line.taux_tva);
    const montantHt = round2(quantite * prixHt * (1 - remisePct / 100));
    const montantTva = round2(montantHt * tauxTva / 100);
    const montantTtc = round2(montantHt + montantTva);

    return {
      ...line,
      ordre: index + 1,
      quantite,
      prix_ht: prixHt,
      remise_pct: remisePct,
      taux_tva: tauxTva,
      montant_ht: montantHt,
      montant_tva: montantTva,
      montant_ttc: montantTtc,
    };
  });

  const subtotalHt = round2(lignes.reduce((sum, line) => sum + line.montant_ht, 0));
  const subtotalTva = round2(lignes.reduce((sum, line) => sum + line.montant_tva, 0));
  const factor = 1 - remiseGlobale / 100;
  const totalHt = round2(subtotalHt * factor);
  const totalTva = round2(subtotalTva * factor);
  const totalTtc = round2(totalHt + totalTva);

  return {
    ...invoice,
    devise: invoice.devise || "EUR",
    remise_globale: remiseGlobale,
    lignes,
    total_ht: totalHt,
    total_tva: totalTva,
    total_ttc: totalTtc,
  };
}

function duplicateInvoice(invoice) {
  return computeInvoice({
    ...invoice,
    id: "",
    numero: "",
    reference: "",
    statut: "Brouillon",
    statut_history: [],
    date: todayIso(),
    lignes: invoice.lignes.map((line, index) => ({
      ...line,
      id: `line-copy-${Date.now()}-${index + 1}`,
      ordre: index + 1,
    })),
  });
}

function getTvaBreakdown(invoice) {
  const breakdown = new Map();
  for (const line of invoice.lignes || []) {
    const rate = round2(line.taux_tva);
    const current = breakdown.get(rate) || { base: 0, tax: 0 };
    breakdown.set(rate, {
      base: round2(current.base + line.montant_ht),
      tax: round2(current.tax + line.montant_tva),
    });
  }
  return Array.from(breakdown.entries())
    .map(([rate, values]) => ({ rate, ...values }))
    .sort((left, right) => right.rate - left.rate);
}

function matchesSearch(haystack, needle) {
  return `${haystack || ""}`.toLowerCase().includes((needle || "").toLowerCase());
}

function SearchSelect({
  placeholder,
  value,
  displayValue,
  options,
  onSelect,
  renderOption,
  getSearchText = (option) => `${option.code || option.id || ""} ${option.nom || option.libelle || ""}`,
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState(displayValue || "");

  useEffect(() => {
    if (!open) setQuery(displayValue || "");
  }, [displayValue, open]);

  useEffect(() => {
    if (!open) return undefined;
    const handle = () => setOpen(false);
    window.addEventListener("click", handle);
    return () => window.removeEventListener("click", handle);
  }, [open]);

  const filtered = useMemo(() => {
    if (!query.trim()) return options.slice(0, 12);
    return options
      .filter((option) => matchesSearch(getSearchText(option), query))
      .slice(0, 12);
  }, [getSearchText, options, query]);

  return (
    <div className="invoice-search-select" onClick={(event) => event.stopPropagation()}>
      <input
        value={query}
        placeholder={placeholder}
        onFocus={() => setOpen(true)}
        onChange={(event) => {
          setQuery(event.target.value);
          setOpen(true);
        }}
      />
      {open && filtered.length ? (
        <div className="invoice-search-select-menu">
          {filtered.map((option) => (
            <button
              key={option.id || option.code}
              type="button"
              className={`invoice-search-select-option ${value === option.id ? "active" : ""}`}
              onClick={() => {
                onSelect(option);
                setQuery("");
                setOpen(false);
              }}
            >
              {renderOption(option)}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function TabButton({ active, icon: Icon, label, onClick }) {
  return (
    <button type="button" className={`invoice-tab-btn ${active ? "active" : ""}`} onClick={onClick}>
      <Icon size={15} />
      <span>{label}</span>
    </button>
  );
}

function StatusBadge({ statut }) {
  const tone = STATUS_COLORS[statut] || "neutral";
  return <span className={`invoice-status-badge ${tone}`}>{displayStatus(statut)}</span>;
}

function InvoiceCard({ invoice, active, onClick }) {
  return (
    <button type="button" className={`invoice-card ${active ? "active" : ""}`} onClick={onClick}>
      <div className="invoice-card-top">
        <strong>{invoice.numero || "—"}</strong>
        <StatusBadge statut={invoice.statut} />
      </div>
      <div className="invoice-card-meta">{formatDate(invoice.date)}</div>
      <div className="invoice-card-title">{invoice.tiers_nom || invoice.tiers_code || "Tiers inconnu"}</div>
      <div className="invoice-card-bottom">
        <span>{invoice.reference || "Sans reference"}</span>
        <strong>{formatCurrency(invoice.total_ttc, invoice.devise)}</strong>
      </div>
    </button>
  );
}

function PreviewModal({
  html,
  loading,
  templates,
  templateId,
  onTemplateChange,
  onClose,
  onExportPdf,
}) {
  const { t } = useT();
  const iframeRef = useRef(null);

  return (
    <div className="invoice-preview-overlay">
      <div className="invoice-preview-shell">
        <div className="invoice-preview-toolbar">
          <button type="button" className="btn" onClick={onClose}>
            <ArrowLeft size={14} />
            {t("invoice_preview_back")}
          </button>
          <div className="invoice-preview-toolbar-group">
            <select value={templateId} onChange={(event) => onTemplateChange(event.target.value)}>
              {templates.map((template) => (
                <option key={template.id} value={template.id}>
                  {template.name}
                </option>
              ))}
            </select>
            <button type="button" className="btn" onClick={() => iframeRef.current?.contentWindow?.print()} disabled={!html}>
              <Printer size={14} />
              {t("invoice_print")}
            </button>
            <button type="button" className="btn btn-accent" onClick={onExportPdf} disabled={!html}>
              <BadgeEuro size={14} />
              {t("invoice_export_pdf")}
            </button>
          </div>
        </div>
        <div className="invoice-preview-content">
          {loading ? (
            <div className="empty-state">
              <div className="spinner" />
              <p>{t("invoice_preview_loading")}</p>
            </div>
          ) : (
            <iframe ref={iframeRef} title="invoice-preview" srcDoc={html} className="invoice-preview-iframe" />
          )}
        </div>
      </div>
    </div>
  );
}

function DetailRow({ label, value, mono = false }) {
  return (
    <div className="invoice-detail-row">
      <span>{label}</span>
      <strong className={mono ? "mono" : ""}>{value || "—"}</strong>
    </div>
  );
}

function TiersPanel({ tier, invoices, loading }) {
  const { t } = useT();

  const totals = useMemo(() => {
    let ca = 0;
    let balance = 0;
    for (const invoice of invoices) {
      if (invoice.nature === "Facture") {
        ca += invoice.total_ttc;
        balance += invoice.total_ttc;
      } else if (invoice.nature === "Avoir") {
        balance -= invoice.total_ttc;
      }
    }
    return { ca: round2(ca), balance: round2(balance) };
  }, [invoices]);

  return (
    <aside className="invoice-directory-panel">
      {!tier ? (
        <div className="empty-state">
          <Users size={28} />
          <p>{t("invoice_tiers_select_hint")}</p>
        </div>
      ) : (
        <>
          <div className="invoice-directory-panel-header">
            <div>
              <h3>{tier.nom}</h3>
              <span>{tier.code}</span>
            </div>
            <StatusBadge statut={tier.type_tiers === "fournisseur" ? "Bon_Commande" : "Facture"} />
          </div>
          <div className="invoice-kpi-grid compact">
            <div className="invoice-kpi-card">
              <span>{t("invoice_total_revenue")}</span>
              <strong>{formatCurrency(totals.ca)}</strong>
            </div>
            <div className="invoice-kpi-card">
              <span>{t("invoice_balance")}</span>
              <strong>{formatCurrency(totals.balance)}</strong>
            </div>
          </div>
          <div className="invoice-detail-card">
            <DetailRow label={t("invoice_city")} value={[tier.cp, tier.ville].filter(Boolean).join(" ")} />
            <DetailRow label="SIRET" value={tier.siret} mono />
            <DetailRow label="Email" value={tier.email} />
            <DetailRow label={t("invoice_phone")} value={tier.telephone} />
            <DetailRow label={t("invoice_encours")} value={formatCurrency(tier.encours)} />
          </div>
          <div className="invoice-directory-panel-list">
            <div className="invoice-directory-panel-title">{t("invoice_related_documents")}</div>
            {loading ? <div className="spinner" /> : invoices.map((invoice) => (
              <InvoiceCard key={invoice.id} invoice={invoice} active={false} onClick={() => {}} />
            ))}
          </div>
        </>
      )}
    </aside>
  );
}

function ArticlesPanel({ article }) {
  const { t } = useT();

  return (
    <aside className="invoice-directory-panel">
      {!article ? (
        <div className="empty-state">
          <Package2 size={28} />
          <p>{t("invoice_articles_select_hint")}</p>
        </div>
      ) : (
        <>
          <div className="invoice-directory-panel-header">
            <div>
              <h3>{article.libelle}</h3>
              <span>{article.code}</span>
            </div>
            <StatusBadge statut={article.en_activite ? "Facture" : "Brouillon"} />
          </div>
          <div className="invoice-detail-card">
            <DetailRow label={t("invoice_unit_price")} value={formatCurrency(article.prix_ht)} />
            <DetailRow label={t("invoice_vat_rate")} value={`${round2(article.taux_tva)}%`} />
            <DetailRow label={t("invoice_unit")} value={article.unite} />
            <DetailRow label={t("invoice_reference")} value={article.reference} />
          </div>
        </>
      )}
    </aside>
  );
}

const BLANK_TIERS = {
  id: "",
  code: "",
  nom: "",
  adresse: "",
  cp: "",
  ville: "",
  pays: "",
  siret: "",
  email: "",
  telephone: "",
  tva_intra: "",
  type_tiers: "client",
  encours: 0,
  nb_factures: 0,
  is_local: false,
};
const BLANK_ARTICLE = { id: "", code: "", libelle: "", prix_ht: 0, taux_tva: 20, unite: "", reference: "", en_activite: true };

function TiersFormModal({ initial, onSave, onClose, t }) {
  const [form, setForm] = useState({ ...BLANK_TIERS, ...initial });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const up = (patch) => setForm((f) => ({ ...f, ...patch }));

  async function handleSave() {
    if (!form.code.trim()) { setError(t("invoice_code_required")); return; }
    setSaving(true); setError("");
    try { await onSave(form); onClose(); }
    catch (e) { setError(String(e)); setSaving(false); }
  }

  return (
    <div className="modal-overlay invoice-form-overlay" onClick={onClose}>
      <div className="modal invoice-form-modal" onClick={(e) => e.stopPropagation()}>
        <form className="invoice-form-shell" onSubmit={(event) => {
          event.preventDefault();
          handleSave();
        }}>
          <div className="invoice-form-header">
            <div className="invoice-form-heading">
              <strong>{form.id ? t("invoice_edit_tiers") : t("invoice_new_tiers")}</strong>
            </div>
            <button type="button" className="btn btn-icon" onClick={onClose}>✕</button>
          </div>
          <div className="invoice-form-body">
            <div className="invoice-form-section">
              <div className="invoice-editor-grid invoice-form-grid">
                <label>
                  <span>{t("invoice_tiers_type")}</span>
                  <select value={form.type_tiers} onChange={(e) => up({ type_tiers: e.target.value })}>
                    <option value="client">Client</option>
                    <option value="fournisseur">Fournisseur</option>
                    <option value="les_deux">Les deux</option>
                  </select>
                </label>
                <label>
                  <span>{t("invoice_code")} *</span>
                  <input value={form.code} onChange={(e) => up({ code: e.target.value })} />
                </label>
                <label className="span-2">
                  <span>{t("invoice_name")}</span>
                  <input value={form.nom} onChange={(e) => up({ nom: e.target.value })} />
                </label>
              </div>
            </div>
            <div className="invoice-form-section">
              <div className="invoice-editor-grid invoice-form-grid">
                <label className="span-2">
                  <span>{t("invoice_address")}</span>
                  <textarea rows="3" value={form.adresse} onChange={(e) => up({ adresse: e.target.value })} />
                </label>
                <label>
                  <span>{t("invoice_postal_code")}</span>
                  <input value={form.cp} onChange={(e) => up({ cp: e.target.value })} />
                </label>
                <label>
                  <span>{t("invoice_city")}</span>
                  <input value={form.ville} onChange={(e) => up({ ville: e.target.value })} />
                </label>
                <label>
                  <span>{t("invoice_country")}</span>
                  <input value={form.pays} onChange={(e) => up({ pays: e.target.value })} />
                </label>
                <label>
                  <span>SIRET</span>
                  <input value={form.siret} onChange={(e) => up({ siret: e.target.value })} />
                </label>
                <label>
                  <span>Email</span>
                  <input value={form.email} onChange={(e) => up({ email: e.target.value })} />
                </label>
                <label>
                  <span>{t("invoice_phone")}</span>
                  <input type="tel" value={form.telephone} onChange={(e) => up({ telephone: e.target.value })} />
                </label>
                <label className="span-2">
                  <span>TVA Intracommunautaire</span>
                  <input value={form.tva_intra} onChange={(e) => up({ tva_intra: e.target.value })} />
                </label>
              </div>
            </div>
          </div>
          {error ? <div className="invoice-form-error">{error}</div> : null}
          <div className="invoice-form-footer">
            <button type="button" className="btn" onClick={onClose}>{t("cancel")}</button>
            <button type="submit" className="btn btn-accent" disabled={saving}>
              {saving ? t("saving") : t("save")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

function ArticleFormModal({ initial, onSave, onClose, t }) {
  const [form, setForm] = useState({ ...BLANK_ARTICLE, ...initial });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const up = (patch) => setForm((f) => ({ ...f, ...patch }));

  async function handleSave() {
    if (!form.code.trim()) { setError(t("invoice_code_required")); return; }
    setSaving(true); setError("");
    try { await onSave(form); onClose(); }
    catch (e) { setError(String(e)); setSaving(false); }
  }

  return (
    <div className="modal-overlay invoice-form-overlay" onClick={onClose}>
      <div className="modal invoice-form-modal" onClick={(e) => e.stopPropagation()}>
        <form className="invoice-form-shell" onSubmit={(event) => {
          event.preventDefault();
          handleSave();
        }}>
          <div className="invoice-form-header">
            <div className="invoice-form-heading">
              <strong>{form.id ? t("invoice_edit_article") : t("invoice_new_article")}</strong>
            </div>
            <button type="button" className="btn btn-icon" onClick={onClose}>✕</button>
          </div>
          <div className="invoice-form-body">
            <div className="invoice-form-section">
              <div className="invoice-editor-grid invoice-form-grid">
                <label>
                  <span>{t("invoice_code")} *</span>
                  <input value={form.code} onChange={(e) => up({ code: e.target.value })} />
                </label>
                <label>
                  <span>{t("invoice_reference")}</span>
                  <input value={form.reference} onChange={(e) => up({ reference: e.target.value })} />
                </label>
                <label className="span-2">
                  <span>{t("invoice_label")}</span>
                  <input value={form.libelle} onChange={(e) => up({ libelle: e.target.value })} />
                </label>
              </div>
            </div>
            <div className="invoice-form-section">
              <div className="invoice-editor-grid invoice-form-grid">
                <label>
                  <span>{t("invoice_unit_price")} HT</span>
                  <input type="number" step="0.01" value={form.prix_ht} onChange={(e) => up({ prix_ht: parseFloat(e.target.value) || 0 })} />
                </label>
                <label>
                  <span>{t("invoice_vat_rate")} %</span>
                  <input type="number" step="0.1" value={form.taux_tva} onChange={(e) => up({ taux_tva: parseFloat(e.target.value) || 0 })} />
                </label>
                <label>
                  <span>{t("invoice_unit")}</span>
                  <input value={form.unite} onChange={(e) => up({ unite: e.target.value })} />
                </label>
                <label className="invoice-form-toggle">
                  <input type="checkbox" checked={form.en_activite} onChange={(e) => up({ en_activite: e.target.checked })} />
                  <span>{t("invoice_active")}</span>
                </label>
              </div>
            </div>
          </div>
          {error ? <div className="invoice-form-error">{error}</div> : null}
          <div className="invoice-form-footer">
            <button type="button" className="btn" onClick={onClose}>{t("cancel")}</button>
            <button type="submit" className="btn btn-accent" disabled={saving}>
              {saving ? t("saving") : t("save")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

export default function Invoicing({ connId, schema, onBack, onOpenTemplateDesigner }) {
  const { t } = useT();

  const [activeTab, setActiveTab] = useState("factures");
  const [documents, setDocuments] = useState([]);
  const [documentsLoading, setDocumentsLoading] = useState(false);
  const [selectedId, setSelectedId] = useState("");
  const [selectedInvoice, setSelectedInvoice] = useState(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [mode, setMode] = useState("view");
  const [editorInvoice, setEditorInvoice] = useState(null);
  const [search, setSearch] = useState("");
  const [statutFilter, setStatutFilter] = useState("");
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");
  const [saving, setSaving] = useState(false);
  const [templates, setTemplates] = useState([]);
  const [templateId, setTemplateId] = useState(EMPTY_TEMPLATE_ID);
  const [previewHtml, setPreviewHtml] = useState("");
  const [previewLoading, setPreviewLoading] = useState(false);
  const [tiers, setTiers] = useState([]);
  const [tiersLoading, setTiersLoading] = useState(false);
  const [selectedTier, setSelectedTier] = useState(null);
  const [selectedTierInvoices, setSelectedTierInvoices] = useState([]);
  const [tierDocsLoading, setTierDocsLoading] = useState(false);
  const [tierSearch, setTierSearch] = useState("");
  const [articles, setArticles] = useState([]);
  const [articlesLoading, setArticlesLoading] = useState(false);
  const [selectedArticle, setSelectedArticle] = useState(null);
  const [articleSearch, setArticleSearch] = useState("");
  const [tiersModal, setTiersModal] = useState(null); // null | { initial }
  const [articleModal, setArticleModal] = useState(null); // null | { initial }

  const deferredSearch = useDeferredValue(search);
  const deferredTierSearch = useDeferredValue(tierSearch);
  const deferredArticleSearch = useDeferredValue(articleSearch);
  const documentTab = DOCUMENT_TABS.find((tab) => tab.id === activeTab) ?? null;
  const directoryTab = DIRECTORY_TABS.find((tab) => tab.id === activeTab) ?? null;

  useEffect(() => {
    if (!connId) return;
    getInvoiceTemplates(connId)
      .then((items) => {
        setTemplates(items);
        if (items.length) setTemplateId(items[0].id);
      })
      .catch(() => {
        setTemplates([]);
        setTemplateId(EMPTY_TEMPLATE_ID);
      });
  }, [connId]);

  const refreshTiers = () => {
    if (!connId) return;
    setTiersLoading(true);
    listTiers(connId, "all", null)
      .then(setTiers)
      .catch(() => {})
      .finally(() => setTiersLoading(false));
  };

  const refreshArticles = () => {
    if (!connId) return;
    setArticlesLoading(true);
    listArticles(connId, null)
      .then(setArticles)
      .catch(() => {})
      .finally(() => setArticlesLoading(false));
  };

  useEffect(() => {
    if (!connId) return;

    let cancelled = false;
    setTiersLoading(true);
    setArticlesLoading(true);

    listTiers(connId, "all", null)
      .then((items) => {
        if (!cancelled) setTiers(items);
      })
      .finally(() => {
        if (!cancelled) setTiersLoading(false);
      });

    listArticles(connId, null)
      .then((items) => {
        if (!cancelled) setArticles(items);
      })
      .finally(() => {
        if (!cancelled) setArticlesLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [connId]);

  useEffect(() => {
    if (!connId || !documentTab) return;
    let cancelled = false;
    setDocumentsLoading(true);
    listInvoices(connId, {
      nature: documentTab.nature,
      statut: statutFilter || null,
      dateFrom: dateFrom || null,
      dateTo: dateTo || null,
      search: deferredSearch || null,
    })
      .then((items) => {
        if (cancelled) return;
        setDocuments(items);
        if (!selectedId && items[0] && mode !== "edit") {
          setSelectedId(items[0].id);
        } else if (selectedId && !items.some((item) => item.id === selectedId) && mode !== "edit") {
          setSelectedId(items[0]?.id || "");
          setSelectedInvoice(null);
        }
      })
      .catch(() => {
        if (!cancelled) setDocuments([]);
      })
      .finally(() => {
        if (!cancelled) setDocumentsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [connId, dateFrom, dateTo, deferredSearch, documentTab, mode, selectedId, statutFilter]);

  useEffect(() => {
    if (!connId || !selectedId || !documentTab || mode === "edit") return;
    let cancelled = false;
    setDetailLoading(true);
    getInvoice(connId, selectedId)
      .then((invoice) => {
        if (!cancelled) setSelectedInvoice(invoice);
      })
      .finally(() => {
        if (!cancelled) setDetailLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [connId, documentTab, mode, selectedId]);

  useEffect(() => {
    if (!connId || !selectedTier?.id) return;
    let cancelled = false;
    setTierDocsLoading(true);
    listInvoices(connId, { tiersId: selectedTier.id })
      .then((items) => {
        if (!cancelled) setSelectedTierInvoices(items);
      })
      .finally(() => {
        if (!cancelled) setTierDocsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [connId, selectedTier]);

  const filteredTiers = useMemo(() => {
    const base = directoryTab?.type === "clients"
      ? tiers.filter((tier) => tier.type_tiers !== "fournisseur")
      : directoryTab?.type === "fournisseurs"
        ? tiers.filter((tier) => tier.type_tiers !== "client")
        : tiers;

    if (!deferredTierSearch.trim()) return base;
    return base.filter((tier) => matchesSearch(`${tier.code} ${tier.nom} ${tier.ville}`, deferredTierSearch));
  }, [deferredTierSearch, directoryTab?.type, tiers]);

  const filteredArticles = useMemo(() => {
    if (!deferredArticleSearch.trim()) return articles;
    return articles.filter((article) =>
      matchesSearch(`${article.code} ${article.libelle} ${article.reference}`, deferredArticleSearch)
    );
  }, [articles, deferredArticleSearch]);

  const selectedTemplate = useMemo(
    () => templates.find((template) => template.id === templateId) ?? null,
    [templateId, templates],
  );

  const workingInvoice = mode === "edit" ? editorInvoice : selectedInvoice;
  const currentStatus = workingInvoice?.statut || "Brouillon";
  const nextStatuses = STATUS_TRANSITIONS[currentStatus] || [];

  const openPreview = async (invoice, nextTemplateId = templateId) => {
    if (!invoice) return;
    setPreviewLoading(true);
    setMode("preview");
    try {
      const html = await renderInvoiceHtml(invoice, nextTemplateId);
      setPreviewHtml(html);
    } finally {
      setPreviewLoading(false);
    }
  };

  const refreshSelectedInvoice = async (pieceId) => {
    if (!pieceId) return;
    const invoice = await getInvoice(connId, pieceId);
    setSelectedInvoice(invoice);
    return invoice;
  };

  const saveCurrentInvoice = async (openPreviewAfter = false) => {
    if (!editorInvoice) return;
    setSaving(true);
    try {
      const payload = computeInvoice(editorInvoice);
      const saved = payload.id ? await updateInvoice(connId, payload) : await createInvoice(connId, payload);
      setSelectedId(saved.id);
      setSelectedInvoice(saved);
      setEditorInvoice(saved);
      setMode(openPreviewAfter ? "preview" : "view");
      await listInvoices(connId, {
        nature: documentTab?.nature,
        statut: statutFilter || null,
        dateFrom: dateFrom || null,
        dateTo: dateTo || null,
        search: deferredSearch || null,
      }).then(setDocuments);
      if (openPreviewAfter) {
        await openPreview(saved);
      }
    } finally {
      setSaving(false);
    }
  };

  const handleStatusChange = async (newStatus) => {
    if (!selectedInvoice?.id) return;
    await updateStatut(connId, selectedInvoice.id, newStatus, "SDB");
    await refreshSelectedInvoice(selectedInvoice.id);
    const items = await listInvoices(connId, {
      nature: documentTab?.nature,
      statut: statutFilter || null,
      dateFrom: dateFrom || null,
      dateTo: dateTo || null,
      search: deferredSearch || null,
    });
    setDocuments(items);
  };

  const handleDelete = async () => {
    if (!selectedInvoice?.id) return;
    if (!window.confirm(t("invoice_delete_confirm", selectedInvoice.numero || selectedInvoice.id))) return;
    await deleteInvoice(connId, selectedInvoice.id);
    const items = await listInvoices(connId, { nature: documentTab?.nature });
    setDocuments(items);
    setSelectedId(items[0]?.id || "");
    setSelectedInvoice(null);
  };

  const handleComptabiliser = async () => {
    if (!selectedInvoice?.id) return;
    await comptabiliserInvoice(connId, selectedInvoice.id);
    await refreshSelectedInvoice(selectedInvoice.id);
    const items = await listInvoices(connId, { nature: documentTab?.nature });
    setDocuments(items);
  };

  const handleExportPdf = async () => {
    const invoice = workingInvoice;
    if (!invoice) return;
    const filePath = await save({
      defaultPath: `${invoice.numero || "invoice"}.pdf`,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (!filePath) return;
    await exportInvoicePdf(invoice, templateId, filePath);
  };

  const updateEditor = (patch) => {
    setEditorInvoice((current) => computeInvoice({ ...current, ...patch }));
  };

  const updateLine = (lineIndex, patch) => {
    setEditorInvoice((current) => computeInvoice({
      ...current,
      lignes: current.lignes.map((line, index) => index === lineIndex ? { ...line, ...patch } : line),
    }));
  };

  const addLine = () => {
    setEditorInvoice((current) => computeInvoice({
      ...current,
      lignes: [...current.lignes, buildBlankLine(current.lignes.length + 1)],
    }));
  };

  const removeLine = (lineIndex) => {
    setEditorInvoice((current) => computeInvoice({
      ...current,
      lignes: current.lignes.filter((_, index) => index !== lineIndex),
    }));
  };

  const moveLine = (fromIndex, toIndex) => {
    setEditorInvoice((current) => {
      const nextLines = [...current.lignes];
      const [moved] = nextLines.splice(fromIndex, 1);
      nextLines.splice(toIndex, 0, moved);
      return computeInvoice({ ...current, lignes: nextLines });
    });
  };

  const renderDocumentView = () => {
    if (!selectedInvoice && !detailLoading) {
      return (
        <div className="empty-state">
          <Files size={30} />
          <p>{t("invoice_empty_selection")}</p>
        </div>
      );
    }

    if (detailLoading) {
      return (
        <div className="empty-state">
          <div className="spinner" />
          <p>{t("invoice_loading_detail")}</p>
        </div>
      );
    }

    if (mode === "edit" && editorInvoice) {
      const breakdown = getTvaBreakdown(editorInvoice);

      return (
        <div className="invoice-editor">
          <div className="invoice-action-bar">
            <button type="button" className="btn" onClick={() => {
              setMode("view");
              setEditorInvoice(selectedInvoice);
            }}>
              {t("cancel")}
            </button>
            <button type="button" className="btn" onClick={() => saveCurrentInvoice(false)} disabled={saving}>
              <SaveIcon />
              {t("invoice_save_draft")}
            </button>
            <button type="button" className="btn btn-accent" onClick={() => saveCurrentInvoice(true)} disabled={saving}>
              <Eye size={14} />
              {t("invoice_save_and_preview")}
            </button>
          </div>

          <section className="invoice-editor-card">
            <div className="invoice-editor-grid">
              <label>
                <span>{t("invoice_nature")}</span>
                <select value={editorInvoice.nature} onChange={(event) => updateEditor({ nature: event.target.value })}>
                  <option value="Facture">Facture</option>
                  <option value="Avoir">Avoir</option>
                  <option value="Proforma">Proforma</option>
                  <option value="Commande">Commande</option>
                  <option value="BL">BL</option>
                </select>
              </label>
              <label>
                <span>{t("invoice_number")}</span>
                <input value={editorInvoice.numero} onChange={(event) => updateEditor({ numero: event.target.value })} />
              </label>
              <label>
                <span>{t("invoice_date")}</span>
                <input type="date" value={editorInvoice.date} onChange={(event) => updateEditor({ date: event.target.value })} />
              </label>
              <label>
                <span>{t("invoice_due_date")}</span>
                <input type="date" value={editorInvoice.date_echeance} onChange={(event) => updateEditor({ date_echeance: event.target.value })} />
              </label>
              <label className="span-2">
                <span>{t("invoice_reference")}</span>
                <input value={editorInvoice.reference} onChange={(event) => updateEditor({ reference: event.target.value })} />
              </label>
              <div className="span-2">
                <span className="invoice-editor-label">{t("invoice_customer")}</span>
                <SearchSelect
                  placeholder={t("invoice_tiers_placeholder")}
                  value={editorInvoice.tiers_id}
                  displayValue={[editorInvoice.tiers_code, editorInvoice.tiers_nom, editorInvoice.tiers_ville].filter(Boolean).join(" · ")}
                  options={tiers}
                  onSelect={(tier) => {
                    updateEditor({
                      tiers_id: tier.id,
                      tiers_code: tier.code,
                      tiers_nom: tier.nom,
                      tiers_adresse: tier.adresse,
                      tiers_cp: tier.cp,
                      tiers_ville: tier.ville,
                      tiers_pays: tier.pays,
                      tiers_siret: tier.siret,
                      tiers_tva_intra: tier.tva_intra || "",
                      is_supplier: tier.type_tiers === "fournisseur",
                    });
                  }}
                  renderOption={(tier) => (
                    <div className="invoice-search-option-copy">
                      <strong>{tier.code}</strong>
                      <span>{tier.nom} · {tier.ville}</span>
                    </div>
                  )}
                  getSearchText={(tier) => `${tier.code} ${tier.nom} ${tier.ville} ${tier.siret}`}
                />
              </div>
              <label className="span-2">
                <span>{t("invoice_address")}</span>
                <input value={editorInvoice.tiers_adresse} onChange={(event) => updateEditor({ tiers_adresse: event.target.value })} />
              </label>
              <label>
                <span>{t("invoice_postal_code")}</span>
                <input value={editorInvoice.tiers_cp} onChange={(event) => updateEditor({ tiers_cp: event.target.value })} />
              </label>
              <label>
                <span>{t("invoice_city")}</span>
                <input value={editorInvoice.tiers_ville} onChange={(event) => updateEditor({ tiers_ville: event.target.value })} />
              </label>
              <label>
                <span>{t("invoice_country")}</span>
                <input value={editorInvoice.tiers_pays} onChange={(event) => updateEditor({ tiers_pays: event.target.value })} />
              </label>
              <label>
                <span>SIRET</span>
                <input value={editorInvoice.tiers_siret} onChange={(event) => updateEditor({ tiers_siret: event.target.value })} />
              </label>
            </div>
          </section>

          <section className="invoice-editor-card">
            <div className="invoice-editor-section-head">
              <strong>{t("invoice_lines")}</strong>
              <button type="button" className="btn" onClick={addLine}>
                <FilePlus2 size={14} />
                {t("invoice_add_line")}
              </button>
            </div>

            <div className="invoice-lines-table-wrap">
              <table className="invoice-lines-table">
                <thead>
                  <tr>
                    <th>#</th>
                    <th>{t("invoice_article")}</th>
                    <th>{t("invoice_label")}</th>
                    <th>{t("invoice_qty")}</th>
                    <th>{t("invoice_unit")}</th>
                    <th>{t("invoice_unit_price")}</th>
                    <th>{t("invoice_discount_pct")}</th>
                    <th>HT</th>
                    <th>{t("invoice_vat_rate")}</th>
                    <th>TTC</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {editorInvoice.lignes.map((line, index) => (
                    <tr
                      key={line.id}
                      draggable
                      onDragStart={(event) => event.dataTransfer.setData("text/plain", String(index))}
                      onDragOver={(event) => event.preventDefault()}
                      onDrop={(event) => {
                        const fromIndex = Number(event.dataTransfer.getData("text/plain"));
                        if (!Number.isNaN(fromIndex)) moveLine(fromIndex, index);
                      }}
                    >
                      <td>
                        <div className="invoice-drag-handle">⋮⋮</div>
                      </td>
                      <td>
                        <SearchSelect
                          placeholder={t("invoice_article_placeholder")}
                          value={line.article_id}
                          displayValue={[line.article_code, line.libelle].filter(Boolean).join(" · ")}
                          options={articles}
                          onSelect={(article) => {
                            updateLine(index, {
                              article_id: article.id,
                              article_code: article.code,
                              libelle: article.libelle,
                              prix_ht: article.prix_ht,
                              taux_tva: article.taux_tva,
                              unite: article.unite,
                            });
                          }}
                          renderOption={(article) => (
                            <div className="invoice-search-option-copy">
                              <strong>{article.code}</strong>
                              <span>{article.libelle}</span>
                            </div>
                          )}
                          getSearchText={(article) => `${article.code} ${article.libelle} ${article.reference}`}
                        />
                      </td>
                      <td><input value={line.libelle} onChange={(event) => updateLine(index, { libelle: event.target.value })} /></td>
                      <td><input type="number" step="0.01" value={line.quantite} onChange={(event) => updateLine(index, { quantite: event.target.value })} /></td>
                      <td><input value={line.unite} onChange={(event) => updateLine(index, { unite: event.target.value })} /></td>
                      <td><input type="number" step="0.01" value={line.prix_ht} onChange={(event) => updateLine(index, { prix_ht: event.target.value })} /></td>
                      <td><input type="number" step="0.01" value={line.remise_pct} onChange={(event) => updateLine(index, { remise_pct: event.target.value })} /></td>
                      <td className="num">{formatCurrency(line.montant_ht, editorInvoice.devise)}</td>
                      <td><input type="number" step="0.01" value={line.taux_tva} onChange={(event) => updateLine(index, { taux_tva: event.target.value })} /></td>
                      <td className="num">{formatCurrency(line.montant_ttc, editorInvoice.devise)}</td>
                      <td>
                        <button type="button" className="btn btn-icon" onClick={() => removeLine(index)}>
                          <Trash2 size={14} />
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>

            <div className="invoice-totals-layout">
              <div className="invoice-editor-card subtle">
                <label>
                  <span>{t("invoice_notes")}</span>
                  <textarea rows="5" value={editorInvoice.notes} onChange={(event) => updateEditor({ notes: event.target.value })} />
                </label>
                <label>
                  <span>{t("invoice_payment_conditions")}</span>
                  <textarea rows="4" value={editorInvoice.conditions} onChange={(event) => updateEditor({ conditions: event.target.value })} />
                </label>
              </div>
              <div className="invoice-editor-card totals">
                <DetailRow label={t("invoice_total_ht")} value={formatCurrency(editorInvoice.total_ht, editorInvoice.devise)} />
                <label className="invoice-inline-label">
                  <span>{t("invoice_global_discount")}</span>
                  <input type="number" step="0.01" value={editorInvoice.remise_globale} onChange={(event) => updateEditor({ remise_globale: event.target.value })} />
                </label>
                <div className="invoice-tva-breakdown">
                  {breakdown.map((row) => (
                    <div key={row.rate} className="invoice-detail-row">
                      <span>{t("invoice_vat_row", row.rate)}</span>
                      <strong>{formatCurrency(row.tax, editorInvoice.devise)}</strong>
                    </div>
                  ))}
                </div>
                <DetailRow label={t("invoice_total_vat")} value={formatCurrency(editorInvoice.total_tva, editorInvoice.devise)} />
                <div className="invoice-grand-total">
                  <span>{t("invoice_total_ttc")}</span>
                  <strong>{formatCurrency(editorInvoice.total_ttc, editorInvoice.devise)}</strong>
                </div>
              </div>
            </div>
          </section>
        </div>
      );
    }

    if (!selectedInvoice) return null;

    return (
      <div className="invoice-view">
        <div className="invoice-action-bar">
          <button
            type="button"
            className="btn"
            disabled={displayStatus(selectedInvoice.statut) === "Comptabilisé"}
            onClick={() => {
              setEditorInvoice(computeInvoice(selectedInvoice));
              setMode("edit");
            }}
          >
            <PencilLine size={14} />
            {t("invoice_edit")}
          </button>

          <select
            className="invoice-inline-select"
            value=""
            disabled={!nextStatuses.length}
            onChange={(event) => {
              if (event.target.value) handleStatusChange(event.target.value);
              event.target.value = "";
            }}
          >
            <option value="">{t("invoice_change_status")}</option>
            {nextStatuses.map((status) => (
              <option key={status} value={status}>
                {displayStatus(status)}
              </option>
            ))}
          </select>

          <button type="button" className="btn" onClick={() => openPreview(selectedInvoice)}>
            <Eye size={14} />
            {t("invoice_preview")}
          </button>
          <button type="button" className="btn" onClick={handleExportPdf}>
            <BadgeEuro size={14} />
            {t("invoice_export_pdf")}
          </button>
          <button
            type="button"
            className="btn"
            disabled={selectedInvoice.nature !== "Facture" || displayStatus(selectedInvoice.statut) === "Comptabilisé"}
            onClick={handleComptabiliser}
          >
            <BadgeEuro size={14} />
            {t("invoice_post")}
          </button>
          <button
            type="button"
            className="btn"
            onClick={() => {
              const copy = duplicateInvoice(selectedInvoice);
              setSelectedInvoice(copy);
              setEditorInvoice(copy);
              setMode("edit");
            }}
          >
            <Files size={14} />
            {t("invoice_duplicate")}
          </button>
          <button
            type="button"
            className="btn btn-danger"
            disabled={selectedInvoice.statut !== "Brouillon"}
            onClick={handleDelete}
          >
            <Trash2 size={14} />
            {t("invoice_delete")}
          </button>
        </div>

        <div className="invoice-view-grid">
          <section className="invoice-detail-card">
            <div className="invoice-view-head">
              <div>
                <h2>{selectedInvoice.numero || "Document"}</h2>
                <p>{selectedInvoice.tiers_nom}</p>
              </div>
              <StatusBadge statut={selectedInvoice.statut} />
            </div>
            <DetailRow label={t("invoice_nature")} value={selectedInvoice.nature} />
            <DetailRow label={t("invoice_date")} value={formatDate(selectedInvoice.date)} />
            <DetailRow label={t("invoice_due_date")} value={formatDate(selectedInvoice.date_echeance)} />
            <DetailRow label={t("invoice_reference")} value={selectedInvoice.reference} />
            <DetailRow label={t("invoice_currency")} value={selectedInvoice.devise} />
          </section>
          <section className="invoice-detail-card">
            <div className="invoice-view-head">
              <div>
                <h2>{t("invoice_customer")}</h2>
                <p>{selectedInvoice.tiers_code}</p>
              </div>
            </div>
            <DetailRow label={t("invoice_address")} value={selectedInvoice.tiers_adresse} />
            <DetailRow label={t("invoice_city")} value={[selectedInvoice.tiers_cp, selectedInvoice.tiers_ville].filter(Boolean).join(" ")} />
            <DetailRow label={t("invoice_country")} value={selectedInvoice.tiers_pays} />
            <DetailRow label="SIRET" value={selectedInvoice.tiers_siret} />
          </section>
        </div>

        <section className="invoice-editor-card">
          <div className="invoice-editor-section-head">
            <strong>{t("invoice_lines")}</strong>
            <span>{selectedInvoice.lignes.length} ligne(s)</span>
          </div>
          <div className="invoice-lines-table-wrap">
            <table className="invoice-lines-table">
              <thead>
                <tr>
                  <th>#</th>
                  <th>{t("invoice_article")}</th>
                  <th>{t("invoice_label")}</th>
                  <th>{t("invoice_qty")}</th>
                  <th>{t("invoice_unit")}</th>
                  <th>{t("invoice_unit_price")}</th>
                  <th>{t("invoice_discount_pct")}</th>
                  <th>HT</th>
                  <th>{t("invoice_vat_rate")}</th>
                  <th>TTC</th>
                </tr>
              </thead>
              <tbody>
                {selectedInvoice.lignes.map((line) => (
                  <tr key={line.id}>
                    <td>{line.ordre}</td>
                    <td>{line.article_code}</td>
                    <td>{line.libelle}</td>
                    <td className="num">{line.quantite}</td>
                    <td>{line.unite}</td>
                    <td className="num">{formatCurrency(line.prix_ht, selectedInvoice.devise)}</td>
                    <td className="num">{round2(line.remise_pct)}%</td>
                    <td className="num">{formatCurrency(line.montant_ht, selectedInvoice.devise)}</td>
                    <td className="num">{round2(line.taux_tva)}%</td>
                    <td className="num">{formatCurrency(line.montant_ttc, selectedInvoice.devise)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>

        <div className="invoice-totals-layout">
          <section className="invoice-editor-card subtle">
            <strong>{t("invoice_notes")}</strong>
            <p>{selectedInvoice.notes || "—"}</p>
            <strong>{t("invoice_payment_conditions")}</strong>
            <p>{selectedInvoice.conditions || "—"}</p>
          </section>
          <section className="invoice-editor-card totals">
            <DetailRow label={t("invoice_total_ht")} value={formatCurrency(selectedInvoice.total_ht, selectedInvoice.devise)} />
            <DetailRow label={t("invoice_total_vat")} value={formatCurrency(selectedInvoice.total_tva, selectedInvoice.devise)} />
            <div className="invoice-grand-total">
              <span>{t("invoice_total_ttc")}</span>
              <strong>{formatCurrency(selectedInvoice.total_ttc, selectedInvoice.devise)}</strong>
            </div>
          </section>
        </div>

        <section className="invoice-editor-card">
          <div className="invoice-editor-section-head">
            <strong>{t("invoice_status_history")}</strong>
          </div>
          <div className="invoice-history-list">
            {(selectedInvoice.statut_history || []).length ? selectedInvoice.statut_history.map((entry) => (
              <div key={entry.id} className="invoice-history-item">
                <span>{entry.updated_at}</span>
                <strong>{displayStatus(entry.from_statut || "Brouillon")} → {displayStatus(entry.to_statut)}</strong>
                <span>{entry.updated_by || "SDB"}</span>
              </div>
            )) : <div className="sidebar-empty-copy">{t("invoice_no_history")}</div>}
          </div>
        </section>
      </div>
    );
  };

  return (
    <div className="invoicing-shell">
      <header className="invoicing-header">
        <div className="invoicing-header-start">
          <button type="button" className="btn" onClick={onBack}>
            <ArrowLeft size={14} />
            {t("invoice_back")}
          </button>
          <div>
            <div className="settings-drawer-eyebrow">{t("sidebar_invoicing")}</div>
            <h2>{t("invoice_title")}</h2>
          </div>
        </div>
        <div className="invoicing-header-actions">
          <button type="button" className="btn" onClick={onOpenTemplateDesigner}>
            <Printer size={14} />
            {t("invoice_template_appearance")}
          </button>
          <div className="invoice-schema-chip">{schema?.edition || "auto"}</div>
        </div>
      </header>

      <div className="invoice-tabbar">
        {[...DOCUMENT_TABS, ...DIRECTORY_TABS].map((tab) => (
          <TabButton
            key={tab.id}
            active={activeTab === tab.id}
            icon={tab.icon}
            label={t(tab.labelKey)}
            onClick={() => {
              startTransition(() => {
                setActiveTab(tab.id);
                setMode("view");
                setSelectedInvoice(null);
                setSelectedTier(null);
                setSelectedArticle(null);
              });
            }}
          />
        ))}
      </div>

      {documentTab ? (
        <div className="invoicing-layout">
          <aside className="invoice-list-panel">
            <div className="invoice-list-toolbar">
              <div className="invoice-searchbox">
                <Search size={14} />
                <input
                  value={search}
                  onChange={(event) => setSearch(event.target.value)}
                  placeholder={t("invoice_search_placeholder")}
                />
              </div>
              <div className="invoice-filter-grid">
                <select value={statutFilter} onChange={(event) => setStatutFilter(event.target.value)}>
                  <option value="">{t("invoice_filter_status_all")}</option>
                  <option value="Brouillon">Brouillon</option>
                  <option value="Proforma">Proforma</option>
                  <option value="Bon_Commande">Bon de commande</option>
                  <option value="Facture">Facture</option>
                  <option value="Avoir">Avoir</option>
                  <option value="Comptabilise">Comptabilisé</option>
                </select>
                <input type="date" value={dateFrom} onChange={(event) => setDateFrom(event.target.value)} />
                <input type="date" value={dateTo} onChange={(event) => setDateTo(event.target.value)} />
              </div>
            </div>

            <div className="invoice-list-scroll">
              {documentsLoading ? (
                <div className="empty-state">
                  <div className="spinner" />
                  <p>{t("invoice_loading_list")}</p>
                </div>
              ) : documents.length ? documents.map((invoice) => (
                <InvoiceCard
                  key={invoice.id}
                  invoice={invoice}
                  active={invoice.id === selectedId && mode !== "edit"}
                  onClick={() => {
                    setSelectedId(invoice.id);
                    setMode("view");
                  }}
                />
              )) : (
                <div className="empty-state">
                  <FileText size={28} />
                  <p>{t("invoice_empty_list")}</p>
                </div>
              )}
            </div>

            <button
              type="button"
              className="btn btn-accent invoice-new-btn"
              onClick={() => {
                const fresh = buildBlankInvoice(documentTab.nature, selectedTemplate);
                setSelectedId("");
                setSelectedInvoice(fresh);
                setEditorInvoice(fresh);
                setMode("edit");
              }}
            >
              <FilePlus2 size={14} />
              {t("invoice_new_document", t(documentTab.labelKey))}
            </button>
          </aside>

          <main className="invoice-detail-panel">
            {renderDocumentView()}
          </main>
        </div>
      ) : activeTab === "articles" ? (
        <div className="invoice-directory-layout">
          <section className="invoice-directory-list">
            <div className="invoice-searchbox directory">
              <Search size={14} />
              <input value={articleSearch} onChange={(event) => setArticleSearch(event.target.value)} placeholder={t("invoice_articles_search")} />
            </div>
            <button type="button" className="btn invoice-directory-create" onClick={() => setArticleModal({ initial: BLANK_ARTICLE })}>
              <FilePlus2 size={14} />
              {t("invoice_new_article")}
            </button>
            <div className="invoice-directory-items">
              {articlesLoading ? <div className="spinner" /> : filteredArticles.map((article) => (
                <div key={article.id} className={`invoice-directory-item-wrap ${selectedArticle?.id === article.id ? "active" : ""}`}>
                  <button
                    type="button"
                    className="invoice-directory-item-main"
                    onClick={() => setSelectedArticle(article)}
                  >
                    <strong>{article.code}</strong>
                    <span>{article.libelle}</span>
                    <em>{formatCurrency(article.prix_ht)}</em>
                  </button>
                  {article.id?.startsWith("sdb-") && (
                    <div className="invoice-directory-item-actions">
                      <button type="button" className="btn btn-icon" title={t("edit")} onClick={() => setArticleModal({ initial: article })}>
                        <PencilLine size={12} />
                      </button>
                      <button type="button" className="btn btn-icon btn-danger" title={t("delete")} onClick={async () => {
                        if (!window.confirm(t("invoice_confirm_delete"))) return;
                        await deleteArticle(connId, article.id);
                        refreshArticles();
                        if (selectedArticle?.id === article.id) setSelectedArticle(null);
                      }}>
                        <Trash2 size={12} />
                      </button>
                    </div>
                  )}
                </div>
              ))}
            </div>
          </section>
          <ArticlesPanel article={selectedArticle} />
        </div>
      ) : (
        <div className="invoice-directory-layout">
          <section className="invoice-directory-list">
            <div className="invoice-searchbox directory">
              <Search size={14} />
              <input value={tierSearch} onChange={(event) => setTierSearch(event.target.value)} placeholder={t("invoice_tiers_search")} />
            </div>
            <button
              type="button"
              className="btn invoice-directory-create"
              onClick={() => setTiersModal({ initial: { ...BLANK_TIERS, type_tiers: activeTab === "fournisseurs" ? "fournisseur" : "client" } })}
            >
              <FilePlus2 size={14} />
              {activeTab === "clients" ? t("invoice_new_client") : t("invoice_new_supplier")}
            </button>
            <div className="invoice-directory-items">
              {tiersLoading ? <div className="spinner" /> : filteredTiers.map((tier) => (
                <div key={tier.id} className={`invoice-directory-item-wrap ${selectedTier?.id === tier.id ? "active" : ""}`}>
                  <button
                    type="button"
                    className="invoice-directory-item-main"
                    onClick={() => setSelectedTier(tier)}
                  >
                    <strong>{tier.code}</strong>
                    <span>{tier.nom}</span>
                    <em>{tier.ville}</em>
                  </button>
                  {tier.is_local && (
                    <div className="invoice-directory-item-actions">
                      <button type="button" className="btn btn-icon" title={t("edit")} onClick={() => setTiersModal({ initial: tier })}>
                        <PencilLine size={12} />
                      </button>
                      <button type="button" className="btn btn-icon btn-danger" title={t("delete")} onClick={async () => {
                        if (!window.confirm(t("invoice_confirm_delete"))) return;
                        await deleteTiers(connId, tier.id);
                        refreshTiers();
                        if (selectedTier?.id === tier.id) setSelectedTier(null);
                      }}>
                        <Trash2 size={12} />
                      </button>
                    </div>
                  )}
                </div>
              ))}
            </div>
          </section>
          <TiersPanel tier={selectedTier} invoices={selectedTierInvoices} loading={tierDocsLoading} />
        </div>
      )}

      {mode === "preview" ? (
        <PreviewModal
          html={previewHtml}
          loading={previewLoading}
          templates={templates}
          templateId={templateId}
          onTemplateChange={async (nextTemplateId) => {
            setTemplateId(nextTemplateId);
            await openPreview(workingInvoice, nextTemplateId);
          }}
          onClose={() => setMode(selectedInvoice ? "view" : "edit")}
          onExportPdf={handleExportPdf}
        />
      ) : null}

      {tiersModal && (
        <TiersFormModal
          t={t}
          initial={tiersModal.initial}
          onSave={async (form) => {
            if (form.id && form.id.startsWith("sdb-")) {
              await updateTiers(connId, form);
            } else {
              await createTiers(connId, form);
            }
            refreshTiers();
          }}
          onClose={() => setTiersModal(null)}
        />
      )}

      {articleModal && (
        <ArticleFormModal
          t={t}
          initial={articleModal.initial}
          onSave={async (form) => {
            if (form.id && form.id.startsWith("sdb-")) {
              await updateArticle(connId, form);
            } else {
              await createArticle(connId, form);
            }
            refreshArticles();
          }}
          onClose={() => setArticleModal(null)}
        />
      )}
    </div>
  );
}

function SaveIcon() {
  return <PencilLine size={14} />;
}
