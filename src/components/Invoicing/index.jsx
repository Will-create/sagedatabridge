import {
  useCallback,
  startTransition,
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
  getSettings,
  listInvoices,
  searchArticles,
  searchTiers,
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

const EMPTY_TEMPLATE_ID = "builtin-modern-clean";
const LOCAL_DRAFT_PREFIX = "sdb_invoice_drafts:";
const SYNC_QUEUE_PREFIX = "sdb_sync_queue:";
const LOCAL_ITEMS_PREFIX = "sdb_invoice_items:";
const DEFAULT_INVOICE_SETTINGS = {
  invoice_units: ["Pce", "Kg", "L", "H", "Jour"],
  invoice_vat_rates: [18, 20, 10, 5.5, 0],
  invoice_default_unit: "Pce",
  invoice_default_vat_rate: 20,
  invoice_default_currency: "XOF",
  invoice_default_payment_terms: "",
  invoice_extra_taxes: [],
  tax_types: [
    { id: "vat", name: "VAT", rate: 20, account: "445710", active: true },
    { id: "bic", name: "BIC", rate: 0, account: "", active: false },
  ],
};

function round2(value) {
  return Math.round((Number(value) || 0) * 100) / 100;
}

function todayIso() {
  return new Date().toISOString().slice(0, 10);
}

function formatCurrency(value, devise = "XOF") {
  const isXof = (devise || "XOF").toUpperCase() === "XOF";
  const locale = (localStorage.getItem("sdb_lang") || "en") === "fr" ? "fr-FR" : "en-US";
  return new Intl.NumberFormat(locale, {
    style: "currency",
    currency: devise || "XOF",
    minimumFractionDigits: isXof ? 0 : 2,
    maximumFractionDigits: isXof ? 0 : 2,
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

function normalizeDiscountType(value) {
  return value === "fixed" ? "fixed" : "percent";
}

function applyDiscount(base, type, value) {
  const amount = Math.max(sanitizeNumber(value), 0);
  if (normalizeDiscountType(type) === "fixed") {
    return Math.max(base - Math.min(amount, base), 0);
  }
  return base * (1 - Math.min(amount, 100) / 100);
}

function normalizeInvoiceSettings(settings = {}) {
  const merged = { ...DEFAULT_INVOICE_SETTINGS, ...(settings || {}) };
  const units = Array.isArray(merged.invoice_units)
    ? merged.invoice_units.map((unit) => String(unit).trim()).filter(Boolean)
    : DEFAULT_INVOICE_SETTINGS.invoice_units;
  const rates = Array.isArray(merged.invoice_vat_rates)
    ? merged.invoice_vat_rates.map((rate) => Number(rate)).filter((rate) => Number.isFinite(rate))
    : DEFAULT_INVOICE_SETTINGS.invoice_vat_rates;
  return {
    ...merged,
    invoice_units: units.length ? units : DEFAULT_INVOICE_SETTINGS.invoice_units,
    invoice_vat_rates: rates.length ? rates : DEFAULT_INVOICE_SETTINGS.invoice_vat_rates,
    invoice_default_unit: String(merged.invoice_default_unit || units[0] || "Pce"),
    invoice_default_vat_rate: Number.isFinite(Number(merged.invoice_default_vat_rate))
      ? Number(merged.invoice_default_vat_rate)
      : 20,
    invoice_default_currency: String(merged.invoice_default_currency || "XOF").toUpperCase(),
    tax_types: Array.isArray(merged.tax_types) && merged.tax_types.length
      ? merged.tax_types
      : DEFAULT_INVOICE_SETTINGS.tax_types,
  };
}

function localDraftKey(connId) {
  return `${LOCAL_DRAFT_PREFIX}${connId || "default"}`;
}

function readLocalDrafts(connId) {
  try {
    const raw = localStorage.getItem(localDraftKey(connId));
    const parsed = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function writeLocalDrafts(connId, drafts) {
  localStorage.setItem(localDraftKey(connId), JSON.stringify(drafts));
}

function syncQueueKey(connId) {
  return `${SYNC_QUEUE_PREFIX}${connId || "default"}`;
}

function readSyncQueue(connId) {
  try {
    const raw = localStorage.getItem(syncQueueKey(connId));
    const parsed = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function writeSyncQueue(connId, queue) {
  localStorage.setItem(syncQueueKey(connId), JSON.stringify(queue));
}

function localItemsKey(connId) {
  return `${LOCAL_ITEMS_PREFIX}${connId || "default"}`;
}

function readLocalItems(connId) {
  try {
    const raw = localStorage.getItem(localItemsKey(connId));
    const parsed = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function writeLocalItems(connId, items) {
  localStorage.setItem(localItemsKey(connId), JSON.stringify(items));
}

function makeSyncQueueItem(type, payload, error = "") {
  return {
    id: `sync-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    type,
    payload,
    error: String(error || ""),
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
  };
}

function makeLocalDraft(invoice, error = "") {
  return computeInvoice({
    ...invoice,
    id: invoice.id && String(invoice.id).startsWith("local-")
      ? invoice.id
      : `local-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    numero: invoice.numero || "Brouillon local",
    is_local_draft: true,
    sync_error: String(error || ""),
    updated_at: new Date().toISOString(),
  });
}

function buildBlankLine(ordre = 1, settings = DEFAULT_INVOICE_SETTINGS) {
  const normalized = normalizeInvoiceSettings(settings);
  const bicTax = normalized.tax_types.find((tax) => String(tax.id || tax.name).toLowerCase() === "bic" && tax.active);
  return {
    id: `line-${Date.now()}-${ordre}`,
    ordre,
    article_id: "",
    article_code: "",
    libelle: "",
    quantite: 1,
    unite: normalized.invoice_default_unit,
    prix_ht: 0,
    remise_pct: 0,
    remise_type: "percent",
    remise_valeur: 0,
    montant_ht: 0,
    taux_tva: normalized.invoice_default_vat_rate,
    montant_tva: 0,
    taux_bic: bicTax ? Number(bicTax.rate) || 0 : 0,
    montant_bic: 0,
    montant_ttc: 0,
    tax_exempt: false,
    revenue_account: "",
    expense_account: "",
    vat_account: "",
    bic_account: "",
    custom_tax_rules: "",
  };
}

function buildBlankInvoice(nature = "Facture", template = null, settings = DEFAULT_INVOICE_SETTINGS) {
  const normalized = normalizeInvoiceSettings(settings);
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
    devise: normalized.invoice_default_currency,
    total_ht: 0,
    total_tva: 0,
    total_bic: 0,
    total_ttc: 0,
    remise_globale: 0,
    remise_globale_type: "percent",
    remise_globale_valeur: 0,
    is_supplier: false,
    lignes: [buildBlankLine(1, normalized)],
    notes: "",
    conditions: template?.conditions_paiement || normalized.invoice_default_payment_terms || "",
    statut_history: [],
  });
}

function computeInvoice(invoice) {
  const globalDiscountType = normalizeDiscountType(invoice.remise_globale_type);
  const legacyGlobalDiscount = sanitizeNumber(invoice.remise_globale);
  const globalDiscountValue = sanitizeNumber(invoice.remise_globale_valeur || legacyGlobalDiscount);
  const lignes = (invoice.lignes || []).map((line, index) => {
    const quantite = sanitizeNumber(line.quantite);
    const prixHt = sanitizeNumber(line.prix_ht);
    const remiseType = normalizeDiscountType(line.remise_type);
    const remiseValeur = sanitizeNumber(line.remise_valeur || line.remise_pct);
    const remisePct = remiseType === "percent" ? Math.min(Math.max(remiseValeur, 0), 100) : Math.max(remiseValeur, 0);
    const tauxTva = line.tax_exempt ? 0 : sanitizeNumber(line.taux_tva);
    const tauxBic = line.tax_exempt ? 0 : sanitizeNumber(line.taux_bic);
    const montantHt = round2(applyDiscount(round2(quantite * prixHt), remiseType, remiseValeur));
    const montantTva = line.tax_exempt ? 0 : round2(montantHt * tauxTva / 100);
    const montantBic = line.tax_exempt ? 0 : round2(montantHt * tauxBic / 100);
    const montantTtc = round2(montantHt + montantTva + montantBic);

    return {
      ...line,
      ordre: index + 1,
      quantite,
      prix_ht: prixHt,
      remise_pct: remisePct,
      remise_type: remiseType,
      remise_valeur: remiseValeur,
      taux_tva: tauxTva,
      taux_bic: tauxBic,
      montant_ht: montantHt,
      montant_tva: montantTva,
      montant_bic: montantBic,
      montant_ttc: montantTtc,
    };
  });

  const subtotalHt = round2(lignes.reduce((sum, line) => sum + line.montant_ht, 0));
  const subtotalBeforeDiscount = round2(lignes.reduce((sum, line) => sum + round2(line.quantite * line.prix_ht), 0));
  const totalLineDiscount = round2(subtotalBeforeDiscount - subtotalHt);
  const subtotalTva = round2(lignes.reduce((sum, line) => sum + line.montant_tva, 0));
  const subtotalBic = round2(lignes.reduce((sum, line) => sum + line.montant_bic, 0));
  const totalHt = round2(applyDiscount(subtotalHt, globalDiscountType, globalDiscountValue));
  const totalGlobalDiscount = round2(subtotalHt - totalHt);
  const factor = subtotalHt > 0 ? totalHt / subtotalHt : 1;
  const totalTva = round2(subtotalTva * factor);
  const totalBic = round2(subtotalBic * factor);
  const totalTtc = round2(totalHt + totalTva + totalBic);

  return {
    ...invoice,
    devise: invoice.devise || "XOF",
    remise_globale: globalDiscountType === "percent" ? Math.min(Math.max(globalDiscountValue, 0), 100) : Math.max(globalDiscountValue, 0),
    remise_globale_type: globalDiscountType,
    remise_globale_valeur: globalDiscountValue,
    lignes,
    total_ht: totalHt,
    subtotal_ht_before_discount: subtotalBeforeDiscount,
    total_line_discount: totalLineDiscount,
    total_global_discount: totalGlobalDiscount,
    total_tva: totalTva,
    total_bic: totalBic,
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
    devise: invoice.devise || "XOF",
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

function getBicBreakdown(invoice) {
  const breakdown = new Map();
  for (const line of invoice.lignes || []) {
    const rate = round2(line.taux_bic);
    if (!rate) continue;
    const current = breakdown.get(rate) || { base: 0, tax: 0 };
    breakdown.set(rate, {
      base: round2(current.base + line.montant_ht),
      tax: round2(current.tax + line.montant_bic),
    });
  }
  return Array.from(breakdown.entries())
    .map(([rate, values]) => ({ rate, ...values }))
    .sort((left, right) => right.rate - left.rate);
}

function matchesSearch(haystack, needle) {
  return `${haystack || ""}`.toLowerCase().includes((needle || "").toLowerCase());
}

function itemKey(item) {
  return String(item.code || item.reference || item.id || "").trim().toLowerCase();
}

function normalizeItem(item = {}, invoiceSettings = DEFAULT_INVOICE_SETTINGS) {
  const currency = String(item.currency || item.devise || invoiceSettings.invoice_default_currency || "XOF").toUpperCase();
  const libelle = String(item.libelle || item.name || "").trim();
  const reference = String(item.reference || item.sku || "").trim();
  const code = String(item.code || reference || (libelle ? libelle.toUpperCase().replace(/[^A-Z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 24) : "")).trim();
  const status = item.status || (!code || !libelle ? "invalid" : item.is_local_draft || item.source === "local_draft" ? "unsynced" : "synced");

  return {
    id: item.id || `local-item-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    code,
    libelle,
    description: item.description || "",
    prix_ht: sanitizeNumber(item.prix_ht),
    currency,
    taux_tva: sanitizeNumber(item.taux_tva),
    taux_bic: sanitizeNumber(item.taux_bic),
    unite: item.unite || invoiceSettings.invoice_default_unit || "",
    reference,
    category: item.category || "",
    en_activite: item.en_activite !== false,
    revenue_account: item.revenue_account || item.accounting_account || "",
    expense_account: item.expense_account || "",
    vat_account: item.vat_account || item.tax_account || "",
    bic_account: item.bic_account || "",
    tax_exempt: Boolean(item.tax_exempt),
    custom_tax_rules: item.custom_tax_rules || "",
    source: item.source || (item.is_local_draft ? "local_draft" : "database"),
    status,
    sync_error: item.sync_error || "",
    updated_at: item.updated_at || new Date().toISOString(),
  };
}

function itemMatchesSearch(item, query) {
  const needle = String(query || "").trim().toLowerCase();
  if (!needle) return true;
  return [
    item.code,
    item.libelle,
    item.description,
    item.reference,
    item.category,
    item.prix_ht,
    item.currency,
  ].some((value) => String(value || "").toLowerCase().includes(needle));
}

function mergeCatalogItems(remoteItems = [], localItems = [], query = "", invoiceSettings = DEFAULT_INVOICE_SETTINGS) {
  const merged = [];
  const seen = new Set();
  for (const item of localItems.map((entry) => normalizeItem({ ...entry, source: "local_draft", is_local_draft: true }, invoiceSettings))) {
    if (!itemMatchesSearch(item, query)) continue;
    const key = itemKey(item) || item.id;
    seen.add(key);
    merged.push(item);
  }
  for (const item of remoteItems.map((entry) => normalizeItem({ ...entry, source: "database", status: "synced" }, invoiceSettings))) {
    const key = itemKey(item) || item.id;
    if (seen.has(key)) continue;
    if (!itemMatchesSearch(item, query)) continue;
    seen.add(key);
    merged.push(item);
  }
  return merged.sort((left, right) => String(left.libelle || left.code).localeCompare(String(right.libelle || right.code)));
}

function useDebouncedValue(value, delay = 300) {
  const [debounced, setDebounced] = useState(value);

  useEffect(() => {
    const timeoutId = globalThis.setTimeout(() => setDebounced(value), delay);
    return () => globalThis.clearTimeout(timeoutId);
  }, [delay, value]);

  return debounced;
}

function SearchSelect({
  placeholder,
  value,
  displayValue,
  onSelect,
  searchFn,
  emptyLabel,
  renderOption,
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState(displayValue || "");
  const [options, setOptions] = useState([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const debouncedQuery = useDebouncedValue(query, 300);

  useEffect(() => {
    if (!open) setQuery(displayValue || "");
  }, [displayValue, open]);

  useEffect(() => {
    if (!open) return undefined;
    const handle = () => setOpen(false);
    window.addEventListener("click", handle);
    return () => window.removeEventListener("click", handle);
  }, [open]);

  useEffect(() => {
    if (!open) return undefined;

    let cancelled = false;

    if (debouncedQuery.trim().length < 2) {
      setOptions([]);
      setLoading(false);
      return undefined;
    }

    setLoading(true);
    setError("");

    console.debug("[SearchSelect] Searching for:", debouncedQuery);

    searchFn(debouncedQuery)
      .then((items) => {
        if (cancelled) return;
        console.debug("[SearchSelect] Found items:", items?.length || 0);
        setOptions(items.slice(0, 12));
      })
      .catch((nextError) => {
        if (cancelled) return;
        console.error("[SearchSelect] Error:", nextError);
        setOptions([]);
        setError(String(nextError));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [debouncedQuery, open, searchFn]);

  return (
    <div className="invoice-search-select" onClick={(event) => event.stopPropagation()}>
      <input
        value={query}
        placeholder={placeholder}
        onFocus={() => {
          if (query === displayValue) setQuery("");
          setOpen(true);
        }}
        onChange={(event) => {
          setQuery(event.target.value);
          setOpen(true);
        }}
      />
      {open ? (
        <div className="invoice-search-select-menu">
          {loading ? <div className="invoice-search-select-empty">Loading…</div> : null}
          {!loading && error ? <div className="invoice-search-select-empty">{error}</div> : null}
          {!loading && !error && !options.length && debouncedQuery.trim().length >= 2 ? (
            <div className="invoice-search-select-empty">{emptyLabel}</div>
          ) : null}
          {!loading && !error ? options.map((option) => (
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
          )) : null}
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
        {invoice.is_local_draft ? <span className="invoice-status-badge warning">Local</span> : <StatusBadge statut={invoice.statut} />}
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

function TiersPanel({ tier, invoices, loading, error }) {
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
            {loading ? <div className="spinner" /> : null}
            {!loading && error ? <div className="invoice-form-error">{error}</div> : null}
            {!loading && !error ? invoices.map((invoice) => (
              <InvoiceCard key={invoice.id} invoice={invoice} active={false} onClick={() => {}} />
            )) : null}
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
            <DetailRow label={t("invoice_bic_rate")} value={`${round2(article.taux_bic)}%`} />
            <DetailRow label={t("invoice_unit")} value={article.unite} />
            <DetailRow label={t("invoice_reference")} value={article.reference} />
            <DetailRow label={t("invoice_revenue_account")} value={article.revenue_account} mono />
            <DetailRow label={t("invoice_expense_account")} value={article.expense_account} mono />
            <DetailRow label={t("invoice_tax_exempt")} value={article.tax_exempt ? t("yes") || "Yes" : t("no") || "No"} />
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
const BLANK_ARTICLE = {
  id: "",
  code: "",
  libelle: "",
  description: "",
  prix_ht: 0,
  currency: "XOF",
  taux_tva: 20,
  taux_bic: 0,
  unite: "",
  reference: "",
  category: "",
  en_activite: true,
  revenue_account: "",
  expense_account: "",
  vat_account: "",
  bic_account: "",
  tax_exempt: false,
  custom_tax_rules: "",
};

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

function ArticleFormModal({ initial, onSave, onClose, t, unitOptions = DEFAULT_INVOICE_SETTINGS.invoice_units }) {
  const [form, setForm] = useState({ ...BLANK_ARTICLE, ...initial });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const up = (patch) => setForm((f) => ({ ...f, ...patch }));

  async function handleSave() {
    const normalized = normalizeItem(form);
    if (!normalized.libelle.trim()) { setError(t("invoice_item_name_required")); return; }
    setSaving(true); setError("");
    try { await onSave(normalized); onClose(); }
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
                  <span>{t("invoice_sku")}</span>
                  <input value={form.code} onChange={(e) => up({ code: e.target.value })} />
                </label>
                <label>
                  <span>{t("invoice_reference")}</span>
                  <input value={form.reference} onChange={(e) => up({ reference: e.target.value })} />
                </label>
                <label className="span-2">
                  <span>{t("invoice_item_name")} *</span>
                  <input value={form.libelle} onChange={(e) => up({ libelle: e.target.value })} />
                </label>
                <label className="span-2">
                  <span>{t("invoice_item_description")}</span>
                  <textarea rows="2" value={form.description} onChange={(e) => up({ description: e.target.value })} />
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
                  <span>{t("invoice_currency")}</span>
                  <select value={form.currency || "XOF"} onChange={(e) => up({ currency: e.target.value })}>
                    <option value="XOF">XOF</option>
                    <option value="EUR">EUR</option>
                    <option value="USD">USD</option>
                  </select>
                </label>
                <label>
                  <span>{t("invoice_vat_rate")} %</span>
                  <input type="number" step="0.1" value={form.taux_tva} onChange={(e) => up({ taux_tva: parseFloat(e.target.value) || 0 })} />
                </label>
                <label>
                  <span>{t("invoice_bic_rate")} %</span>
                  <input type="number" step="0.1" value={form.taux_bic} onChange={(e) => up({ taux_bic: parseFloat(e.target.value) || 0 })} />
                </label>
                <label>
                  <span>{t("invoice_unit")}</span>
                  <select value={form.unite || ""} onChange={(e) => up({ unite: e.target.value })}>
                    <option value="">—</option>
                    {unitOptions.map((unit) => (
                      <option key={unit} value={unit}>{unit}</option>
                    ))}
                  </select>
                </label>
                <label>
                  <span>{t("invoice_category")}</span>
                  <input value={form.category} onChange={(e) => up({ category: e.target.value })} />
                </label>
                <label className="invoice-form-toggle">
                  <input type="checkbox" checked={form.en_activite} onChange={(e) => up({ en_activite: e.target.checked })} />
                  <span>{t("invoice_active")}</span>
                </label>
                <label className="invoice-form-toggle">
                  <input type="checkbox" checked={form.tax_exempt} onChange={(e) => up({ tax_exempt: e.target.checked })} />
                  <span>{t("invoice_tax_exempt")}</span>
                </label>
              </div>
            </div>
            <div className="invoice-form-section">
              <div className="invoice-editor-grid invoice-form-grid">
                <label>
                  <span>{t("invoice_revenue_account")}</span>
                  <input value={form.revenue_account} onChange={(e) => up({ revenue_account: e.target.value })} />
                </label>
                <label>
                  <span>{t("invoice_expense_account")}</span>
                  <input value={form.expense_account} onChange={(e) => up({ expense_account: e.target.value })} />
                </label>
                <label>
                  <span>{t("invoice_vat_account")}</span>
                  <input value={form.vat_account} onChange={(e) => up({ vat_account: e.target.value })} />
                </label>
                <label>
                  <span>{t("invoice_bic_account")}</span>
                  <input value={form.bic_account} onChange={(e) => up({ bic_account: e.target.value })} />
                </label>
                <label className="span-2">
                  <span>{t("invoice_custom_tax_rules")}</span>
                  <textarea rows="2" value={form.custom_tax_rules} onChange={(e) => up({ custom_tax_rules: e.target.value })} />
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
  const { t, lang } = useT();

  const [activeTab, setActiveTab] = useState("factures");
  const [listCollapsed, setListCollapsed] = useState(false);
  const [documents, setDocuments] = useState([]);
  const [localDrafts, setLocalDrafts] = useState([]);
  const [localItems, setLocalItems] = useState([]);
  const [syncQueue, setSyncQueue] = useState([]);
  const [draftNotice, setDraftNotice] = useState("");
  const [documentsLoading, setDocumentsLoading] = useState(false);
  const [documentsError, setDocumentsError] = useState("");
  const [selectedId, setSelectedId] = useState("");
  const [selectedInvoice, setSelectedInvoice] = useState(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailError, setDetailError] = useState("");
  const [mode, setMode] = useState("view");
  const [editorInvoice, setEditorInvoice] = useState(null);
  const [search, setSearch] = useState("");
  const [statutFilter, setStatutFilter] = useState("");
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");
  const [saving, setSaving] = useState(false);
  const [actionBusy, setActionBusy] = useState("");
  const [templates, setTemplates] = useState([]);
  const [templateId, setTemplateId] = useState(EMPTY_TEMPLATE_ID);
  const [previewHtml, setPreviewHtml] = useState("");
  const [previewLoading, setPreviewLoading] = useState(false);
  const [tiers, setTiers] = useState([]);
  const [tiersLoading, setTiersLoading] = useState(false);
  const [tiersError, setTiersError] = useState("");
  const [selectedTier, setSelectedTier] = useState(null);
  const [selectedTierInvoices, setSelectedTierInvoices] = useState([]);
  const [tierDocsLoading, setTierDocsLoading] = useState(false);
  const [tierDocsError, setTierDocsError] = useState("");
  const [tierSearch, setTierSearch] = useState("");
  const [articles, setArticles] = useState([]);
  const [articlesLoading, setArticlesLoading] = useState(false);
  const [articlesError, setArticlesError] = useState("");
  const [selectedArticle, setSelectedArticle] = useState(null);
  const [articleSearch, setArticleSearch] = useState("");
  const [itemSourceFilter, setItemSourceFilter] = useState("");
  const [itemStatusFilter, setItemStatusFilter] = useState("");
  const [tiersModal, setTiersModal] = useState(null); // null | { initial }
  const [articleModal, setArticleModal] = useState(null); // null | { initial }
  const [invoiceSettings, setInvoiceSettings] = useState(DEFAULT_INVOICE_SETTINGS);

  const debouncedSearch = useDebouncedValue(search, 300);
  const debouncedTierSearch = useDebouncedValue(tierSearch, 300);
  const debouncedArticleSearch = useDebouncedValue(articleSearch, 300);
  const isItemsTab = activeTab === "items";
  const documentTab = DOCUMENT_TABS.find((tab) => tab.id === activeTab) ?? null;
  const documentDrafts = useMemo(() => (
    localDrafts.filter((invoice) => invoice.nature === documentTab?.nature)
  ), [documentTab, localDrafts]);
  const visibleDocuments = useMemo(() => {
    const remoteIds = new Set(documents.map((invoice) => invoice.id));
    return [...documentDrafts.filter((invoice) => !remoteIds.has(invoice.id)), ...documents];
  }, [documentDrafts, documents]);
  const unitOptions = useMemo(() => {
    const units = [...invoiceSettings.invoice_units];
    for (const line of editorInvoice?.lignes || []) {
      if (line.unite && !units.includes(line.unite)) units.push(line.unite);
    }
    return units;
  }, [editorInvoice, invoiceSettings.invoice_units]);

  const loadEditorTiers = useCallback(
    async (query = "") => searchTiers(connId, query, "all", { limit: 20 }),
    [connId],
  );
  const loadEditorArticles = useCallback(
    async (query = "") => {
      const remote = await searchArticles(connId, query, { limit: 40 }).catch(() => []);
      return mergeCatalogItems(remote, localItems, query, invoiceSettings).slice(0, 20);
    },
    [connId, invoiceSettings, localItems],
  );

  useEffect(() => {
    if (!connId) return;
    setLocalDrafts(readLocalDrafts(connId));
    setLocalItems(readLocalItems(connId));
    setSyncQueue(readSyncQueue(connId));
    setDraftNotice("");
    console.debug("[invoice] active connection", {
      connectionId: connId,
      schemaEdition: schema?.edition || null,
    });
    getSettings()
      .then((settings) => setInvoiceSettings(normalizeInvoiceSettings(settings)))
      .catch(() => setInvoiceSettings(DEFAULT_INVOICE_SETTINGS));
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

  const refreshTiers = useCallback(() => {
    if (!connId) return Promise.resolve([]);
    setTiersLoading(true);
    setTiersError("");
    return searchTiers(connId, debouncedTierSearch, "all", { limit: 60 })
      .then((items) => {
        setTiers(items);
        return items;
      })
      .catch((error) => {
        setTiers([]);
        setTiersError(String(error));
        return [];
      })
      .finally(() => setTiersLoading(false));
  }, [connId, debouncedTierSearch]);

  const refreshArticles = useCallback(() => {
    if (!connId) return Promise.resolve([]);
    setArticlesLoading(true);
    setArticlesError("");
    return searchArticles(connId, debouncedArticleSearch, { limit: 60 })
      .then((items) => {
        const merged = mergeCatalogItems(items, localItems, debouncedArticleSearch, invoiceSettings);
        setArticles(merged);
        return merged;
      })
      .catch((error) => {
        const localOnly = mergeCatalogItems([], localItems, debouncedArticleSearch, invoiceSettings);
        setArticles(localOnly);
        setArticlesError(String(error));
        return localOnly;
      })
      .finally(() => setArticlesLoading(false));
  }, [connId, debouncedArticleSearch, invoiceSettings, localItems]);

  useEffect(() => {
    if (isItemsTab) {
      refreshArticles();
    }
  }, [isItemsTab, refreshArticles]);

  useEffect(() => {
    if (!connId || !documentTab) return;
    let cancelled = false;
    setDocumentsLoading(true);
    setDocumentsError("");
    listInvoices(connId, {
      nature: documentTab.nature,
      statut: statutFilter || null,
      dateFrom: dateFrom || null,
      dateTo: dateTo || null,
      search: debouncedSearch || null,
    })
      .then((items) => {
        if (cancelled) return;
        setDocuments(items);
        const nextVisible = [...documentDrafts, ...items];
        if (!selectedId && nextVisible[0] && mode !== "edit") {
          setSelectedId(nextVisible[0].id);
        } else if (selectedId && !nextVisible.some((item) => item.id === selectedId) && mode !== "edit") {
          setSelectedId(nextVisible[0]?.id || "");
          setSelectedInvoice(null);
        }
      })
      .catch((error) => {
        if (cancelled) return;
        setDocuments([]);
        setDocumentsError(String(error));
      })
      .finally(() => {
        if (!cancelled) setDocumentsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [connId, dateFrom, dateTo, debouncedSearch, documentDrafts, documentTab, mode, selectedId, statutFilter]);

  useEffect(() => {
    if (!connId || !selectedId || !documentTab || mode === "edit") return;
    const localDraft = localDrafts.find((invoice) => invoice.id === selectedId);
    if (localDraft) {
      setSelectedInvoice(localDraft);
      setDetailError("");
      setDetailLoading(false);
      return undefined;
    }
    let cancelled = false;
    setDetailLoading(true);
    setDetailError("");
    getInvoice(connId, selectedId)
      .then((invoice) => {
        if (!cancelled) setSelectedInvoice(invoice);
      })
      .catch((error) => {
        if (!cancelled) setDetailError(String(error));
      })
      .finally(() => {
        if (!cancelled) setDetailLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [connId, documentTab, localDrafts, mode, selectedId]);

  useEffect(() => {
    if (!connId || !selectedTier?.id) return;
    let cancelled = false;
    setTierDocsLoading(true);
    setTierDocsError("");
    listInvoices(connId, { tiersId: selectedTier.id })
      .then((items) => {
        if (!cancelled) setSelectedTierInvoices(items);
      })
      .catch((error) => {
        if (cancelled) return;
        setSelectedTierInvoices([]);
        setTierDocsError(String(error));
      })
      .finally(() => {
        if (!cancelled) setTierDocsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [connId, selectedTier]);

  const selectedTemplate = useMemo(
    () => templates.find((template) => template.id === templateId) ?? null,
    [templateId, templates],
  );

  const workingInvoice = mode === "edit" ? editorInvoice : selectedInvoice;
  const actionPending = actionBusy !== "";
  const currentStatus = workingInvoice?.statut || "Brouillon";
  const nextStatuses = STATUS_TRANSITIONS[currentStatus] || [];

  const openPreview = async (invoice, nextTemplateId = templateId) => {
    if (!invoice) return;
    setPreviewLoading(true);
    setMode("preview");
    try {
      const html = await renderInvoiceHtml(invoice, nextTemplateId, lang);
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

  const persistLocalDraft = useCallback((invoice, error = "") => {
    const draft = makeLocalDraft(invoice, error);
    setLocalDrafts((current) => {
      const next = [draft, ...current.filter((item) => item.id !== draft.id)];
      writeLocalDrafts(connId, next);
      return next;
    });
    setDraftNotice(
      t("invoice_local_saved_message"),
    );
    setSyncQueue((current) => {
      if (!error) return current;
      const next = [makeSyncQueueItem("invoice", draft, error), ...current].slice(0, 100);
      writeSyncQueue(connId, next);
      return next;
    });
    return draft;
  }, [connId, t]);

  const enqueueLocalSave = useCallback((type, payload, error = "") => {
    setSyncQueue((current) => {
      const next = [makeSyncQueueItem(type, payload, error), ...current].slice(0, 100);
      writeSyncQueue(connId, next);
      return next;
    });
    setDraftNotice(t("invoice_local_saved_message"));
  }, [connId, t]);

  const persistLocalItem = useCallback((item, error = "") => {
    const localItem = normalizeItem({
      ...item,
      id: item.id && String(item.id).startsWith("local-item-") ? item.id : `local-item-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      source: "local_draft",
      status: error ? "unsynced" : "draft",
      is_local_draft: true,
      sync_error: String(error || ""),
      updated_at: new Date().toISOString(),
    }, invoiceSettings);
    setLocalItems((current) => {
      const key = itemKey(localItem);
      const next = [localItem, ...current.filter((entry) => entry.id !== localItem.id && itemKey(entry) !== key)];
      writeLocalItems(connId, next);
      return next;
    });
    setSyncQueue((current) => {
      const next = [makeSyncQueueItem("article", localItem, error), ...current.filter((entry) => entry.payload?.id !== localItem.id)].slice(0, 100);
      writeSyncQueue(connId, next);
      return next;
    });
    setDraftNotice(t("invoice_local_item_saved"));
    return localItem;
  }, [connId, invoiceSettings, t]);

  const syncQueuedRecords = async () => {
    if (!syncQueue.length || actionPending) return;
    setActionBusy("sync");
    const remaining = [];
    const syncedArticleKeys = new Set();
    for (const item of syncQueue) {
      try {
        if (item.type === "invoice") {
          const payload = computeInvoice({ ...item.payload, id: String(item.payload?.id || "").startsWith("local-") ? "" : item.payload?.id });
          await (payload.id ? updateInvoice(connId, payload) : createInvoice(connId, payload));
        } else if (item.type === "article") {
          const payload = item.payload || {};
          await (payload.id && String(payload.id).startsWith("sdb-") ? updateArticle(connId, payload) : createArticle(connId, payload));
          syncedArticleKeys.add(itemKey(payload) || payload.id);
        } else if (item.type === "tiers") {
          const payload = item.payload || {};
          await (payload.id && String(payload.id).startsWith("sdb-") ? updateTiers(connId, payload) : createTiers(connId, payload));
        }
      } catch (error) {
        remaining.push({ ...item, error: String(error), updated_at: new Date().toISOString() });
      }
    }
    setSyncQueue(remaining);
    writeSyncQueue(connId, remaining);
    if (syncedArticleKeys.size) {
      setLocalItems((current) => {
        const next = current.filter((entry) => !syncedArticleKeys.has(itemKey(entry) || entry.id));
        writeLocalItems(connId, next);
        return next;
      });
      await refreshArticles();
    }
    if (!remaining.length) {
      setLocalDrafts([]);
      writeLocalDrafts(connId, []);
      setDraftNotice(t("invoice_sync_complete"));
      await listInvoices(connId, { nature: documentTab?.nature }).then(setDocuments).catch(() => {});
    } else {
      setDraftNotice(t("invoice_sync_partial", remaining.length));
    }
    setActionBusy("");
  };

  const saveCurrentInvoice = async (openPreviewAfter = false) => {
    if (!editorInvoice) return;

    if (!editorInvoice.tiers_id) {
      persistLocalDraft(editorInvoice, "client-required");
      return;
    }
    if (!editorInvoice.lignes || editorInvoice.lignes.length === 0) {
      persistLocalDraft(editorInvoice, "lines-required");
      return;
    }

    setSaving(true);
    try {
      const payload = computeInvoice(editorInvoice);
      if (payload.id && String(payload.id).startsWith("local-")) {
        const localDraft = persistLocalDraft(payload);
        setSelectedId(localDraft.id);
        setSelectedInvoice(localDraft);
        setEditorInvoice(localDraft);
        if (openPreviewAfter) {
          await openPreview(localDraft);
        } else {
          setMode("view");
        }
        return;
      }

      const saved = payload.id ? await updateInvoice(connId, payload) : await createInvoice(connId, payload);

      setSelectedId(saved.id);
      setSelectedInvoice(saved);
      setEditorInvoice(saved);
      setDraftNotice("");
      setLocalDrafts((current) => {
        const next = current.filter((item) => item.id !== payload.id);
        writeLocalDrafts(connId, next);
        return next;
      });

      if (openPreviewAfter) {
        await openPreview(saved);
      } else {
        setMode("view");
      }

      await listInvoices(connId, {
        nature: documentTab?.nature,
        statut: statutFilter || null,
        dateFrom: dateFrom || null,
        dateTo: dateTo || null,
        search: debouncedSearch || null,
      }).then(setDocuments);
    } catch (error) {
      console.error("[invoice] Save error:", error);
      const localDraft = persistLocalDraft(editorInvoice, error);
      setSelectedId(localDraft.id);
      setSelectedInvoice(localDraft);
      setEditorInvoice(localDraft);
      if (openPreviewAfter) {
        await openPreview(localDraft);
      } else {
        setMode("view");
      }
    } finally {
      setSaving(false);
    }
  };

  const handleStatusChange = async (newStatus) => {
    if (!selectedInvoice?.id || actionPending) return;
    if (selectedInvoice.is_local_draft) {
      const updated = persistLocalDraft({ ...selectedInvoice, statut: newStatus });
      setSelectedInvoice(updated);
      setSelectedId(updated.id);
      return;
    }
    setActionBusy("status");
    try {
      await updateStatut(connId, selectedInvoice.id, newStatus, "SDB");
      await refreshSelectedInvoice(selectedInvoice.id);
      const items = await listInvoices(connId, {
        nature: documentTab?.nature,
        statut: statutFilter || null,
        dateFrom: dateFrom || null,
        dateTo: dateTo || null,
        search: debouncedSearch || null,
      });
      setDocuments(items);
    } catch (error) {
      console.error("[invoice] Status update error:", error);
      alert(t("error") + ": " + error);
    } finally {
      setActionBusy("");
    }
  };

  const handleDelete = async () => {
    if (!selectedInvoice?.id || actionPending) return;
    if (!window.confirm(t("invoice_delete_confirm", selectedInvoice.numero || selectedInvoice.id))) return;
    setActionBusy("delete");
    try {
      if (selectedInvoice.is_local_draft) {
        const next = localDrafts.filter((invoice) => invoice.id !== selectedInvoice.id);
        setLocalDrafts(next);
        writeLocalDrafts(connId, next);
        setSelectedId(visibleDocuments.find((invoice) => invoice.id !== selectedInvoice.id)?.id || "");
        setSelectedInvoice(null);
        return;
      }
      await deleteInvoice(connId, selectedInvoice.id);
      const items = await listInvoices(connId, { nature: documentTab?.nature });
      setDocuments(items);
      setSelectedId(items[0]?.id || "");
      setSelectedInvoice(null);
    } catch (error) {
      console.error("[invoice] Delete error:", error);
      alert(t("error") + ": " + error);
    } finally {
      setActionBusy("");
    }
  };

  const handleComptabiliser = async () => {
    if (!selectedInvoice?.id || actionPending) return;
    if (selectedInvoice.is_local_draft) {
      setDraftNotice(t("invoice_sync_before_post"));
      return;
    }
    setActionBusy("post");
    try {
      await comptabiliserInvoice(connId, selectedInvoice.id);
      await refreshSelectedInvoice(selectedInvoice.id);
      const items = await listInvoices(connId, { nature: documentTab?.nature });
      setDocuments(items);
    } catch (error) {
      console.error("[invoice] Post error:", error);
      alert(t("error") + ": " + error);
    } finally {
      setActionBusy("");
    }
  };

  const handleExportPdf = async () => {
    const invoice = workingInvoice;
    if (!invoice || actionPending) return;
    setActionBusy("export");
    try {
      const filePath = await save({
        defaultPath: `${invoice.numero || "invoice"}.pdf`,
        filters: [{ name: "PDF", extensions: ["pdf"] }],
      });
      if (!filePath) return;
      await exportInvoicePdf(invoice, templateId, filePath, lang);
    } finally {
      setActionBusy("");
    }
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
      lignes: [...current.lignes, buildBlankLine(current.lignes.length + 1, invoiceSettings)],
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

    if (detailError) {
      return (
        <div className="empty-state">
          <FileText size={30} />
          <p>{detailError}</p>
        </div>
      );
    }

    if (mode === "edit" && editorInvoice) {
      const breakdown = getTvaBreakdown(editorInvoice);
      const bicBreakdown = getBicBreakdown(editorInvoice);

      return (
        <div className="invoice-editor compact">
          <div className="invoice-action-bar">
            {draftNotice ? <span className="invoice-draft-notice">{draftNotice}</span> : null}
            <button type="button" className="btn btn-sm" onClick={() => {
              setMode("view");
              setEditorInvoice(selectedInvoice);
            }}>
              {t("cancel")}
            </button>
            <button type="button" className="btn btn-sm" onClick={() => saveCurrentInvoice(false)} disabled={saving}>
              {saving ? <span className="mini-spinner" /> : <SaveIcon />}
              {t("invoice_save_draft")}
            </button>
            <button type="button" className="btn btn-sm btn-accent" onClick={() => saveCurrentInvoice(true)} disabled={saving}>
              {saving ? <span className="mini-spinner" /> : <Eye size={14} />}
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
              <label>
                <span>{t("invoice_reference")}</span>
                <input value={editorInvoice.reference} onChange={(event) => updateEditor({ reference: event.target.value })} />
              </label>
              <label>
                <span>{t("invoice_currency")}</span>
                <select value={editorInvoice.devise} onChange={(event) => updateEditor({ devise: event.target.value })}>
                  <option value="EUR">EUR (€)</option>
                  <option value="USD">USD ($)</option>
                  <option value="XOF">XOF (CFA)</option>
                </select>
              </label>
              <div className="span-2">
                <span className="invoice-editor-label">{t("invoice_customer")}</span>
                <SearchSelect
                  placeholder={t("invoice_tiers_placeholder")}
                  value={editorInvoice.tiers_id}
                  displayValue={[editorInvoice.tiers_code, editorInvoice.tiers_nom, editorInvoice.tiers_ville].filter(Boolean).join(" · ")}
                  searchFn={loadEditorTiers}
                  emptyLabel={t("invoice_no_client_found")}
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
              <button type="button" className="btn btn-sm" onClick={addLine}>
                <FilePlus2 size={13} />
                {t("invoice_add_line")}
              </button>
            </div>

            <div className="invoice-lines-table-wrap">
              <table className="invoice-lines-table compact">
                <thead>
                  <tr>
                    <th>#</th>
                    <th>{t("invoice_article")}</th>
                    <th>{t("invoice_label")}</th>
                    <th>{t("invoice_qty")}</th>
                    <th>{t("invoice_unit")}</th>
                    <th>{t("invoice_unit_price")}</th>
                    <th>{t("invoice_discount")}</th>
                    <th>{t("invoice_line_total_ht")}</th>
                    <th>{t("invoice_vat_rate")}</th>
                    <th>{t("invoice_vat_amount")}</th>
                    <th>{t("invoice_bic_rate")}</th>
                    <th>{t("invoice_bic_amount")}</th>
                    <th>{t("invoice_line_total_ttc")}</th>
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
                          searchFn={loadEditorArticles}
                          emptyLabel={t("invoice_no_article_found")}
                          onSelect={(article) => {
                            if (article.currency && article.currency !== editorInvoice.devise) {
                              setDraftNotice(t("invoice_currency_mismatch", article.currency, editorInvoice.devise));
                            }
                            updateLine(index, {
                              article_id: article.id,
                              article_code: article.code,
                              libelle: article.description ? `${article.libelle} - ${article.description}` : article.libelle,
                              prix_ht: article.prix_ht,
                              taux_tva: article.taux_tva,
                              taux_bic: article.taux_bic,
                              unite: article.unite,
                              tax_exempt: article.tax_exempt,
                              revenue_account: article.revenue_account,
                              expense_account: article.expense_account,
                              vat_account: article.vat_account,
                              bic_account: article.bic_account,
                              custom_tax_rules: article.custom_tax_rules,
                            });
                          }}
                          renderOption={(article) => (
                            <div className="invoice-search-option-copy">
                              <strong>{article.code} · {formatCurrency(article.prix_ht, article.currency || editorInvoice.devise)}</strong>
                              <span>{[article.libelle, article.reference, article.category, article.status === "unsynced" ? t("invoice_status_unsynced") : ""].filter(Boolean).join(" · ")}</span>
                            </div>
                          )}
                        />
                      </td>
                      <td><input value={line.libelle} onChange={(event) => updateLine(index, { libelle: event.target.value })} /></td>
                      <td><input type="number" step="0.01" value={line.quantite} onChange={(event) => updateLine(index, { quantite: event.target.value })} /></td>
                      <td>
                        <select value={line.unite || ""} onChange={(event) => updateLine(index, { unite: event.target.value })}>
                          <option value="">—</option>
                          {unitOptions.map((unit) => (
                            <option key={unit} value={unit}>{unit}</option>
                          ))}
                        </select>
                      </td>
                      <td><input type="number" step="0.01" value={line.prix_ht} onChange={(event) => updateLine(index, { prix_ht: event.target.value })} /></td>
                      <td>
                        <div style={{ display: "grid", gap: 4, minWidth: 96 }}>
                          <select value={line.remise_type || "percent"} onChange={(event) => updateLine(index, { remise_type: event.target.value })}>
                            <option value="percent">%</option>
                            <option value="fixed">{editorInvoice.devise}</option>
                          </select>
                          <input type="number" step="0.01" value={line.remise_valeur ?? line.remise_pct} onChange={(event) => updateLine(index, { remise_valeur: event.target.value })} />
                        </div>
                      </td>
                      <td className="num">{formatCurrency(line.montant_ht, editorInvoice.devise)}</td>
                      <td>
                        <div style={{ display: "flex", alignItems: "center", gap: 4 }}>
                          <input type="number" step="0.01" style={{ width: 60 }} value={line.taux_tva} onChange={(event) => updateLine(index, { taux_tva: event.target.value })} />
                          <select
                            style={{ width: 45, padding: "4px 2px" }}
                            value=""
                            onChange={(e) => {
                              if (e.target.value) updateLine(index, { taux_tva: Number(e.target.value) });
                              e.target.value = "";
                            }}
                          >
                            <option value="">⋯</option>
                            {invoiceSettings.invoice_vat_rates.map((rate) => (
                              <option key={rate} value={rate}>{rate}%</option>
                            ))}
                          </select>
                        </div>
                      </td>
                      <td className="num">{formatCurrency(line.montant_tva, editorInvoice.devise)}</td>
                      <td><input type="number" step="0.01" value={line.taux_bic} onChange={(event) => updateLine(index, { taux_bic: event.target.value })} /></td>
                      <td className="num">{formatCurrency(line.montant_bic, editorInvoice.devise)}</td>
                      <td className="num">{formatCurrency(line.montant_ttc, editorInvoice.devise)}</td>
                      <td>
                        <button type="button" className="btn btn-icon btn-sm" onClick={() => removeLine(index)}>
                          <Trash2 size={13} />
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
                  <textarea rows="3" value={editorInvoice.notes} onChange={(event) => updateEditor({ notes: event.target.value })} />
                </label>
                <label>
                  <span>{t("invoice_payment_conditions")}</span>
                  <textarea rows="2" value={editorInvoice.conditions} onChange={(event) => updateEditor({ conditions: event.target.value })} />
                </label>
              </div>
              <div className="invoice-editor-card totals">
                <DetailRow label={t("invoice_subtotal_before_discount")} value={formatCurrency(editorInvoice.subtotal_ht_before_discount, editorInvoice.devise)} />
                <DetailRow label={t("invoice_line_discounts")} value={formatCurrency(editorInvoice.total_line_discount, editorInvoice.devise)} />
                <DetailRow label={t("invoice_total_ht")} value={formatCurrency(editorInvoice.total_ht, editorInvoice.devise)} />
                <label className="invoice-inline-label">
                  <span>{t("invoice_global_discount")}</span>
                  <div style={{ display: "grid", gridTemplateColumns: "90px 1fr", gap: 8 }}>
                    <select value={editorInvoice.remise_globale_type || "percent"} onChange={(event) => updateEditor({ remise_globale_type: event.target.value })}>
                      <option value="percent">%</option>
                      <option value="fixed">{editorInvoice.devise}</option>
                    </select>
                    <input type="number" step="0.01" value={editorInvoice.remise_globale_valeur ?? editorInvoice.remise_globale} onChange={(event) => updateEditor({ remise_globale_valeur: event.target.value })} />
                  </div>
                </label>
                <div className="invoice-tva-breakdown">
                  {breakdown.map((row) => (
                    <div key={row.rate} className="invoice-detail-row">
                      <span>{t("invoice_vat_row", row.rate)}</span>
                      <strong>{formatCurrency(row.tax, editorInvoice.devise)}</strong>
                    </div>
                  ))}
                  {bicBreakdown.map((row) => (
                    <div key={`bic-${row.rate}`} className="invoice-detail-row">
                      <span>{t("invoice_bic_row", row.rate)}</span>
                      <strong>{formatCurrency(row.tax, editorInvoice.devise)}</strong>
                    </div>
                  ))}
                </div>
                <DetailRow label={t("invoice_total_vat")} value={formatCurrency(editorInvoice.total_tva, editorInvoice.devise)} />
                <DetailRow label={t("invoice_total_bic")} value={formatCurrency(editorInvoice.total_bic, editorInvoice.devise)} />
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
          {selectedInvoice.is_local_draft ? (
            <span className="invoice-draft-notice">
              {t("invoice_local_draft_notice")}
            </span>
          ) : null}
          <button
            type="button"
            className="btn btn-sm"
            disabled={actionPending || displayStatus(selectedInvoice.statut) === "Comptabilisé"}
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
            disabled={actionPending || !nextStatuses.length}
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

          <button type="button" className="btn btn-sm" onClick={() => openPreview(selectedInvoice)} disabled={actionPending}>
            <Eye size={14} />
            {t("invoice_preview")}
          </button>
          <button type="button" className="btn btn-sm" onClick={handleExportPdf} disabled={actionPending}>
            {actionBusy === "export" ? <span className="mini-spinner" /> : <BadgeEuro size={14} />}
            {t("invoice_export_pdf")}
          </button>
          <button
            type="button"
            className="btn btn-sm"
            disabled={actionPending || selectedInvoice.is_local_draft || selectedInvoice.nature !== "Facture" || displayStatus(selectedInvoice.statut) === "Comptabilisé"}
            onClick={handleComptabiliser}
          >
            <BadgeEuro size={14} />
            {t("invoice_post")}
          </button>
          <button
            type="button"
            className="btn btn-sm"
            disabled={actionPending}
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
            className="btn btn-sm btn-danger"
            disabled={actionPending || (!selectedInvoice.is_local_draft && selectedInvoice.statut !== "Brouillon")}
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
              {selectedInvoice.is_local_draft ? <span className="invoice-status-badge warning">Local</span> : <StatusBadge statut={selectedInvoice.statut} />}
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
                  <th>{t("invoice_discount")}</th>
                  <th>{t("invoice_line_total_ht")}</th>
                  <th>{t("invoice_vat_rate")}</th>
                  <th>{t("invoice_vat_amount")}</th>
                  <th>{t("invoice_bic_rate")}</th>
                  <th>{t("invoice_bic_amount")}</th>
                  <th>{t("invoice_line_total_ttc")}</th>
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
                    <td className="num">{line.remise_type === "fixed" ? formatCurrency(line.remise_valeur, selectedInvoice.devise) : `${round2(line.remise_pct)}%`}</td>
                    <td className="num">{formatCurrency(line.montant_ht, selectedInvoice.devise)}</td>
                    <td className="num">{round2(line.taux_tva)}%</td>
                    <td className="num">{formatCurrency(line.montant_tva, selectedInvoice.devise)}</td>
                    <td className="num">{round2(line.taux_bic)}%</td>
                    <td className="num">{formatCurrency(line.montant_bic, selectedInvoice.devise)}</td>
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
            <DetailRow label={t("invoice_line_discounts")} value={formatCurrency(selectedInvoice.total_line_discount, selectedInvoice.devise)} />
            <DetailRow label={t("invoice_global_discount")} value={formatCurrency(selectedInvoice.total_global_discount, selectedInvoice.devise)} />
            <DetailRow label={t("invoice_total_vat")} value={formatCurrency(selectedInvoice.total_tva, selectedInvoice.devise)} />
            <DetailRow label={t("invoice_total_bic")} value={formatCurrency(selectedInvoice.total_bic, selectedInvoice.devise)} />
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
                <div className="invoice-history-item-top">
                  <span>{entry.updated_at}</span>
                  <span className="invoice-history-item-actor">{entry.updated_by || "SDB"}</span>
                </div>
                <div className="invoice-history-item-main">
                  {displayStatus(entry.from_statut || "Brouillon")} → {displayStatus(entry.to_statut)}
                </div>
              </div>
            )) : <div className="sidebar-empty-copy">{t("invoice_no_history")}</div>}
          </div>
        </section>
      </div>
    );
  };

  const filteredItems = useMemo(() => {
    return articles.filter((item) => {
      if (itemSourceFilter && item.source !== itemSourceFilter) return false;
      if (itemStatusFilter && item.status !== itemStatusFilter) return false;
      return true;
    });
  }, [articles, itemSourceFilter, itemStatusFilter]);

  const renderItemsView = () => (
    <div className="invoice-items-view">
      <section className="invoice-editor-card">
        <div className="invoice-editor-section-head">
          <strong>{t("invoice_items_catalog")}</strong>
          <button type="button" className="btn btn-sm btn-accent" onClick={() => setArticleModal({ initial: { ...BLANK_ARTICLE, currency: invoiceSettings.invoice_default_currency } })}>
            <FilePlus2 size={13} />
            {t("invoice_new_article")}
          </button>
        </div>
        <div className="invoice-items-toolbar">
          <div className="invoice-searchbox">
            <Search size={14} />
            <input value={articleSearch} onChange={(event) => setArticleSearch(event.target.value)} placeholder={t("invoice_articles_search")} />
          </div>
          <select value={itemSourceFilter} onChange={(event) => setItemSourceFilter(event.target.value)}>
            <option value="">{t("invoice_filter_source_all")}</option>
            <option value="database">{t("invoice_source_database")}</option>
            <option value="local_draft">{t("invoice_source_local")}</option>
          </select>
          <select value={itemStatusFilter} onChange={(event) => setItemStatusFilter(event.target.value)}>
            <option value="">{t("invoice_filter_status_all")}</option>
            <option value="synced">{t("invoice_status_synced")}</option>
            <option value="unsynced">{t("invoice_status_unsynced")}</option>
            <option value="invalid">{t("invoice_status_invalid")}</option>
            <option value="draft">{t("invoice_status_draft")}</option>
          </select>
        </div>
      </section>

      <section className="invoice-editor-card">
        {articlesLoading ? (
          <div className="empty-state"><div className="spinner" /><p>{t("loading")}</p></div>
        ) : null}
        {!articlesLoading && articlesError && !filteredItems.length ? (
          <div className="invoice-form-error">{articlesError}</div>
        ) : null}
        {!articlesLoading && filteredItems.length ? (
          <div className="invoice-items-table-wrap">
            <table className="invoice-lines-table invoice-items-table">
              <thead>
                <tr>
                  <th>{t("invoice_item_name")}</th>
                  <th>{t("invoice_item_description")}</th>
                  <th>{t("invoice_unit_price")}</th>
                  <th>{t("invoice_currency")}</th>
                  <th>{t("invoice_vat_rate")}</th>
                  <th>{t("invoice_bic_rate")}</th>
                  <th>{t("invoice_revenue_account")}</th>
                  <th>{t("invoice_tax_account")}</th>
                  <th>{t("invoice_source")}</th>
                  <th>{t("invoice_status")}</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {filteredItems.map((item) => (
                  <tr key={item.id || item.code}>
                    <td>
                      <strong>{item.libelle || item.code}</strong>
                      <div className="invoice-item-subline">{[item.code, item.reference, item.category].filter(Boolean).join(" · ")}</div>
                    </td>
                    <td>{item.description || "—"}</td>
                    <td className="num">{formatCurrency(item.prix_ht, item.currency || invoiceSettings.invoice_default_currency)}</td>
                    <td>{item.currency || invoiceSettings.invoice_default_currency}</td>
                    <td className="num">{round2(item.taux_tva)}%</td>
                    <td className="num">{round2(item.taux_bic)}%</td>
                    <td>{item.revenue_account || item.expense_account || "—"}</td>
                    <td>{item.vat_account || item.bic_account || "—"}</td>
                    <td><span className="invoice-status-badge info">{item.source === "local_draft" ? t("invoice_source_local") : t("invoice_source_database")}</span></td>
                    <td><span className={`invoice-status-badge ${item.status === "synced" ? "success" : item.status === "invalid" ? "orange" : "warning"}`}>{t(`invoice_status_${item.status || "draft"}`)}</span></td>
                    <td>
                      <button type="button" className="btn btn-icon btn-sm" onClick={() => setArticleModal({ initial: item })} title={t("edit")}>
                        <PencilLine size={13} />
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : null}
        {!articlesLoading && !filteredItems.length && !articlesError ? (
          <div className="empty-state">
            <Package2 size={28} />
            <p>{t("invoice_items_empty")}</p>
          </div>
        ) : null}
      </section>
    </div>
  );

  return (
    <div className="invoicing-shell">
      <header className="invoicing-header">
        <div className="invoicing-header-start">
          <button type="button" className="btn btn-sm" onClick={onBack}>
            <ArrowLeft size={14} />
            {t("invoice_back")}
          </button>
          <div>
            <div className="settings-drawer-eyebrow">{t("sidebar_invoicing")}</div>
            <h2>{t("invoice_title")}</h2>
          </div>
        </div>
        <div className="invoicing-header-actions">
          {syncQueue.length ? (
            <button type="button" className="btn btn-sm" onClick={syncQueuedRecords} disabled={actionPending}>
              {actionBusy === "sync" ? <span className="mini-spinner" /> : null}
              {t("invoice_sync_queue", syncQueue.length)}
            </button>
          ) : null}
          <button type="button" className="btn btn-sm" onClick={onOpenTemplateDesigner}>
            <Printer size={14} />
            {t("invoice_template_appearance")}
          </button>
          <div className="invoice-schema-chip">{schema?.edition || "auto"}</div>
        </div>
      </header>

      <div className="invoice-tabbar">
        {[...DOCUMENT_TABS, { id: "items", icon: Package2, labelKey: "invoice_tab_articles" }].map((tab) => (
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
                if (tab.id === "items") refreshArticles();
              });
            }}
          />
        ))}
      </div>

      {isItemsTab ? (
        <div className="invoicing-items-layout">
          {renderItemsView()}
        </div>
      ) : (
      <div className={`invoicing-layout ${listCollapsed ? "list-collapsed" : ""}`}>
        <aside className="invoice-list-panel">
          <div className="invoice-list-header">
            <div className="invoice-list-toolbar">
              <div className="invoice-search-row">
                <div className="invoice-searchbox">
                  <Search size={14} />
                  <input
                    value={search}
                    onChange={(event) => setSearch(event.target.value)}
                    placeholder={t("invoice_search_placeholder")}
                  />
                </div>
                <div className="invoice-search-count" aria-live="polite">
                  {documentsLoading ? t("loading") : t("invoice_results_count", documents.length)}
                </div>
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
            <button className="panel-header-btn" onClick={() => setListCollapsed(true)} title={t("panel_hide_connections")}>
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M15 18l-6-6 6-6" />
              </svg>
            </button>
          </div>

          <div className="invoice-list-scroll">
            {documentsLoading ? (
              <div className="empty-state">
                <div className="spinner" />
                <p>{t("invoice_loading_list")}</p>
              </div>
            ) : documentsError ? (
              <div className="empty-state">
                <FileText size={28} />
                <p>{documentsError}</p>
              </div>
            ) : visibleDocuments.length ? visibleDocuments.map((invoice) => (
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
              if (!documentTab) return;
              const fresh = buildBlankInvoice(documentTab.nature, selectedTemplate, invoiceSettings);
              setSelectedId("");
              setSelectedInvoice(fresh);
              setEditorInvoice(fresh);
              setMode("edit");
            }}
          >
            <FilePlus2 size={14} />
            {documentTab ? t("invoice_new_document", t(documentTab.labelKey)) : t("invoice_new_document", "")}
          </button>
        </aside>

        {listCollapsed && (
          <div className="panel-rail">
            <button className="panel-rail-btn" onClick={() => setListCollapsed(false)} title={t("panel_show_connections")}>
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <rect x="4" y="4" width="8" height="16" rx="1" />
                <path d="M16 8l4 4-4 4" />
              </svg>
            </button>
          </div>
        )}

        <main className="invoice-detail-panel">
          {renderDocumentView()}
        </main>
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
            try {
              if (form.id && form.id.startsWith("sdb-")) {
                await updateTiers(connId, form);
              } else {
                await createTiers(connId, form);
              }
              refreshTiers();
            } catch (error) {
              enqueueLocalSave("tiers", form, error);
            }
          }}
          onClose={() => setTiersModal(null)}
        />
      )}

      {articleModal && (
        <ArticleFormModal
          t={t}
          initial={articleModal.initial}
          unitOptions={unitOptions}
          onSave={async (form) => {
            try {
              if (form.source === "local_draft" || String(form.id || "").startsWith("local-item-")) {
                persistLocalItem(form);
              } else if (form.id && form.id.startsWith("sdb-")) {
                await updateArticle(connId, form);
              } else {
                await createArticle(connId, form);
              }
              refreshArticles();
            } catch (error) {
              persistLocalItem(form, error);
            }
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
