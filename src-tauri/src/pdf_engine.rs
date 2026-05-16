use std::collections::BTreeMap;
use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::invoice_commands::InvoiceHeader;
use crate::state::{AppState, InvoiceTemplate};

fn default_company_name() -> String {
    "Sage Data Bridge".to_string()
}

fn builtin_templates() -> Vec<InvoiceTemplate> {
    vec![
        InvoiceTemplate {
            id: "builtin-modern".to_string(),
            connection_id: String::new(),
            name: "Moderne Bleu".to_string(),
            primary_color: "#1e3a5f".to_string(),
            secondary_color: "#2563eb".to_string(),
            font_family: "Helvetica".to_string(),
            font_size_px: 10,
            show_logo: false,
            logo_base64: String::new(),
            company_name: default_company_name(),
            company_address: String::new(),
            company_siret: String::new(),
            company_tva: String::new(),
            company_phone: String::new(),
            company_email: String::new(),
            company_website: String::new(),
            footer_text: "Merci de votre confiance".to_string(),
            legal_mentions: String::new(),
            show_bank_details: false,
            bank_iban: String::new(),
            bank_bic: String::new(),
            conditions_paiement: "Paiement a reception de facture.".to_string(),
            mention_tva: String::new(),
            layout: "modern".to_string(),
        },
        InvoiceTemplate {
            id: "builtin-classic".to_string(),
            connection_id: String::new(),
            name: "Classique".to_string(),
            primary_color: "#21486a".to_string(),
            secondary_color: "#6b7280".to_string(),
            font_family: "Times New Roman".to_string(),
            font_size_px: 10,
            show_logo: false,
            logo_base64: String::new(),
            company_name: default_company_name(),
            company_address: String::new(),
            company_siret: String::new(),
            company_tva: String::new(),
            company_phone: String::new(),
            company_email: String::new(),
            company_website: String::new(),
            footer_text: "Document genere par Sage Data Bridge".to_string(),
            legal_mentions: String::new(),
            show_bank_details: false,
            bank_iban: String::new(),
            bank_bic: String::new(),
            conditions_paiement: "Reglement selon les conditions convenues.".to_string(),
            mention_tva: String::new(),
            layout: "classic".to_string(),
        },
        InvoiceTemplate {
            id: "builtin-minimal".to_string(),
            connection_id: String::new(),
            name: "Minimaliste".to_string(),
            primary_color: "#111827".to_string(),
            secondary_color: "#d1d5db".to_string(),
            font_family: "Arial".to_string(),
            font_size_px: 10,
            show_logo: false,
            logo_base64: String::new(),
            company_name: default_company_name(),
            company_address: String::new(),
            company_siret: String::new(),
            company_tva: String::new(),
            company_phone: String::new(),
            company_email: String::new(),
            company_website: String::new(),
            footer_text: "Merci de votre confiance".to_string(),
            legal_mentions: String::new(),
            show_bank_details: false,
            bank_iban: String::new(),
            bank_bic: String::new(),
            conditions_paiement: "Paiement a 30 jours.".to_string(),
            mention_tva: String::new(),
            layout: "minimal".to_string(),
        },
        InvoiceTemplate {
            id: "builtin-corporate".to_string(),
            connection_id: String::new(),
            name: "Corporate".to_string(),
            primary_color: "#0f8f7c".to_string(),
            secondary_color: "#1de8c8".to_string(),
            font_family: "Helvetica".to_string(),
            font_size_px: 10,
            show_logo: false,
            logo_base64: String::new(),
            company_name: default_company_name(),
            company_address: String::new(),
            company_siret: String::new(),
            company_tva: String::new(),
            company_phone: String::new(),
            company_email: String::new(),
            company_website: String::new(),
            footer_text: "Votre partenaire de gestion".to_string(),
            legal_mentions: String::new(),
            show_bank_details: false,
            bank_iban: String::new(),
            bank_bic: String::new(),
            conditions_paiement: "Paiement par virement bancaire.".to_string(),
            mention_tva: String::new(),
            layout: "corporate".to_string(),
        },
    ]
}

fn merge_templates(configured: &[InvoiceTemplate], connection_id: Option<&str>) -> Vec<InvoiceTemplate> {
    let mut templates = builtin_templates();
    templates.extend(
        configured
            .iter()
            .filter(|template| {
                connection_id
                    .map(|connection_id| template.connection_id == connection_id)
                    .unwrap_or(true)
            })
            .cloned(),
    );
    templates
}

