import { useT } from "../i18n";

export default function LangToggle() {
  const { lang, setLang, t } = useT();
  return (
    <button
      className="btn btn-ghost btn-sm"
      title={lang === "en" ? "Passer en français" : "Switch to English"}
      onClick={() => setLang(lang === "en" ? "fr" : "en")}
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: 9.02,
        fontWeight: 700,
        letterSpacing: "0.06em",
        padding: "2.46px 6.56px",
        color: "var(--accent)",
        border: "1px solid var(--border-mid)",
      }}
    >
      {t("lang_toggle")}
    </button>
  );
}
