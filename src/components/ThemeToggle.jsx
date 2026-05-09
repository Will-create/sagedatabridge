import { useT } from "../i18n";
import { useTheme } from "../theme";

export default function ThemeToggle() {
  const { t } = useT();
  const { theme, setTheme } = useTheme();

  return (
    <div className="theme-toggle" role="group" aria-label={t("theme_label")}>
      <button
        type="button"
        className={`theme-toggle-btn ${theme === "light" ? "active" : ""}`}
        aria-pressed={theme === "light"}
        title={t("theme_light")}
        onClick={() => setTheme("light")}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <circle cx="12" cy="12" r="4" />
          <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41" />
        </svg>
        <span>{t("theme_light")}</span>
      </button>

      <button
        type="button"
        className={`theme-toggle-btn ${theme === "dark" ? "active" : ""}`}
        aria-pressed={theme === "dark"}
        title={t("theme_dark")}
        onClick={() => setTheme("dark")}
      >
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
          <path d="M21 12.79A9 9 0 1 1 11.21 3c0.13 0 0.26 0.01 0.39 0.02A7 7 0 0 0 21 12.79z" />
        </svg>
        <span>{t("theme_dark")}</span>
      </button>
    </div>
  );
}