fn resolve_template(state: &AppState, template_id: &str) -> Result<InvoiceTemplate, String> {
    let config = state.config.lock().map_err(|error| error.to_string())?;
    let all_templates = merge_templates(&config.invoice_templates, None);
    let target_id = if template_id.trim().is_empty() {
        config.active_template_id.clone()
    } else {
        template_id.trim().to_string()
    };

    all_templates
        .into_iter()
        .find(|template| template.id == target_id)
        .ok_or_else(|| format!("Invoice template '{}' not found", target_id))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn nl2br(value: &str) -> String {
    escape_html(value).replace('\n', "<br />")
}

fn format_currency_fr(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    let negative = rounded.is_sign_negative();
    let abs = rounded.abs();
    let major = abs.trunc() as i64;
    let minor = ((abs - major as f64) * 100.0).round() as i64;
    let reversed = major
        .to_string()
        .chars()
        .rev()
        .collect::<Vec<_>>();
    let grouped = reversed
        .chunks(3)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .rev()
        .collect::<String>();
    format!(
        "{}{},{} \u{20AC}",
        if negative { "-" } else { "" },
        grouped,
        format!("{:02}", minor)
    )
}

fn format_number_fr(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    format!("{:.2}", rounded).replace('.', ",")
}

fn invoice_title(nature: &str) -> &'static str {
    match nature.trim() {
        "Avoir" => "AVOIR",
        "Proforma" => "PROFORMA",
        "Commande" => "BON DE COMMANDE",
        "BL" => "BON DE LIVRAISON",
        _ => "FACTURE",
    }
}

fn logo_html(template: &InvoiceTemplate) -> String {
    if !template.show_logo || template.logo_base64.trim().is_empty() {
        return String::new();
    }
    let src = if template.logo_base64.trim_start().starts_with("data:") {
        template.logo_base64.clone()
    } else {
        format!("data:image/png;base64,{}", template.logo_base64.trim())
    };
    format!(
        "<img src=\"{}\" alt=\"logo\" style=\"max-height:72px; max-width:160px; object-fit:contain;\" />",
        escape_html(&src)
    )
}

fn bank_html(template: &InvoiceTemplate) -> String {
    if !template.show_bank_details {
        return String::new();
    }
    format!(
        "<div class=\"bank-block\"><strong>Coordonnees bancaires</strong><div>IBAN : {}</div><div>BIC : {}</div></div>",
        escape_html(&template.bank_iban),
        escape_html(&template.bank_bic)
    )
}

fn watermark_html(invoice: &InvoiceHeader) -> String {
    match invoice.statut.as_str() {
        "Brouillon" | "Proforma" => format!(
            "<div class=\"watermark\">{}</div>",
            escape_html(&invoice.statut.to_uppercase())
        ),
        _ => String::new(),
    }
}

fn statut_badge_html(invoice: &InvoiceHeader) -> String {
    match invoice.statut.as_str() {
        "Brouillon" | "Proforma" => format!(
            "<span class=\"status-badge\">{}</span>",
            escape_html(&invoice.statut.to_uppercase())
        ),
        _ => String::new(),
    }
}

