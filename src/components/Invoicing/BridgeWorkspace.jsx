import { useCallback, useEffect, useMemo, useState } from "react";
import {
  BadgeEuro,
  BookOpen,
  Boxes,
  Files,
  Landmark,
  PencilLine,
  Plus,
  RefreshCw,
  Send,
  Settings2,
  ShieldCheck,
  ToggleLeft,
} from "lucide-react";

import {
  deactivateBridgeManagementRecord,
  generateBridgeInvoicePosting,
  generateBridgePaymentPosting,
  getBridgeAccountingConfiguration,
  getBridgeManagementData,
  getBridgeMasterData,
  initializeBridge,
  listBridgeDocuments,
  listBridgePostingBatches,
  prepareBridgeExport,
  registerBridgePayment,
  saveBridgeAccountingConfiguration,
  saveBridgeManagementRecord,
  searchArticles,
  searchTiers,
  transformBridgeQuote,
  validateBridgeDocument,
} from "../../hooks/useTauri";
import { useT } from "../../i18n";
import { SearchSelect } from "./InvoiceInputs";

const today = () => new Date().toISOString().slice(0, 10);
const EMPTY_MASTER_DATA = {
  accounts: [],
  journals: [],
  product_families: [],
  accounting_categories: [],
  tax_codes: [],
  payment_methods: [],
  product_profiles: [],
};
const EMPTY_MANAGEMENT = {
  dashboard: {},
  partner_profiles: [],
  accounting_schemas: [],
  context_mappings: [],
  document_journal_mappings: [],
  integration_profiles: [],
  export_jobs: [],
};
const EMPTY_CONFIGURATION = {
  partner_code: "",
  partner_account: "411000",
  product_code: "",
  product_family_code: "",
  accounting_category_code: "",
  revenue_account: "701000",
  expense_account: "",
  tax_rate: 18,
  tax_account: "443000",
  payment_method_code: "bank",
  payment_journal_code: "BQ",
  payment_debit_account: "521000",
};

const TABS = [
  ["overview", Landmark, "bridge_tab_overview"],
  ["catalogs", BookOpen, "bridge_tab_catalogs"],
  ["mappings", Boxes, "bridge_tab_mappings"],
  ["settings", Settings2, "bridge_tab_settings"],
  ["models", ShieldCheck, "bridge_tab_models"],
  ["documents", Files, "bridge_tab_documents"],
  ["postings", BadgeEuro, "bridge_tab_postings"],
  ["transfers", Send, "bridge_tab_transfers"],
];

const SETTINGS_SECTIONS = {
  accounting_category: {
    title: "bridge_accounting_categories",
    keyField: "code",
    fields: [
      ["code", "invoice_code"],
      ["name", "invoice_name"],
      ["revenue_account", "bridge_revenue_account", "account"],
      ["expense_account", "invoice_expense_account", "account"],
      ["tax_account", "bridge_collected_vat_account", "account"],
    ],
  },
  product_family: {
    title: "bridge_product_families",
    keyField: "code",
    fields: [
      ["code", "invoice_code"],
      ["name", "invoice_name"],
      ["accounting_category_code", "bridge_accounting_category", "category"],
    ],
  },
  tax_code: {
    title: "bridge_tax_codes",
    keyField: "code",
    fields: [
      ["code", "invoice_code"],
      ["name", "invoice_name"],
      ["tax_type", "bridge_tax_type"],
      ["rate", "invoice_vat_rate", "number"],
      ["collected_account", "bridge_collected_vat_account", "account"],
      ["deductible_account", "bridge_deductible_vat_account", "account"],
    ],
  },
  payment_method: {
    title: "bridge_payment_methods",
    keyField: "code",
    fields: [
      ["code", "invoice_code"],
      ["name", "invoice_name"],
      ["journal_code", "bridge_payment_journal", "journal"],
      ["debit_account", "bridge_bank_cash_account", "account"],
    ],
  },
  document_journal_mapping: {
    title: "bridge_document_journal_mappings",
    keyField: "document_type",
    fields: [
      ["document_type", "invoice_nature"],
      ["journal_code", "dashboard_journal", "journal"],
    ],
  },
  partner_profile: {
    title: "bridge_customer_mapping",
    keyField: "partner_code",
    fields: [
      ["partner_code", "bridge_customer_code"],
      ["partner_type", "invoice_tiers_type"],
      ["accounting_category_code", "bridge_accounting_category", "category"],
      ["collective_account", "bridge_customer_collective_account", "account"],
    ],
  },
  accounting_schema: {
    title: "bridge_accounting_models",
    keyField: "id",
    fields: [
      ["code", "invoice_code"],
      ["name", "invoice_name"],
      ["operation_type", "bridge_operation_type"],
      ["direction", "bridge_direction"],
      ["journal_code", "dashboard_journal", "journal"],
      ["edition_scope", "bridge_sage_edition"],
    ],
  },
  integration_profile: {
    title: "bridge_integration_profiles",
    keyField: "id",
    fields: [
      ["name", "invoice_name"],
      ["adapter_type", "bridge_adapter"],
      ["sage_edition", "bridge_sage_edition"],
      ["configuration_json", "bridge_configuration_json"],
    ],
  },
};

