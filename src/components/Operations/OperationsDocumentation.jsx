import { useEffect } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  BookOpen,
  CheckCircle2,
  Database,
  FolderOpen,
  HardDrive,
  Layers,
  RefreshCw,
  Shield,
  Sparkles,
  Table2,
  Undo2,
} from "lucide-react";
import { useT } from "../../i18n";

const SECTION_IDS = [
  ["quick", "operations_doc_nav_quick", Sparkles],
  ["connections", "operations_doc_nav_connections", Database],
  ["tables", "operations_doc_nav_tables", Table2],
  ["paths", "operations_doc_nav_paths", FolderOpen],
  ["guardrails", "operations_doc_nav_guardrails", Shield],
  ["backup", "operations_doc_nav_backup", HardDrive],
  ["restore", "operations_doc_nav_restore", Undo2],
  ["resume", "operations_doc_nav_resume", RefreshCw],
  ["examples", "operations_doc_nav_examples", Layers],
  ["troubleshooting", "operations_doc_nav_trouble", AlertTriangle],
  ["salvage", "operations_doc_nav_salvage", Shield],
];

function DocSection({ id, title, icon: Icon, children }) {
  return (
    <section id={id} className="invoice-doc-section">
      <div className="invoice-doc-section-head">
        <Icon size={18} />
        <h2>{title}</h2>
      </div>
      {children}
    </section>
  );
}

function DocNote({ type = "info", title, children }) {
  const Icon = type === "warning" ? AlertTriangle : CheckCircle2;
  return (
    <div className={`invoice-doc-note ${type}`}>
      <Icon size={16} />
      <div>
        <strong>{title}</strong>
        <p>{children}</p>
      </div>
    </div>
  );
}

export default function OperationsDocumentation({ onBack, initialAnchor = "" }) {
  const { t } = useT();

  useEffect(() => {
    if (!initialAnchor) return;
    const node = document.getElementById(initialAnchor);
    node?.scrollIntoView({ behavior: "smooth", block: "start" });
  }, [initialAnchor]);

  return (
    <div className="invoice-doc-page operations-doc-page">
      <aside className="invoice-doc-nav">
        <div className="invoice-doc-nav-title">
          <BookOpen size={17} />
          <span>{t("operations_doc_nav")}</span>
        </div>
        {SECTION_IDS.map(([id, key, Icon]) => (
          <a key={id} href={`#${id}`}>
            <Icon size={14} />
            {t(key)}
          </a>
        ))}
      </aside>

      <main className="invoice-doc-content">
        <div className="invoice-doc-hero">
          <button type="button" className="btn btn-sm" onClick={onBack}>
            <ArrowLeft size={14} />
            {t("operations_doc_back")}
          </button>
          <div>
            <span>{t("operations_doc_kicker")}</span>
            <h1>{t("operations_doc_title")}</h1>
            <p>{t("operations_doc_intro")}</p>
          </div>
        </div>

        <DocSection id="quick" title={t("operations_doc_quick_title")} icon={Sparkles}>
          <p>{t("operations_doc_quick_body")}</p>
          <ol className="invoice-doc-steps">
            <li>{t("operations_doc_quick_1")}</li>
            <li>{t("operations_doc_quick_2")}</li>
            <li>{t("operations_doc_quick_3")}</li>
            <li>{t("operations_doc_quick_4")}</li>
          </ol>
        </DocSection>

        <DocSection id="connections" title={t("operations_doc_conn_title")} icon={Database}>
          <p>{t("operations_doc_conn_body")}</p>
          <DocNote title={t("operations_doc_conn_note_title")}>{t("operations_doc_conn_note")}</DocNote>
        </DocSection>

        <DocSection id="tables" title={t("operations_doc_tables_title")} icon={Table2}>
          <p>{t("operations_doc_tables_body")}</p>
        </DocSection>

        <DocSection id="paths" title={t("operations_doc_paths_title")} icon={FolderOpen}>
          <p>{t("operations_doc_paths_body")}</p>
          <DocNote type="warning" title={t("operations_doc_paths_warn_title")}>{t("operations_doc_paths_warn")}</DocNote>
        </DocSection>

        <DocSection id="guardrails" title={t("operations_doc_guard_title")} icon={Shield}>
          <p>{t("operations_doc_guard_body")}</p>
          <p><strong>{t("operations_doc_guard_block_title")}</strong> {t("operations_doc_guard_block")}</p>
          <p><strong>{t("operations_doc_guard_warn_title")}</strong> {t("operations_doc_guard_warn")}</p>
          <DocNote type="warning" title={t("operations_doc_guard_never_title")}>{t("operations_doc_guard_never")}</DocNote>
        </DocSection>

        <DocSection id="backup" title={t("operations_doc_backup_title")} icon={HardDrive}>
          <p>{t("operations_doc_backup_body")}</p>
        </DocSection>

        <DocSection id="restore" title={t("operations_doc_restore_title")} icon={Undo2}>
          <p>{t("operations_doc_restore_body")}</p>
        </DocSection>

        <DocSection id="resume" title={t("operations_doc_resume_title")} icon={RefreshCw}>
          <p>{t("operations_doc_resume_body")}</p>
        </DocSection>

        <DocSection id="examples" title={t("operations_doc_examples_title")} icon={Layers}>
          <div className="invoice-doc-grid">
            <article className="invoice-doc-example"><strong>{t("operations_doc_ex1_title")}</strong><p>{t("operations_doc_ex1")}</p></article>
            <article className="invoice-doc-example"><strong>{t("operations_doc_ex2_title")}</strong><p>{t("operations_doc_ex2")}</p></article>
            <article className="invoice-doc-example"><strong>{t("operations_doc_ex3_title")}</strong><p>{t("operations_doc_ex3")}</p></article>
            <article className="invoice-doc-example"><strong>{t("operations_doc_ex4_title")}</strong><p>{t("operations_doc_ex4")}</p></article>
          </div>
        </DocSection>

        <DocSection id="troubleshooting" title={t("operations_doc_trouble_title")} icon={AlertTriangle}>
          <p>{t("operations_doc_trouble_body")}</p>
        </DocSection>

        <DocSection id="salvage" title={t("operations_doc_salvage_title")} icon={Shield}>
          <p>{t("operations_doc_salvage_body")}</p>
          <DocNote type="info" title={t("operations_doc_salvage_guide_title")}>{t("operations_doc_salvage_guide")}</DocNote>
          <DocNote type="warning" title={t("operations_doc_salvage_warn_title")}>{t("operations_doc_salvage_warn")}</DocNote>
        </DocSection>
      </main>
    </div>
  );
}
