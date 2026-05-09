import { useT } from "../i18n";

export default function AboutPage({ onBack, onOpenSettings }) {
  const { t } = useT();

  return (
    <div className="about-view">
      <div className="about-header">
        <div className="about-header-start">
          <button className="dashboard-back-btn" onClick={onBack}>
            {t("about_back")}
          </button>
          <div>
            <div className="dashboard-header-eyebrow">{t("about_eyebrow")}</div>
            <div className="dashboard-header-title">{t("about_title")}</div>
          </div>
        </div>

        <button className="btn btn-ghost btn-icon dashboard-settings-trigger" onClick={onOpenSettings} title={t("sidebar_settings")}>
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01A1.65 1.65 0 0 0 10.09 3H10a2 2 0 1 1 4 0h-.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01A1.65 1.65 0 0 0 21 10.09V10a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
          </svg>
        </button>
      </div>

      <div className="about-body about-body-simple">
        <section className="about-simple-card">
          <span className="about-kicker">{t("about_kicker")}</span>
          <h2>{t("about_headline")}</h2>
          <p>{t("about_intro")}</p>
        </section>

        <div className="about-simple-grid">
          <section className="about-card">
            <h3>{t("about_section_source")}</h3>
            <p>{t("about_source_body")}</p>
          </section>

          <section className="about-card">
            <h3>{t("about_section_training")}</h3>
            <p>{t("about_training_body")}</p>
          </section>

          <section className="about-card about-contact-card-simple">
            <h3>{t("about_author_title")}</h3>
            <strong>Louis BERTSON</strong>
            <span>{t("about_author_role")}</span>
            <div className="about-contact-list">
              <a href="tel:+22656920671">+226 56 92 06 71</a>
              <a href="tel:+421950420361">+421 9 50 42 03 61</a>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