function money(value, currency = "XOF", lang = "fr") {
  return new Intl.NumberFormat(lang === "fr" ? "fr-FR" : "en-US", {
    style: "currency",
    currency,
  }).format(Number(value) || 0);
}

function BridgeTabs({ active, onChange, t }) {
  return (
    <div className="bridge-tabs">
      {TABS.map(([id, Icon, label]) => (
        <button key={id} type="button" className={active === id ? "active" : ""} onClick={() => onChange(id)}>
          <Icon size={14} />
          {t(label)}
        </button>
      ))}
    </div>
  );
}

function DataTable({ columns, rows, empty, actions }) {
  return (
    <div className="invoice-items-table-wrap">
      <table className="invoice-lines-table invoice-items-table bridge-management-table">
        <thead><tr>{columns.map((column) => <th key={column.key}>{column.label}</th>)}{actions ? <th /> : null}</tr></thead>
        <tbody>
          {rows.length ? rows.map((row, index) => (
            <tr key={row.id || row.code || row[0] || index}>
              {columns.map((column) => <td key={column.key} className={column.numeric ? "num" : ""}>{column.render ? column.render(row) : row[column.key] ?? "—"}</td>)}
              {actions ? <td>{actions(row)}</td> : null}
            </tr>
          )) : <tr><td colSpan={columns.length + (actions ? 1 : 0)}>{empty}</td></tr>}
        </tbody>
      </table>
    </div>
  );
}

function RecordEditor({ entity, initial, masterData, onSave, onClose, t }) {
  const definition = SETTINGS_SECTIONS[entity];
  const [form, setForm] = useState({ active: true, ...initial });
  const field = (name, label, type) => {
    const value = form[name] ?? "";
    const update = (next) => setForm((current) => ({ ...current, [name]: next }));
    if (type === "account") {
      return <label key={name}><span>{t(label)}</span><select value={value} onChange={(event) => update(event.target.value)}><option value="">—</option>{masterData.accounts.map((item) => <option key={item.code} value={item.code}>{item.code} · {item.name}</option>)}</select></label>;
    }
    if (type === "journal") {
      return <label key={name}><span>{t(label)}</span><select value={value} onChange={(event) => update(event.target.value)}><option value="">—</option>{masterData.journals.map((item) => <option key={item.code} value={item.code}>{item.code} · {item.name}</option>)}</select></label>;
    }
    if (type === "category") {
      return <label key={name}><span>{t(label)}</span><select value={value} onChange={(event) => update(event.target.value)}><option value="">—</option>{masterData.accounting_categories.map((item) => <option key={item.code} value={item.code}>{item.code} · {item.name}</option>)}</select></label>;
    }
    return <label key={name}><span>{t(label)}</span><input type={type === "number" ? "number" : "text"} value={value} onChange={(event) => update(type === "number" ? Number(event.target.value) : event.target.value)} /></label>;
  };
  return (
    <div className="modal-overlay invoice-form-overlay" onClick={onClose}>
      <div className="modal invoice-form-modal bridge-record-modal" onClick={(event) => event.stopPropagation()}>
        <div className="invoice-form-header"><strong>{t(definition.title)}</strong><button type="button" className="btn btn-icon" onClick={onClose}>×</button></div>
        <div className="invoice-form-body"><div className="invoice-editor-grid invoice-form-grid">{definition.fields.map(([name, label, type]) => field(name, label, type))}</div></div>
        <div className="invoice-form-footer"><button type="button" className="btn" onClick={onClose}>{t("cancel")}</button><button type="button" className="btn btn-accent" onClick={() => onSave(form)}>{t("save")}</button></div>
      </div>
    </div>
  );
}