fn lines_html(invoice: &InvoiceHeader) -> String {
    invoice
        .lignes
        .iter()
        .map(|line| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"num\">{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}%</td><td class=\"num\">{}</td><td class=\"num\">{}%</td><td class=\"num\">{}</td></tr>",
                line.ordre,
                escape_html(&line.article_code),
                escape_html(&line.libelle),
                format_number_fr(line.quantite),
                escape_html(&line.unite),
                format_currency_fr(line.prix_ht),
                format_number_fr(line.remise_pct),
                format_currency_fr(line.montant_ht),
                format_number_fr(line.taux_tva),
                format_currency_fr(line.montant_ttc),
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn tva_breakdown_html(invoice: &InvoiceHeader) -> String {
    let mut breakdown = BTreeMap::<String, (f64, f64)>::new();
    for line in &invoice.lignes {
        let key = format!("{:.2}", line.taux_tva);
        let entry = breakdown.entry(key).or_insert((0.0, 0.0));
        entry.0 += line.montant_ht;
        entry.1 += line.montant_tva;
    }

    breakdown
        .into_iter()
        .map(|(rate, (base, tax))| {
            format!(
                "<tr><td>TVA {}%</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
                rate.replace('.', ","),
                format_currency_fr(base),
                format_currency_fr(tax),
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn company_block(template: &InvoiceTemplate) -> String {
    let mut lines = Vec::new();
    if !template.company_name.trim().is_empty() {
        lines.push(escape_html(&template.company_name));
    }
    if !template.company_address.trim().is_empty() {
        lines.push(nl2br(&template.company_address));
    }
    if !template.company_phone.trim().is_empty() {
        lines.push(format!("Tel. {}", escape_html(&template.company_phone)));
    }
    if !template.company_email.trim().is_empty() {
        lines.push(escape_html(&template.company_email));
    }
    if !template.company_website.trim().is_empty() {
        lines.push(escape_html(&template.company_website));
    }
    lines.join("<br />")
}

fn tiers_cp_ville(invoice: &InvoiceHeader) -> String {
    match (invoice.tiers_cp.trim(), invoice.tiers_ville.trim()) {
        ("", "") => String::new(),
        ("", ville) => ville.to_string(),
        (cp, "") => cp.to_string(),
        (cp, ville) => format!("{} {}", cp, ville),
    }
}

fn common_replacements(invoice: &InvoiceHeader, template: &InvoiceTemplate) -> Vec<(&'static str, String)> {
    let conditions = if invoice.conditions.trim().is_empty() {
        template.conditions_paiement.clone()
    } else {
        invoice.conditions.clone()
    };

    vec![
        ("__PRIMARY_COLOR__", template.primary_color.clone()),
        ("__SECONDARY_COLOR__", template.secondary_color.clone()),
        ("__FONT_FAMILY__", template.font_family.clone()),
        ("__FONT_SIZE__", template.font_size_px.to_string()),
        ("__COMPANY_NAME__", escape_html(&template.company_name)),
        ("__COMPANY_ADDRESS__", nl2br(&template.company_address)),
        ("__COMPANY_SIRET__", escape_html(&template.company_siret)),
        ("__COMPANY_TVA__", escape_html(&template.company_tva)),
        ("__COMPANY_PHONE__", escape_html(&template.company_phone)),
        ("__COMPANY_EMAIL__", escape_html(&template.company_email)),
        ("__COMPANY_WEBSITE__", escape_html(&template.company_website)),
        ("__COMPANY_BLOCK__", company_block(template)),
        ("__COMPANY_LOGO_HTML__", logo_html(template)),
        ("__INVOICE_NUMERO__", escape_html(&invoice.numero)),
        ("__INVOICE_DATE__", escape_html(&invoice.date)),
        ("__INVOICE_ECHEANCE__", escape_html(&invoice.date_echeance)),
        ("__INVOICE_NATURE__", invoice_title(&invoice.nature).to_string()),
        ("__TIERS_NOM__", escape_html(&invoice.tiers_nom)),
        ("__TIERS_ADRESSE__", nl2br(&invoice.tiers_adresse)),
        ("__TIERS_CP_VILLE__", escape_html(&tiers_cp_ville(invoice))),
        ("__TIERS_SIRET__", escape_html(&invoice.tiers_siret)),
        ("__TIERS_TVA__", escape_html(&invoice.tiers_tva_intra)),
        ("__LINES_HTML__", lines_html(invoice)),
        ("__TOTAL_HT__", format_currency_fr(invoice.total_ht)),
        ("__TOTAL_TVA__", format_currency_fr(invoice.total_tva)),
        ("__TOTAL_TTC__", format_currency_fr(invoice.total_ttc)),
        ("__TVA_BREAKDOWN_HTML__", tva_breakdown_html(invoice)),
        ("__NOTES__", nl2br(&invoice.notes)),
        ("__CONDITIONS__", nl2br(&conditions)),
        ("__FOOTER_TEXT__", nl2br(&template.footer_text)),
        ("__LEGAL_MENTIONS__", nl2br(&template.legal_mentions)),
        ("__BANK_HTML__", bank_html(template)),
        ("__MENTION_TVA__", nl2br(&template.mention_tva)),
        ("__STATUT_BADGE_HTML__", statut_badge_html(invoice)),
        ("__WATERMARK_HTML__", watermark_html(invoice)),
    ]
}

fn apply_replacements(mut html: String, replacements: Vec<(&'static str, String)>) -> String {
    for (key, value) in replacements {
        html = html.replace(key, &value);
    }
    html
}

fn render_modern(invoice: &InvoiceHeader, template: &InvoiceTemplate) -> String {
    let template_html = r#"<!doctype html>
<html lang="fr">
<head>
  <meta charset="utf-8" />
  <title>__INVOICE_NATURE__ __INVOICE_NUMERO__</title>
  <style>
    @page { size: A4; margin: 15mm; }
    @media print {
      body { print-color-adjust: exact; -webkit-print-color-adjust: exact; }
    }
    body {
      margin: 0;
      color: #101828;
      font-family: "__FONT_FAMILY__", sans-serif;
      font-size: __FONT_SIZE__px;
      background: #fff;
    }
    .invoice {
      position: relative;
      display: flex;
      flex-direction: column;
    }
    .watermark {
      position: fixed;
      inset: 0;
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 92px;
      font-weight: 800;
      color: rgba(15, 23, 42, 0.05);
      transform: rotate(-32deg);
      pointer-events: none;
      text-transform: uppercase;
    }
    .hero {
      background: __PRIMARY_COLOR__;
      color: #fff;
      padding: 26px 28px 22px;
      border-radius: 24px 24px 14px 14px;
      display: grid;
      grid-template-columns: 1fr auto;
      gap: 20px;
      align-items: start;
    }
    .hero h1 {
      margin: 0 0 8px;
      font-size: 30px;
      letter-spacing: -0.03em;
    }
    .hero-meta {
      font-size: 12px;
      line-height: 1.7;
      opacity: 0.92;
    }
    .status-badge {
      display: inline-flex;
      margin-top: 12px;
      padding: 6px 10px;
      border-radius: 999px;
      background: rgba(255,255,255,0.18);
      font-size: 11px;
      font-weight: 700;
      letter-spacing: 0.08em;
    }
    .header-grid {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 18px;
      margin: 18px 0 22px;
    }
    .card {
      background: #f8fafc;
      border: 1px solid #e2e8f0;
      border-radius: 16px;
      padding: 18px;
    }
    .card h2 {
      margin: 0 0 10px;
      font-size: 12px;
      letter-spacing: 0.1em;
      text-transform: uppercase;
      color: __PRIMARY_COLOR__;
    }
    table {
      width: 100%;
      border-collapse: collapse;
      margin-top: 10px;
    }
    thead th {
      background: __SECONDARY_COLOR__;
      color: #fff;
      font-size: 11px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      padding: 10px 8px;
      text-align: left;
    }
    tbody tr:nth-child(even) td { background: #f9fafb; }
    td {
      padding: 9px 8px;
      border-bottom: 1px solid #e5e7eb;
      vertical-align: top;
    }
    .num { text-align: right; white-space: nowrap; }
    .totals-wrap {
      display: grid;
      grid-template-columns: 1fr 320px;
      gap: 20px;
      margin-top: 22px;
      align-items: start;
    }
    .totals {
      margin-left: auto;
      border-left: 5px solid __PRIMARY_COLOR__;
      background: #f8fafc;
      border-radius: 12px;
      padding: 14px 18px;
      width: 100%;
    }
    .totals-row {
      display: flex;
      justify-content: space-between;
      gap: 16px;
      padding: 6px 0;
    }
    .grand-total {
      margin-top: 10px;
      padding-top: 12px;
      border-top: 2px solid #cbd5e1;
      font-size: 20px;
      font-weight: 800;
      color: __PRIMARY_COLOR__;
    }
    .notes-grid {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 18px;
      margin-top: 22px;
    }
    .footer {
      margin-top: 26px;
      background: __PRIMARY_COLOR__;
      color: #fff;
      border-radius: 14px;
      padding: 14px 18px;
      display: flex;
      justify-content: space-between;
      gap: 20px;
      align-items: end;
    }
    .footer small {
      display: block;
      opacity: 0.9;
      line-height: 1.6;
    }
    tr { page-break-inside: avoid; }
  </style>
</head>
<body>
  <div class="invoice">
    __WATERMARK_HTML__
    <section class="hero">
      <div>
        <h1>__COMPANY_NAME__</h1>
        <div class="hero-meta">__COMPANY_BLOCK__</div>
        __STATUT_BADGE_HTML__
      </div>
      <div>__COMPANY_LOGO_HTML__</div>
    </section>

    <div class="header-grid">
      <div class="card">
        <h2>Client</h2>
        <strong>__TIERS_NOM__</strong><br />
        __TIERS_ADRESSE__<br />
        __TIERS_CP_VILLE__<br />
        SIRET : __TIERS_SIRET__<br />
        TVA : __TIERS_TVA__
      </div>
      <div class="card">
        <h2>Document</h2>
        <strong>__INVOICE_NATURE__ N° __INVOICE_NUMERO__</strong><br />
        Date : __INVOICE_DATE__<br />
        Echeance : __INVOICE_ECHEANCE__<br />
        SIRET : __COMPANY_SIRET__<br />
        TVA intra : __COMPANY_TVA__
      </div>
    </div>

    <table>
      <thead>
        <tr>
          <th>#</th><th>Article</th><th>Libelle</th><th class="num">Qte</th><th>Unite</th>
          <th class="num">PU HT</th><th class="num">Remise</th><th class="num">HT</th><th class="num">TVA</th><th class="num">TTC</th>
        </tr>
      </thead>
      <tbody>__LINES_HTML__</tbody>
    </table>

    <div class="totals-wrap">
      <div class="card">
        <h2>Detail TVA</h2>
        <table>
          <tbody>__TVA_BREAKDOWN_HTML__</tbody>
        </table>
      </div>
      <div class="totals">
        <div class="totals-row"><span>Total HT</span><strong>__TOTAL_HT__</strong></div>
        <div class="totals-row"><span>Total TVA</span><strong>__TOTAL_TVA__</strong></div>
        <div class="totals-row grand-total"><span>Total TTC</span><strong>__TOTAL_TTC__</strong></div>
      </div>
    </div>

    <div class="notes-grid">
      <div class="card"><h2>Notes</h2>__NOTES__</div>
      <div class="card"><h2>Conditions</h2>__CONDITIONS__<br /><br />__MENTION_TVA__<br />__BANK_HTML__</div>
    </div>

    <footer class="footer">
      <div><strong>__FOOTER_TEXT__</strong><small>__LEGAL_MENTIONS__</small></div>
      <div><small>__COMPANY_EMAIL__<br />__COMPANY_PHONE__<br />__COMPANY_WEBSITE__</small></div>
    </footer>
  </div>
</body>
</html>
"#;
    apply_replacements(template_html.to_string(), common_replacements(invoice, template))
}

fn render_classic(invoice: &InvoiceHeader, template: &InvoiceTemplate) -> String {
    let template_html = r#"<!doctype html>
<html lang="fr">
<head>
  <meta charset="utf-8" />
  <title>__INVOICE_NATURE__ __INVOICE_NUMERO__</title>
  <style>
    @page { size: A4; margin: 15mm; }
    @media print {
      body { print-color-adjust: exact; -webkit-print-color-adjust: exact; }
    }
    body {
      margin: 0;
      color: #111827;
      font-family: "__FONT_FAMILY__", serif;
      font-size: __FONT_SIZE__px;
      background: #fff;
    }
    .page { position: relative; }
    .watermark {
      position: fixed;
      inset: 0;
      display: flex;
      align-items: center;
      justify-content: center;
      color: rgba(31, 41, 55, 0.05);
      font-size: 88px;
      letter-spacing: 0.08em;
      transform: rotate(-28deg);
      pointer-events: none;
    }
    .top {
      display: grid;
      grid-template-columns: 1fr 280px;
      gap: 24px;
      align-items: start;
    }
    .box {
      border: 1px solid #1f2937;
      padding: 14px 16px;
      min-height: 120px;
    }
    .title {
      margin: 18px 0 14px;
      text-align: center;
      font-size: 28px;
      letter-spacing: 0.08em;
    }
    table {
      width: 100%;
      border-collapse: collapse;
      margin-top: 12px;
    }
    th, td {
      border: 1px solid #111827;
      padding: 8px 7px;
    }
    th {
      font-size: 11px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      text-align: left;
    }
    .num { text-align: right; white-space: nowrap; }
    .totals {
      margin-left: auto;
      width: 320px;
      margin-top: 18px;
    }
    .totals-row {
      display: flex;
      justify-content: space-between;
      gap: 16px;
      border-bottom: 1px solid #d1d5db;
      padding: 7px 0;
    }
    .grand {
      margin-top: 10px;
      border-top: 3px double #111827;
      border-bottom: 3px double #111827;
      font-weight: 700;
      font-size: 18px;
      padding: 10px 0;
    }
    .footer {
      margin-top: 36px;
      padding-top: 12px;
      border-top: 1px solid #111827;
      text-align: center;
      line-height: 1.7;
    }
    .grid-2 {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 18px;
      margin-top: 20px;
    }
    tr { page-break-inside: avoid; }
  </style>
</head>
<body>
  <div class="page">
    __WATERMARK_HTML__
    <div class="top">
      <div>
        __COMPANY_LOGO_HTML__
        <div style="margin-top:12px;"><strong>__COMPANY_NAME__</strong><br />__COMPANY_BLOCK__</div>
      </div>
      <div class="box">
        <strong>__INVOICE_NATURE__</strong><br />
        Numero : __INVOICE_NUMERO__<br />
        Date : __INVOICE_DATE__<br />
        Echeance : __INVOICE_ECHEANCE__<br />
        __STATUT_BADGE_HTML__
      </div>
    </div>

    <div class="title">__INVOICE_NATURE__</div>

    <div class="box">
      <strong>Destinataire</strong><br />
      __TIERS_NOM__<br />
      __TIERS_ADRESSE__<br />
      __TIERS_CP_VILLE__<br />
      SIRET : __TIERS_SIRET__<br />
      TVA : __TIERS_TVA__
    </div>

    <table>
      <thead>
        <tr>
          <th>#</th><th>Article</th><th>Libelle</th><th class="num">Qte</th><th>Unite</th>
          <th class="num">PU HT</th><th class="num">Remise</th><th class="num">HT</th><th class="num">TVA</th><th class="num">TTC</th>
        </tr>
      </thead>
      <tbody>__LINES_HTML__</tbody>
    </table>

    <div class="totals">
      <div class="totals-row"><span>Total HT</span><strong>__TOTAL_HT__</strong></div>
      <div class="totals-row"><span>Total TVA</span><strong>__TOTAL_TVA__</strong></div>
      <div class="totals-row grand"><span>Total TTC</span><strong>__TOTAL_TTC__</strong></div>
    </div>

    <div class="grid-2">
      <div><strong>Conditions</strong><br />__CONDITIONS__<br /><br />__MENTION_TVA__</div>
      <div><strong>Notes</strong><br />__NOTES__<br /><br />__BANK_HTML__</div>
    </div>

    <table style="margin-top:20px;">
      <tbody>__TVA_BREAKDOWN_HTML__</tbody>
    </table>

    <div class="footer">__FOOTER_TEXT__<br />__LEGAL_MENTIONS__</div>
  </div>
</body>
</html>
"#;
    apply_replacements(template_html.to_string(), common_replacements(invoice, template))
}

fn render_minimal(invoice: &InvoiceHeader, template: &InvoiceTemplate) -> String {
    let template_html = r#"<!doctype html>
<html lang="fr">
<head>
  <meta charset="utf-8" />
  <title>__INVOICE_NATURE__ __INVOICE_NUMERO__</title>
  <style>
    @page { size: A4; margin: 15mm; }
    @media print {
      body { print-color-adjust: exact; -webkit-print-color-adjust: exact; }
    }
    body {
      margin: 0;
      color: #111827;
      font-family: "__FONT_FAMILY__", sans-serif;
      font-size: __FONT_SIZE__px;
      background: #fff;
    }
    .page { position: relative; }
    .watermark {
      position: fixed;
      inset: 0;
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 120px;
      font-weight: 300;
      color: rgba(156, 163, 175, 0.12);
      text-transform: uppercase;
      pointer-events: none;
    }
    .top {
      display: grid;
      grid-template-columns: 1fr auto;
      align-items: start;
      gap: 16px;
    }
    .brand {
      font-size: 30px;
      font-weight: 300;
      letter-spacing: -0.04em;
    }
    .meta {
      margin-top: 12px;
      line-height: 1.8;
      color: #4b5563;
    }
    .doc-meta {
      text-align: right;
      line-height: 1.8;
    }
    table {
      width: 100%;
      border-collapse: collapse;
      margin-top: 28px;
    }
    th, td {
      padding: 9px 6px;
      border-bottom: 1px solid #e5e7eb;
      text-align: left;
    }
    th {
      font-weight: 600;
      letter-spacing: 0.08em;
      text-transform: uppercase;
      color: #6b7280;
      font-size: 11px;
    }
    .num { text-align: right; white-space: nowrap; }
    .totals {
      margin-left: auto;
      width: 280px;
      margin-top: 26px;
      font-size: 13px;
    }
    .totals-row {
      display: flex;
      justify-content: space-between;
      gap: 16px;
      padding: 6px 0;
    }
    .grand {
      margin-top: 10px;
      padding-top: 12px;
      border-top: 1px solid #111827;
      font-size: 18px;
      font-weight: 700;
    }
    .footer {
      margin-top: 36px;
      font-size: 11px;
      color: #6b7280;
      font-style: italic;
      line-height: 1.8;
    }
    tr { page-break-inside: avoid; }
  </style>
</head>
<body>
  <div class="page">
    __WATERMARK_HTML__
    <div class="top">
      <div>
        <div class="brand">__COMPANY_NAME__</div>
        <div class="meta">__COMPANY_BLOCK__</div>
      </div>
      <div class="doc-meta">
        __COMPANY_LOGO_HTML__
        <strong>__INVOICE_NATURE__</strong><br />
        N° __INVOICE_NUMERO__<br />
        Date : __INVOICE_DATE__<br />
        Echeance : __INVOICE_ECHEANCE__<br />
        __STATUT_BADGE_HTML__
      </div>
    </div>

    <div style="margin-top:24px; line-height:1.8;">
      <strong>__TIERS_NOM__</strong><br />
      __TIERS_ADRESSE__<br />
      __TIERS_CP_VILLE__
    </div>

    <table>
      <thead>
        <tr>
          <th>#</th><th>Article</th><th>Libelle</th><th class="num">Qte</th><th>Unite</th>
          <th class="num">PU HT</th><th class="num">Remise</th><th class="num">HT</th><th class="num">TVA</th><th class="num">TTC</th>
        </tr>
      </thead>
      <tbody>__LINES_HTML__</tbody>
    </table>

    <div class="totals">
      <div class="totals-row"><span>Total HT</span><strong>__TOTAL_HT__</strong></div>
      <div class="totals-row"><span>Total TVA</span><strong>__TOTAL_TVA__</strong></div>
      <div class="totals-row grand"><span>Total TTC</span><strong>__TOTAL_TTC__</strong></div>
    </div>

    <div style="margin-top:22px;">__TVA_BREAKDOWN_HTML__</div>
    <div style="margin-top:22px;"><strong>Conditions</strong><br />__CONDITIONS__</div>
    <div style="margin-top:18px;"><strong>Notes</strong><br />__NOTES__</div>
    <div class="footer">__MENTION_TVA__<br />__BANK_HTML__<br />__FOOTER_TEXT__<br />__LEGAL_MENTIONS__</div>
  </div>
</body>
</html>
"#;
    apply_replacements(template_html.to_string(), common_replacements(invoice, template))
}

fn render_corporate(invoice: &InvoiceHeader, template: &InvoiceTemplate) -> String {
    let template_html = r#"<!doctype html>
<html lang="fr">
<head>
  <meta charset="utf-8" />
  <title>__INVOICE_NATURE__ __INVOICE_NUMERO__</title>
  <style>
    @page { size: A4; margin: 15mm; }
    @media print {
      body { print-color-adjust: exact; -webkit-print-color-adjust: exact; }
    }
    body {
      margin: 0;
      color: #0f172a;
      font-family: "__FONT_FAMILY__", sans-serif;
      font-size: __FONT_SIZE__px;
      background: #fff;
    }
    .page { position: relative; }
    .watermark {
      position: fixed;
      inset: 0;
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 92px;
      font-weight: 800;
      color: rgba(15, 23, 42, 0.05);
      transform: rotate(-30deg);
      pointer-events: none;
    }
    .hero {
      display: grid;
      grid-template-columns: 40% 60%;
      border: 1px solid #dbe2ea;
      border-radius: 26px;
      overflow: hidden;
    }
    .hero-left {
      background: __PRIMARY_COLOR__;
      color: #fff;
      padding: 24px;
      min-height: 220px;
    }
    .hero-right {
      background: #fff;
      padding: 24px;
      display: flex;
      flex-direction: column;
      gap: 16px;
    }
    .hero-left h1 {
      margin: 0 0 10px;
      font-size: 28px;
      letter-spacing: -0.03em;
    }
    .hero-right-top {
      display: flex;
      justify-content: space-between;
      gap: 16px;
      align-items: start;
    }
    .client-box {
      border: 1px solid #dbe2ea;
      border-radius: 18px;
      padding: 16px;
      background: #f8fafc;
    }
    .status-badge {
      display: inline-flex;
      padding: 6px 10px;
      border-radius: 999px;
      background: rgba(15, 143, 124, 0.12);
      color: __PRIMARY_COLOR__;
      font-size: 11px;
      font-weight: 700;
      letter-spacing: 0.08em;
    }
    table {
      width: 100%;
      border-collapse: collapse;
      margin-top: 22px;
      border: 1px solid #dbe2ea;
    }
    thead th {
      background: __PRIMARY_COLOR__;
      color: #fff;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      font-size: 11px;
      padding: 10px 8px;
      text-align: left;
    }
    td {
      padding: 9px 8px;
      border-bottom: 1px solid #e5e7eb;
    }
    .num { text-align: right; white-space: nowrap; }
    .subtotals {
      background: rgba(29, 232, 200, 0.12);
      font-weight: 700;
    }
    .totals-band {
      margin-top: 20px;
      display: grid;
      grid-template-columns: 1fr 320px;
      gap: 20px;
      align-items: start;
    }
    .grand-total {
      background: __PRIMARY_COLOR__;
      color: #fff;
      border-radius: 18px;
      padding: 18px 20px;
      box-shadow: 0 18px 36px rgba(15, 143, 124, 0.16);
    }
    .grand-total .line {
      display: flex;
      justify-content: space-between;
      gap: 16px;
      padding: 7px 0;
    }
    .grand-total .line.total {
      font-size: 24px;
      font-weight: 800;
      border-top: 1px solid rgba(255,255,255,0.24);
      margin-top: 10px;
      padding-top: 12px;
    }
    .footer {
      margin-top: 26px;
      padding-top: 16px;
      border-top: 1px solid #dbe2ea;
      display: flex;
      justify-content: space-between;
      gap: 20px;
      align-items: center;
      font-size: 11px;
      color: #475467;
    }
    .page-no::after { content: "1"; }
    tr { page-break-inside: avoid; }
  </style>
</head>
<body>
  <div class="page">
    __WATERMARK_HTML__
    <section class="hero">
      <div class="hero-left">
        __COMPANY_LOGO_HTML__
        <h1>__COMPANY_NAME__</h1>
        <div style="line-height:1.8;">__COMPANY_BLOCK__</div>
      </div>
      <div class="hero-right">
        <div class="hero-right-top">
          <div>
            <strong style="font-size:24px;">__INVOICE_NATURE__</strong><br />
            N° __INVOICE_NUMERO__<br />
            Date : __INVOICE_DATE__<br />
            Echeance : __INVOICE_ECHEANCE__
          </div>
          <div>__STATUT_BADGE_HTML__</div>
        </div>
        <div class="client-box">
          <strong>Client</strong><br />
          __TIERS_NOM__<br />
          __TIERS_ADRESSE__<br />
          __TIERS_CP_VILLE__<br />
          SIRET : __TIERS_SIRET__<br />
          TVA : __TIERS_TVA__
        </div>
      </div>
    </section>

    <table>
      <thead>
        <tr>
          <th>#</th><th>Article</th><th>Libelle</th><th class="num">Qte</th><th>Unite</th>
          <th class="num">PU HT</th><th class="num">Remise</th><th class="num">HT</th><th class="num">TVA</th><th class="num">TTC</th>
        </tr>
      </thead>
      <tbody>__LINES_HTML__</tbody>
      <tbody>
        <tr class="subtotals"><td colspan="7">Sous-totaux</td><td class="num">__TOTAL_HT__</td><td class="num">__TOTAL_TVA__</td><td class="num">__TOTAL_TTC__</td></tr>
      </tbody>
    </table>

    <div class="totals-band">
      <div>
        <div><strong>Detail TVA</strong></div>
        <table style="margin-top:10px;"><tbody>__TVA_BREAKDOWN_HTML__</tbody></table>
        <div style="margin-top:18px;"><strong>Notes</strong><br />__NOTES__</div>
        <div style="margin-top:14px;"><strong>Conditions</strong><br />__CONDITIONS__<br />__MENTION_TVA__<br />__BANK_HTML__</div>
      </div>
      <div class="grand-total">
        <div class="line"><span>Total HT</span><strong>__TOTAL_HT__</strong></div>
        <div class="line"><span>Total TVA</span><strong>__TOTAL_TVA__</strong></div>
        <div class="line total"><span>Total TTC</span><strong>__TOTAL_TTC__</strong></div>
      </div>
    </div>

    <footer class="footer">
      <div style="display:flex; align-items:center; gap:12px;">__COMPANY_LOGO_HTML__<div>__FOOTER_TEXT__<br />__LEGAL_MENTIONS__</div></div>
      <div style="text-align:right;">__COMPANY_EMAIL__<br />__COMPANY_PHONE__<br />Page <span class="page-no"></span></div>
    </footer>
  </div>
</body>
</html>
"#;
    apply_replacements(template_html.to_string(), common_replacements(invoice, template))
}

pub(crate) fn render_invoice_html_with_template(
    invoice: &InvoiceHeader,
    template: &InvoiceTemplate,
) -> String {
    match template.layout.as_str() {
        "classic" => render_classic(invoice, template),
        "minimal" => render_minimal(invoice, template),
        "corporate" => render_corporate(invoice, template),
        _ => render_modern(invoice, template),
    }
}

#[tauri::command]
pub fn render_invoice_html(
    state: State<'_, AppState>,
    invoice: InvoiceHeader,
    template_id: String,
) -> Result<String, String> {
    let template = resolve_template(&state, &template_id)?;
    Ok(render_invoice_html_with_template(&invoice, &template))
}

#[tauri::command]
pub fn render_invoice_html_preview(
    invoice: InvoiceHeader,
    template: InvoiceTemplate,
) -> Result<String, String> {
    Ok(render_invoice_html_with_template(&invoice, &template))
}

#[tauri::command]
pub fn get_invoice_templates(
    state: State<'_, AppState>,
    connection_id: Option<String>,
) -> Result<Vec<InvoiceTemplate>, String> {
    let config = state.config.lock().map_err(|error| error.to_string())?;
    Ok(merge_templates(
        &config.invoice_templates,
        connection_id.as_deref(),
    ))
}

#[tauri::command]
pub fn save_invoice_template(
    state: State<'_, AppState>,
    connection_id: String,
    mut template: InvoiceTemplate,
) -> Result<InvoiceTemplate, String> {
    let mut config = state.config.lock().map_err(|error| error.to_string())?;
    template.connection_id = connection_id.clone();
    if template.id.trim().is_empty() || template.id.starts_with("builtin-") {
        template.id = format!("template-{}", Uuid::new_v4());
    }
    if template.name.trim().is_empty() {
        return Err("Template name is required".to_string());
    }

    let template_count = config
        .invoice_templates
        .iter()
        .filter(|existing| existing.connection_id == connection_id && existing.id != template.id)
        .count();
    if !config.invoice_templates.iter().any(|existing| existing.id == template.id) && template_count >= 5 {
        return Err("Only 5 saved invoice templates are allowed per connection".to_string());
    }

    if let Some(existing) = config
        .invoice_templates
        .iter_mut()
        .find(|existing| existing.id == template.id)
    {
        *existing = template.clone();
    } else {
        config.invoice_templates.push(template.clone());
    }
    config.active_template_id = template.id.clone();
    drop(config);
    state.save_config()?;
    Ok(template)
}

#[tauri::command]
pub fn export_invoice_pdf(
    app: AppHandle,
    state: State<'_, AppState>,
    invoice: InvoiceHeader,
    template_id: String,
    file_path: String,
) -> Result<String, String> {
    let template = resolve_template(&state, &template_id)?;
    let html = render_invoice_html_with_template(&invoice, &template);
    let file_name = PathBuf::from(&file_path)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("invoice-{}", invoice.numero));
    let temp_path = std::env::temp_dir().join(format!("{}.html", file_name));
    std::fs::write(&temp_path, html).map_err(|error| error.to_string())?;

    let target = temp_path.to_string_lossy().to_string();
    let shell_scope = app.shell_scope();
    tauri::api::shell::open(&shell_scope, target.clone(), None).map_err(|error| error.to_string())?;
    Ok(target)
}