export default function BridgeWorkspace({ connId, schema }) {
  const { t, lang } = useT();
  const [activeTab, setActiveTab] = useState("overview");
  const [documents, setDocuments] = useState([]);
  const [batches, setBatches] = useState([]);
  const [configuration, setConfiguration] = useState(EMPTY_CONFIGURATION);
  const [masterData, setMasterData] = useState(EMPTY_MASTER_DATA);
  const [management, setManagement] = useState(EMPTY_MANAGEMENT);
  const [editor, setEditor] = useState(null);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [packagePreview, setPackagePreview] = useState("");
  const [catalogSearch, setCatalogSearch] = useState("");

  const refresh = useCallback(async () => {
    await initializeBridge(connId);
    const [nextDocuments, nextBatches, nextConfiguration, nextMasterData, nextManagement] = await Promise.all([
      listBridgeDocuments(connId),
      listBridgePostingBatches(connId),
      getBridgeAccountingConfiguration(connId),
      getBridgeMasterData(connId),
      getBridgeManagementData(connId),
    ]);
    setDocuments(nextDocuments);
    setBatches(nextBatches);
    setConfiguration(nextConfiguration);
    setMasterData(nextMasterData);
    setManagement(nextManagement);
  }, [connId]);

  useEffect(() => {
    refresh().catch((nextError) => setError(String(nextError)));
  }, [refresh]);

  const run = async (name, action, success = "") => {
    setBusy(name); setError(""); setNotice("");
    try {
      const result = await action();
      await refresh();
      if (success) setNotice(success);
      return result;
    } catch (nextError) {
      setError(String(nextError));
      return null;
    } finally {
      setBusy("");
    }
  };

  const searchClients = useCallback((query) => searchTiers(connId, query, "clients", { limit: 20 }), [connId]);
  const searchProducts = useCallback((query) => searchArticles(connId, query, { limit: 20 }), [connId]);
  const displayStatus = (status) => t(`bridge_status_${String(status || "").toLowerCase()}`);
  const mappedProductCodes = useMemo(() => new Set(masterData.product_profiles.filter((item) => item.active).map((item) => item.product_code)), [masterData.product_profiles]);
  const filteredAccounts = masterData.accounts.filter((item) => `${item.code} ${item.name}`.toLowerCase().includes(catalogSearch.toLowerCase()));
  const filteredJournals = masterData.journals.filter((item) => `${item.code} ${item.name}`.toLowerCase().includes(catalogSearch.toLowerCase()));

  const saveRecord = (entity, record) => run(`save-${entity}`, () => saveBridgeManagementRecord(connId, entity, record), t("bridge_record_saved"));
  const deactivate = (entity, key) => run(`deactivate-${entity}-${key}`, () => deactivateBridgeManagementRecord(connId, entity, key), t("bridge_record_deactivated"));
  const saveMappings = () => run("save-mappings", () => saveBridgeAccountingConfiguration(connId, configuration), t("bridge_rules_saved"));
  const exportBatch = (batch, adapter) => run(`export-${batch.id}`, async () => {
    const payload = await prepareBridgeExport(connId, batch.id, adapter);
    setPackagePreview(JSON.stringify(payload, null, 2));
  }, t("bridge_export_prepared", adapter));
  const registerAndPostPayment = (document) => run(`pay-${document.id}`, async () => {
    const payment = await registerBridgePayment(connId, {
      invoice_id: document.id,
      payment_date: today(),
      amount: document.total_ttc,
      currency: document.currency,
      payment_method_code: configuration.payment_method_code,
      reference: `PAY-${document.number}`,
      journal_code: "",
      debit_account: "",
    });
    await generateBridgePaymentPosting(connId, payment.id);
  }, t("bridge_payment_posted", document.number));

  const managementActions = (entity, keyField) => (row) => (
    <div className="bridge-row-actions">
      <button type="button" className="btn btn-icon btn-sm" onClick={() => setEditor({ entity, initial: row })}><PencilLine size={13} /></button>
      <button type="button" className="btn btn-icon btn-sm" onClick={() => deactivate(entity, row[keyField])}><ToggleLeft size={13} /></button>
    </div>
  );

  const catalogColumns = [
    { key: "code", label: t("invoice_code"), render: (row) => <strong>{row.code}</strong> },
    { key: "name", label: t("invoice_name") },
    { key: "item_type", label: t("invoice_tiers_type") },
    { key: "source", label: t("invoice_source") },
  ];

  const renderOverview = () => (
    <>
      <section className="bridge-kpi-grid">
        {[
          ["documents", "bridge_commercial_documents"],
          ["draft_documents", "bridge_draft_documents"],
          ["posting_batches", "bridge_posting_batches"],
          ["pending_exports", "bridge_pending_exports"],
          ["mapped_products", "bridge_articles_mapped"],
          ["mapped_partners", "bridge_customers_mapped"],
        ].map(([key, label]) => <button key={key} type="button" onClick={() => setActiveTab(key.includes("export") ? "transfers" : key.includes("posting") ? "postings" : key.includes("mapped") ? "mappings" : "documents")}><strong>{management.dashboard[key] || 0}</strong><span>{t(label)}</span></button>)}
      </section>
      <section className="invoice-editor-card">
        <div className="invoice-editor-section-head"><strong>{t("bridge_configuration_health")}</strong></div>
        <div className="bridge-health-grid">
          <span className={masterData.journals.length ? "ready" : "blocked"}>{masterData.journals.length} · {t("bridge_journals_detected")}</span>
          <span className={masterData.accounts.length ? "ready" : "blocked"}>{masterData.accounts.length} · {t("bridge_accounts_detected")}</span>
          <span className={masterData.product_families.length ? "ready" : "blocked"}>{masterData.product_families.length} · {t("bridge_families_configured")}</span>
          <span className={masterData.accounting_categories.length ? "ready" : "blocked"}>{masterData.accounting_categories.length} · {t("bridge_categories_configured")}</span>
          <span className={management.accounting_schemas.length ? "ready" : "blocked"}>{management.accounting_schemas.length} · {t("bridge_accounting_models")}</span>
        </div>
      </section>
    </>
  );

  const renderCatalogs = () => (
    <>
      <section className="invoice-editor-card bridge-toolbar"><div className="invoice-searchbox"><input value={catalogSearch} onChange={(event) => setCatalogSearch(event.target.value)} placeholder={t("search_placeholder")} /></div><span>{t("bridge_sage_read_only_hint")}</span></section>
      <section className="bridge-two-column"><div className="invoice-editor-card"><div className="invoice-editor-section-head"><strong>{t("bridge_sage_accounts")}</strong><span>{filteredAccounts.length}</span></div><DataTable columns={catalogColumns} rows={filteredAccounts} empty={t("no_data")} /></div><div className="invoice-editor-card"><div className="invoice-editor-section-head"><strong>{t("bridge_sage_journals")}</strong><span>{filteredJournals.length}</span></div><DataTable columns={catalogColumns} rows={filteredJournals} empty={t("no_data")} /></div></section>
    </>
  );

  const renderMappings = () => (
    <>
      <section className="invoice-editor-card">
        <div className="invoice-editor-section-head"><strong>{t("bridge_product_mapping")}</strong><span>{mappedProductCodes.size}</span></div>
        <div className="invoice-editor-grid invoice-form-grid">
          <div><span className="invoice-editor-label">{t("invoice_article")}</span><SearchSelect placeholder={t("invoice_article_placeholder")} value={configuration.product_code} displayValue={configuration.product_code} searchFn={searchProducts} emptyLabel={t("invoice_no_article_found")} onSelect={(item) => setConfiguration((current) => ({ ...current, product_code: item.code }))} renderOption={(item) => <div className="invoice-search-option-copy"><strong>{item.code}</strong><span>{item.libelle}</span></div>} /></div>
          <label><span>{t("bridge_product_family")}</span><select value={configuration.product_family_code} onChange={(event) => setConfiguration((current) => ({ ...current, product_family_code: event.target.value }))}><option value="">—</option>{masterData.product_families.map((item) => <option key={item.code} value={item.code}>{item.code} · {item.name}</option>)}</select></label>
          <label><span>{t("bridge_accounting_category")}</span><select value={configuration.accounting_category_code} onChange={(event) => setConfiguration((current) => ({ ...current, accounting_category_code: event.target.value }))}><option value="">—</option>{masterData.accounting_categories.map((item) => <option key={item.code} value={item.code}>{item.code} · {item.name}</option>)}</select></label>
          <label><span>{t("bridge_tax_code")}</span><select value={masterData.tax_codes.find((item) => item.rate === configuration.tax_rate)?.code || ""} onChange={(event) => { const tax = masterData.tax_codes.find((item) => item.code === event.target.value); setConfiguration((current) => ({ ...current, tax_rate: tax?.rate || 0, tax_account: tax?.account_code || current.tax_account })); }}><option value="">—</option>{masterData.tax_codes.map((item) => <option key={item.code} value={item.code}>{item.code} · {item.rate}%</option>)}</select></label>
        </div>
      </section>
      <section className="invoice-editor-card">
        <div className="invoice-editor-section-head"><strong>{t("bridge_customer_mapping")}</strong></div>
        <div className="invoice-editor-grid invoice-form-grid">
          <div><span className="invoice-editor-label">{t("invoice_customer")}</span><SearchSelect placeholder={t("invoice_tiers_placeholder")} value={configuration.partner_code} displayValue={configuration.partner_code} searchFn={searchClients} emptyLabel={t("invoice_no_client_found")} onSelect={(item) => setConfiguration((current) => ({ ...current, partner_code: item.code }))} renderOption={(item) => <div className="invoice-search-option-copy"><strong>{item.code}</strong><span>{item.nom}</span></div>} /></div>
          <label><span>{t("bridge_customer_collective_account")}</span><select value={configuration.partner_account} onChange={(event) => setConfiguration((current) => ({ ...current, partner_account: event.target.value }))}><option value="">—</option>{masterData.accounts.map((item) => <option key={item.code} value={item.code}>{item.code} · {item.name}</option>)}</select></label>
        </div>
        <div className="invoice-action-bar"><span>{t("bridge_rules_snapshot_hint")}</span><button type="button" className="btn btn-sm btn-accent" onClick={saveMappings}><ShieldCheck size={13} />{t("save")}</button></div>
      </section>
      <section className="invoice-editor-card"><div className="invoice-editor-section-head"><strong>{t("bridge_existing_mappings")}</strong></div><DataTable columns={[{ key: "product_code", label: t("bridge_product_code") }, { key: "product_family_code", label: t("bridge_product_family") }, { key: "accounting_category_code", label: t("bridge_accounting_category") }, { key: "revenue_account", label: t("bridge_revenue_account") }, { key: "tax_code", label: t("bridge_tax_code") }]} rows={masterData.product_profiles} empty={t("no_data")} actions={(row) => <button type="button" className="btn btn-icon btn-sm" onClick={() => deactivate("product_profile", row.product_code)}><ToggleLeft size={13} /></button>} /></section>
      <section className="invoice-editor-card"><div className="invoice-editor-section-head"><strong>{t("bridge_customer_mapping")}</strong><button type="button" className="btn btn-sm" onClick={() => setEditor({ entity: "partner_profile", initial: { partner_type: "customer" } })}><Plus size={13} />{t("new")}</button></div><DataTable columns={[{ key: "partner", label: t("bridge_customer_code"), render: (row) => row[0] }, { key: "type", label: t("invoice_tiers_type"), render: (row) => row[1] }, { key: "category", label: t("bridge_accounting_category"), render: (row) => row[2] || "—" }, { key: "account", label: t("bridge_customer_collective_account"), render: (row) => row[3] }]} rows={management.partner_profiles} empty={t("no_data")} actions={(row) => <div className="bridge-row-actions"><button type="button" className="btn btn-icon btn-sm" onClick={() => setEditor({ entity: "partner_profile", initial: { partner_code: row[0], partner_type: row[1], accounting_category_code: row[2], collective_account: row[3], active: Boolean(row[4]) } })}><PencilLine size={13} /></button><button type="button" className="btn btn-icon btn-sm" onClick={() => deactivate("partner_profile", row[0])}><ToggleLeft size={13} /></button></div>} /></section>
    </>
  );

  const settingRows = (entity) => {
    if (entity === "accounting_category") return masterData.accounting_categories.map((item) => ({ id: item.id, code: item.code, name: item.name, revenue_account: item.account_code, active: item.active }));
    if (entity === "product_family") return masterData.product_families.map((item) => ({ id: item.id, code: item.code, name: item.name, accounting_category_code: item.parent_code, active: item.active }));
    if (entity === "tax_code") return masterData.tax_codes.map((item) => ({ code: item.code, name: item.name, tax_type: item.parent_code, rate: item.rate, collected_account: item.account_code, active: item.active }));
    if (entity === "payment_method") return masterData.payment_methods.map((item) => ({ code: item.code, name: item.name, journal_code: item.parent_code, debit_account: item.account_code, active: item.active }));
    if (entity === "document_journal_mapping") return management.document_journal_mappings.map((row) => ({ document_type: row[0], journal_code: row[1] }));
    return [];
  };

  const renderSettings = () => (
    <div className="bridge-settings-grid">
      {["accounting_category", "product_family", "tax_code", "payment_method", "document_journal_mapping"].map((entity) => {
        const definition = SETTINGS_SECTIONS[entity];
        const rows = settingRows(entity);
        const actions = entity === "document_journal_mapping"
          ? (row) => <button type="button" className="btn btn-icon btn-sm" onClick={() => setEditor({ entity, initial: row })}><PencilLine size={13} /></button>
          : managementActions(entity, definition.keyField);
        return <section key={entity} className="invoice-editor-card"><div className="invoice-editor-section-head"><strong>{t(definition.title)}</strong><button type="button" className="btn btn-sm" onClick={() => setEditor({ entity, initial: {} })}><Plus size={13} />{t("new")}</button></div><DataTable columns={definition.fields.slice(0, 4).map(([key, label]) => ({ key, label: t(label) }))} rows={rows} empty={t("no_data")} actions={actions} /></section>;
      })}
    </div>
  );

  const renderModels = () => (
    <>
      <section className="invoice-editor-card"><div className="invoice-editor-section-head"><strong>{t("bridge_accounting_models")}</strong><button type="button" className="btn btn-sm" onClick={() => setEditor({ entity: "accounting_schema", initial: { direction: "credit", edition_scope: "all" } })}><Plus size={13} />{t("new")}</button></div><DataTable columns={[{ key: "code", label: t("invoice_code"), render: (row) => <strong>{row[1]}</strong> }, { key: "name", label: t("invoice_name"), render: (row) => row[2] }, { key: "operation", label: t("bridge_operation_type"), render: (row) => row[3] }, { key: "journal", label: t("dashboard_journal"), render: (row) => row[5] }, { key: "lines", label: t("invoice_lines"), render: (row) => row[8], numeric: true }]} rows={management.accounting_schemas} empty={t("no_data")} actions={(row) => <div className="bridge-row-actions"><button type="button" className="btn btn-icon btn-sm" onClick={() => setEditor({ entity: "accounting_schema", initial: { id: row[0], code: row[1], name: row[2], operation_type: row[3], direction: row[4], journal_code: row[5], edition_scope: row[6], active: Boolean(row[7]) } })}><PencilLine size={13} /></button><button type="button" className="btn btn-icon btn-sm" onClick={() => deactivate("accounting_schema", row[0])}><ToggleLeft size={13} /></button></div>} /></section>
      <section className="invoice-editor-card"><div className="invoice-editor-section-head"><strong>{t("bridge_context_mappings")}</strong><span>{management.context_mappings.length}</span></div><DataTable columns={[{ key: "operation", label: t("bridge_operation_type"), render: (row) => row[1] }, { key: "document", label: t("invoice_nature"), render: (row) => row[2] || "—" }, { key: "payment", label: t("bridge_payment_method"), render: (row) => row[5] || "—" }, { key: "schema", label: t("bridge_accounting_models"), render: (row) => row[6] }, { key: "priority", label: t("bridge_priority"), render: (row) => row[7], numeric: true }]} rows={management.context_mappings} empty={t("no_data")} /></section>
      <div className="invoice-draft-notice">{t("bridge_guided_model_hint")}</div>
    </>
  );

  const renderDocuments = () => (
    <section className="invoice-editor-card"><div className="invoice-editor-section-head"><strong>{t("bridge_commercial_documents")}</strong><span>{documents.length}</span></div><DataTable columns={[{ key: "number", label: t("bridge_type_number"), render: (row) => <><strong>{row.document_type} {row.number}</strong><div className="invoice-item-subline">{row.document_date}</div></> }, { key: "status", label: t("invoice_status"), render: (row) => <span className="invoice-status-badge info">{displayStatus(row.status)}</span> }, { key: "partner_name", label: t("invoice_customer"), render: (row) => `${row.partner_code} · ${row.partner_name}` }, { key: "total_ttc", label: t("bridge_total"), render: (row) => money(row.total_ttc, row.currency, lang), numeric: true }, { key: "source_document_id", label: t("bridge_source") }]} rows={documents} empty={t("no_data")} actions={(document) => <div className="bridge-row-actions">{document.status === "Draft" ? <button type="button" className="btn btn-sm" onClick={() => run(`validate-${document.id}`, () => validateBridgeDocument(connId, document.id))}>{t("bridge_validate")}</button> : null}{document.document_type === "Devis" && document.status === "Validated" ? <button type="button" className="btn btn-sm" onClick={() => run(`transform-${document.id}`, () => transformBridgeQuote(connId, document.id))}>{t("bridge_transform")}</button> : null}{document.document_type === "Facture" && document.status === "Validated" ? <button type="button" className="btn btn-sm" onClick={() => run(`post-${document.id}`, () => generateBridgeInvoicePosting(connId, document.id))}>{t("bridge_generate_posting")}</button> : null}{document.document_type === "Facture" && ["Validated", "Posted"].includes(document.status) ? <button type="button" className="btn btn-sm" onClick={() => registerAndPostPayment(document)}>{t("bridge_register_post_payment")}</button> : null}</div>} /></section>
  );

  const renderPostings = () => (
    <section className="invoice-editor-card"><div className="invoice-editor-section-head"><strong>{t("bridge_posting_batches")}</strong><span>{batches.length}</span></div><DataTable columns={[{ key: "source_kind", label: t("bridge_source"), render: (row) => t(`bridge_source_${row.source_kind}`) }, { key: "journal_code", label: t("bridge_journal_piece"), render: (row) => `${row.journal_code} · ${row.piece_number}` }, { key: "total_debit", label: t("dashboard_debit"), render: (row) => money(row.total_debit, "XOF", lang), numeric: true }, { key: "total_credit", label: t("dashboard_credit"), render: (row) => money(row.total_credit, "XOF", lang), numeric: true }, { key: "lines", label: t("invoice_lines"), render: (row) => row.lines.length, numeric: true }, { key: "status", label: t("invoice_status"), render: (row) => displayStatus(row.status) }]} rows={batches} empty={t("no_data")} /></section>
  );

  const renderTransfers = () => (
    <>
      <section className="invoice-editor-card"><div className="invoice-editor-section-head"><strong>{t("bridge_integration_profiles")}</strong><button type="button" className="btn btn-sm" onClick={() => setEditor({ entity: "integration_profile", initial: { adapter_type: schema?.edition || "neutral", sage_edition: schema?.edition || "generic" } })}><Plus size={13} />{t("new")}</button></div><DataTable columns={[{ key: "name", label: t("invoice_name"), render: (row) => row[1] }, { key: "adapter", label: t("bridge_adapter"), render: (row) => row[2] }, { key: "edition", label: t("bridge_sage_edition"), render: (row) => row[3] }, { key: "delivery", label: t("bridge_delivery"), render: () => t("bridge_delivery_disabled") }]} rows={management.integration_profiles} empty={t("no_data")} actions={(row) => <div className="bridge-row-actions"><button type="button" className="btn btn-icon btn-sm" onClick={() => setEditor({ entity: "integration_profile", initial: { id: row[0], name: row[1], adapter_type: row[2], sage_edition: row[3], active: Boolean(row[5]), configuration_json: row[6] } })}><PencilLine size={13} /></button><button type="button" className="btn btn-icon btn-sm" onClick={() => deactivate("integration_profile", row[0])}><ToggleLeft size={13} /></button></div>} /></section>
      <section className="invoice-editor-card"><div className="invoice-editor-section-head"><strong>{t("bridge_posting_batches")}</strong></div><DataTable columns={[{ key: "piece_number", label: t("bridge_journal_piece"), render: (row) => `${row.journal_code} · ${row.piece_number}` }, { key: "status", label: t("invoice_status"), render: (row) => displayStatus(row.status) }, { key: "total", label: t("bridge_total"), render: (row) => money(row.total_debit, "XOF", lang), numeric: true }]} rows={batches} empty={t("no_data")} actions={(row) => <button type="button" className="btn btn-sm" onClick={() => exportBatch(row, schema?.edition || "neutral")}><Send size={12} />{t("bridge_prepare_export")}</button>} /></section>
      <section className="invoice-editor-card"><div className="invoice-editor-section-head"><strong>{t("bridge_export_jobs")}</strong><span>{management.export_jobs.length}</span></div><DataTable columns={[{ key: "batch", label: t("bridge_source"), render: (row) => row[1] }, { key: "adapter", label: t("bridge_adapter"), render: (row) => row[2] }, { key: "status", label: t("invoice_status"), render: (row) => row[3] }, { key: "error", label: t("error"), render: (row) => row[5] || "—" }, { key: "attempts", label: t("bridge_attempts"), render: (row) => row[8], numeric: true }]} rows={management.export_jobs} empty={t("no_data")} /></section>
      {packagePreview ? <pre className="query-code-preview bridge-package-preview">{packagePreview}</pre> : null}
    </>
  );

  const content = {
    overview: renderOverview,
    catalogs: renderCatalogs,
    mappings: renderMappings,
    settings: renderSettings,
    models: renderModels,
    documents: renderDocuments,
    postings: renderPostings,
    transfers: renderTransfers,
  }[activeTab];

  return (
    <div className="bridge-console">
      <section className="bridge-console-header">
        <div><strong>{t("bridge_title")}</strong><span>{t("bridge_subtitle", schema?.edition || t("bridge_unknown"))}</span></div>
        <button type="button" className="btn btn-sm" onClick={() => run("refresh", refresh)} disabled={Boolean(busy)}><RefreshCw size={13} />{t("refresh")}</button>
      </section>
      <BridgeTabs active={activeTab} onChange={setActiveTab} t={t} />
      {error ? <div className="invoice-form-error">{error}</div> : null}
      {notice ? <div className="invoice-draft-notice">{notice}</div> : null}
      <main className="bridge-console-body">{content()}</main>
      {editor ? <RecordEditor entity={editor.entity} initial={editor.initial} masterData={masterData} t={t} onClose={() => setEditor(null)} onSave={async (record) => { await saveRecord(editor.entity, record); setEditor(null); }} /> : null}
    </div>
  );
}
