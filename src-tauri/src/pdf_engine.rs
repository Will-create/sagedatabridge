use std::collections::BTreeMap;
use std::path::PathBuf;

use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::invoice_commands::InvoiceHeader;
use crate::state::{AppState, InvoiceTemplate};

fn default_company_name() -> String {
    "Sage Data Bridge".to_string()
}

fn builtin_templates() -> Vec<InvoiceTemplate> {
    let presets = [
        (
            "builtin-modern-clean",
            "Modern Clean",
            "#1f4f46",
            "#3b82f6",
            "modern-clean",
        ),
        (
            "builtin-modern-bold",
            "Modern Bold",
            "#172554",
            "#0f766e",
            "modern-bold",
        ),
        (
            "builtin-modern-soft",
            "Modern Soft",
            "#334155",
            "#14b8a6",
            "modern-soft",
        ),
        (
            "builtin-modern-borderless",
            "Modern Borderless",
            "#111827",
            "#64748b",
            "modern-borderless",
        ),
        (
            "builtin-modern-accent",
            "Modern Accent",
            "#7c2d12",
            "#0f766e",
            "modern-accent",
        ),
        (
            "builtin-modern-watermark",
            "Modern Watermark",
            "#1e293b",
            "#2563eb",
            "modern-watermark",
        ),
        (
            "builtin-modern-compact",
            "Modern Compact",
            "#0f172a",
            "#059669",
            "modern-compact",
        ),
        (
            "builtin-modern-premium",
            "Modern Premium",
            "#312e81",
            "#b45309",
            "modern-premium",
        ),
    ];

    presets
        .into_iter()
        .map(|(id, name, primary, secondary, layout)| InvoiceTemplate {
            id: id.to_string(),
            connection_id: String::new(),
            name: name.to_string(),
            primary_color: primary.to_string(),
            secondary_color: secondary.to_string(),
            font_family: "Helvetica".to_string(),
            font_size_px: if layout == "modern-compact" { 7 } else { 8 },
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
            layout: layout.to_string(),
        })
        .collect()
}

fn merge_templates(
    configured: &[InvoiceTemplate],
    connection_id: Option<&str>,
) -> Vec<InvoiceTemplate> {
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
        .iter()
        .find(|template| template.id == target_id)
        .cloned()
        .or_else(|| {
            all_templates
                .iter()
                .find(|template| template.id == "builtin-modern-clean")
                .cloned()
        })
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

fn format_currency(value: f64, devise: &str, locale: &str) -> String {
    let currency = if devise.trim().is_empty() {
        "XOF"
    } else {
        devise.trim()
    };
    let decimals = if currency.eq_ignore_ascii_case("XOF") {
        0
    } else {
        2
    };
    let rounded = (value * 100.0).round() / 100.0;
    let negative = rounded.is_sign_negative();
    let abs = rounded.abs();
    let major = abs.trunc() as i64;
    let minor = ((abs - major as f64) * 100.0).round() as i64;
    let thousands_sep = if locale.starts_with("fr") { " " } else { "," };
    let decimal_sep = if locale.starts_with("fr") { "," } else { "." };
    let reversed = major.to_string().chars().rev().collect::<Vec<_>>();
    let grouped = reversed
        .chunks(3)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(thousands_sep)
        .chars()
        .rev()
        .collect::<String>();
    if decimals == 0 {
        format!(
            "{}{} {}",
            if negative { "-" } else { "" },
            grouped,
            currency
        )
    } else {
        format!(
            "{}{}{}{:02} {}",
            if negative { "-" } else { "" },
            grouped,
            decimal_sep,
            minor,
            currency
        )
    }
}

fn format_number_fr(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    format!("{:.2}", rounded).replace('.', ",")
}

fn format_amount(value: f64, devise: &str, locale: &str) -> String {
    let decimals = if devise.trim().eq_ignore_ascii_case("XOF") {
        0
    } else {
        2
    };
    let rounded = if decimals == 0 {
        value.round()
    } else {
        (value * 100.0).round() / 100.0
    };
    let negative = rounded.is_sign_negative();
    let abs = rounded.abs();
    let major = abs.trunc() as i64;
    let minor = ((abs - major as f64) * 100.0).round() as i64;
    let thousands_sep = if locale.starts_with("fr") { " " } else { "," };
    let decimal_sep = if locale.starts_with("fr") { "," } else { "." };
    let reversed = major.to_string().chars().rev().collect::<Vec<_>>();
    let grouped = reversed
        .chunks(3)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(thousands_sep)
        .chars()
        .rev()
        .collect::<String>();
    if decimals == 0 {
        format!("{}{}", if negative { "-" } else { "" }, grouped)
    } else {
        format!(
            "{}{}{}{:02}",
            if negative { "-" } else { "" },
            grouped,
            decimal_sep,
            minor
        )
    }
}

fn format_plain_number(value: f64, locale: &str) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if locale.starts_with("fr") {
        format!("{:.2}", rounded).replace('.', ",")
    } else {
        format!("{:.2}", rounded)
    }
}

fn pdf_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
        .replace('\r', " ")
        .replace('\n', " ")
}

fn pdf_amount(value: f64, devise: &str, locale: &str) -> String {
    format_amount(value, devise, locale)
}

fn push_pdf_text(content: &mut String, x: f64, y: f64, size: f64, text: &str) {
    content.push_str(&format!(
        "BT /F1 {} Tf 1 0 0 1 {:.2} {:.2} Tm ({}) Tj ET\n",
        size,
        x,
        y,
        pdf_escape(text)
    ));
}

fn push_pdf_line(content: &mut String, x1: f64, y1: f64, x2: f64, y2: f64) {
    content.push_str(&format!("{:.2} {:.2} m {:.2} {:.2} l S\n", x1, y1, x2, y2));
}

fn new_pdf_page_header(invoice: &InvoiceHeader, page_no: usize, page_count_hint: usize) -> String {
    let mut content = String::new();
    content.push_str("0.2 w\n");
    push_pdf_text(
        &mut content,
        40.0,
        805.0,
        18.0,
        &invoice_title(&invoice.nature),
    );
    push_pdf_text(
        &mut content,
        360.0,
        807.0,
        10.0,
        &format!("{}  |  {}", invoice.numero, invoice.date),
    );
    push_pdf_line(&mut content, 40.0, 790.0, 555.0, 790.0);
    if page_no == 1 {
        push_pdf_text(
            &mut content,
            40.0,
            765.0,
            11.0,
            &format!("Client: {}", invoice.tiers_nom),
        );
        push_pdf_text(&mut content, 40.0, 750.0, 9.0, &invoice.tiers_adresse);
        push_pdf_text(
            &mut content,
            40.0,
            737.0,
            9.0,
            &[invoice.tiers_cp.clone(), invoice.tiers_ville.clone()]
                .into_iter()
                .filter(|value| !value.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" "),
        );
        push_pdf_text(
            &mut content,
            360.0,
            765.0,
            9.0,
            &format!("Reference: {}", invoice.reference),
        );
        push_pdf_text(
            &mut content,
            360.0,
            750.0,
            9.0,
            &format!("Echeance: {}", invoice.date_echeance),
        );
    }
    push_pdf_text(
        &mut content,
        455.0,
        25.0,
        8.0,
        &format!("Page {} / {}", page_no, page_count_hint),
    );
    content
}

fn render_pdf_table_header(content: &mut String, y: f64, devise: &str) {
    push_pdf_line(content, 40.0, y + 12.0, 555.0, y + 12.0);
    push_pdf_text(content, 42.0, y, 7.0, "#");
    push_pdf_text(content, 62.0, y, 7.0, "Article");
    push_pdf_text(content, 120.0, y, 7.0, "Libelle");
    push_pdf_text(content, 270.0, y, 7.0, "Qte/Un");
    push_pdf_text(content, 305.0, y, 7.0, &format!("PU {}", devise));
    push_pdf_text(content, 360.0, y, 7.0, &format!("HT {}", devise));
    push_pdf_text(content, 420.0, y, 7.0, &format!("TVA {}", devise));
    push_pdf_text(content, 475.0, y, 7.0, &format!("BIC {}", devise));
    push_pdf_text(content, 520.0, y, 7.0, &format!("TTC {}", devise));
    push_pdf_line(content, 40.0, y - 5.0, 555.0, y - 5.0);
}

fn generate_invoice_pdf_bytes(invoice: &InvoiceHeader, locale: &str) -> Vec<u8> {
    let mut pages = Vec::<String>::new();
    let mut page_no = 1;
    let mut content = new_pdf_page_header(invoice, page_no, 999);
    let mut y = if page_no == 1 { 700.0 } else { 755.0 };
    render_pdf_table_header(&mut content, y, &invoice.devise);
    y -= 20.0;

    for line in &invoice.lignes {
        if y < 95.0 {
            pages.push(content);
            page_no += 1;
            content = new_pdf_page_header(invoice, page_no, 999);
            y = 755.0;
            render_pdf_table_header(&mut content, y, &invoice.devise);
            y -= 20.0;
        }
        let label = if line.libelle.chars().count() > 32 {
            format!("{}...", line.libelle.chars().take(29).collect::<String>())
        } else {
            line.libelle.clone()
        };
        push_pdf_text(&mut content, 42.0, y, 7.0, &line.ordre.to_string());
        push_pdf_text(&mut content, 62.0, y, 7.0, &line.article_code);
        push_pdf_text(&mut content, 120.0, y, 7.0, &label);
        push_pdf_text(
            &mut content,
            270.0,
            y,
            7.0,
            &format_plain_number(line.quantite, locale),
        );
        push_pdf_text(
            &mut content,
            305.0,
            y,
            7.0,
            &pdf_amount(line.prix_ht, &invoice.devise, locale),
        );
        push_pdf_text(
            &mut content,
            360.0,
            y,
            7.0,
            &pdf_amount(line.montant_ht, &invoice.devise, locale),
        );
        push_pdf_text(
            &mut content,
            420.0,
            y,
            7.0,
            &pdf_amount(line.montant_tva, &invoice.devise, locale),
        );
        push_pdf_text(
            &mut content,
            475.0,
            y,
            7.0,
            &pdf_amount(line.montant_bic, &invoice.devise, locale),
        );
        push_pdf_text(
            &mut content,
            520.0,
            y,
            7.0,
            &pdf_amount(line.montant_ttc, &invoice.devise, locale),
        );
        y -= 16.0;
    }

    if y < 170.0 {
        pages.push(content);
        page_no += 1;
        content = new_pdf_page_header(invoice, page_no, 999);
        y = 755.0;
    }
    push_pdf_line(&mut content, 340.0, y, 555.0, y);
    y -= 18.0;
    push_pdf_text(
        &mut content,
        360.0,
        y,
        9.0,
        &format!(
            "{} ({})",
            doc_label(locale, "line_discounts"),
            invoice.devise
        ),
    );
    push_pdf_text(
        &mut content,
        475.0,
        y,
        9.0,
        &pdf_amount(
            invoice_line_discount_total(invoice),
            &invoice.devise,
            locale,
        ),
    );
    y -= 16.0;
    push_pdf_text(
        &mut content,
        360.0,
        y,
        9.0,
        &format!(
            "{} ({})",
            doc_label(locale, "global_discount"),
            invoice.devise
        ),
    );
    push_pdf_text(
        &mut content,
        475.0,
        y,
        9.0,
        &pdf_amount(
            invoice_global_discount_total(invoice),
            &invoice.devise,
            locale,
        ),
    );
    y -= 16.0;
    push_pdf_text(
        &mut content,
        360.0,
        y,
        9.0,
        &format!("{} ({})", doc_label(locale, "total_ht"), invoice.devise),
    );
    push_pdf_text(
        &mut content,
        475.0,
        y,
        9.0,
        &pdf_amount(invoice.total_ht, &invoice.devise, locale),
    );
    y -= 16.0;
    push_pdf_text(
        &mut content,
        360.0,
        y,
        9.0,
        &format!("{} ({})", doc_label(locale, "total_vat"), invoice.devise),
    );
    push_pdf_text(
        &mut content,
        475.0,
        y,
        9.0,
        &pdf_amount(invoice.total_tva, &invoice.devise, locale),
    );
    y -= 16.0;
    push_pdf_text(
        &mut content,
        360.0,
        y,
        9.0,
        &format!("{} ({})", doc_label(locale, "total_bic"), invoice.devise),
    );
    push_pdf_text(
        &mut content,
        475.0,
        y,
        9.0,
        &pdf_amount(invoice.total_bic, &invoice.devise, locale),
    );
    y -= 22.0;
    push_pdf_text(
        &mut content,
        360.0,
        y,
        11.0,
        &format!("{} ({})", doc_label(locale, "total_ttc"), invoice.devise),
    );
    push_pdf_text(
        &mut content,
        475.0,
        y,
        11.0,
        &pdf_amount(invoice.total_ttc, &invoice.devise, locale),
    );
    pages.push(content);

    let page_count = pages.len();
    let pages = pages
        .into_iter()
        .enumerate()
        .map(|(index, page)| {
            page.replace(
                &format!("Page {} / 999", index + 1),
                &format!("Page {} / {}", index + 1, page_count),
            )
        })
        .collect::<Vec<_>>();

    let mut objects = Vec::<String>::new();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());
    let kids = (0..page_count)
        .map(|index| format!("{} 0 R", 3 + index * 2))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids, page_count
    ));
    for (index, page) in pages.iter().enumerate() {
        let page_obj = 3 + index * 2;
        let content_obj = page_obj + 1;
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >> /Contents {} 0 R >>",
            content_obj
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{}endstream",
            page.as_bytes().len(),
            page
        ));
    }

    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.as_bytes().len());
        pdf.push_str(&format!("{} 0 obj\n{}\nendobj\n", index + 1, object));
    }
    let xref_offset = pdf.as_bytes().len();
    pdf.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        pdf.push_str(&format!("{:010} 00000 n \n", offset));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        objects.len() + 1,
        xref_offset
    ));
    pdf.into_bytes()
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

fn doc_label(locale: &str, key: &str) -> &'static str {
    let fr = locale.starts_with("fr");
    match (fr, key) {
        (false, "client") => "Customer",
        (false, "document") => "Document",
        (false, "date") => "Date",
        (false, "due") => "Due date",
        (false, "item") => "Item",
        (false, "label") => "Description",
        (false, "qty") => "Qty",
        (false, "unit") => "Unit",
        (false, "unit_price") => "Unit price",
        (false, "discount") => "Discount",
        (false, "subtotal") => "Subtotal",
        (false, "line_discounts") => "Line discounts",
        (false, "global_discount") => "Global discount",
        (false, "vat_detail") => "VAT detail",
        (false, "total_ht") => "Total excl. tax",
        (false, "total_vat") => "VAT total",
        (false, "total_bic") => "BIC total",
        (false, "total_ttc") => "Grand total",
        (false, "notes") => "Notes",
        (false, "terms") => "Payment terms",
        (_, "client") => "Client",
        (_, "document") => "Document",
        (_, "date") => "Date",
        (_, "due") => "Echeance",
        (_, "item") => "Article",
        (_, "label") => "Libelle",
        (_, "qty") => "Qte",
        (_, "unit") => "Unite",
        (_, "unit_price") => "PU HT",
        (_, "discount") => "Remise",
        (_, "subtotal") => "Sous-total",
        (_, "line_discounts") => "Remises lignes",
        (_, "global_discount") => "Remise globale",
        (_, "vat_detail") => "Detail TVA",
        (_, "total_ht") => "Total HT",
        (_, "total_vat") => "Total TVA",
        (_, "total_bic") => "Total BIC",
        (_, "total_ttc") => "Total TTC",
        (_, "notes") => "Notes",
        (_, "terms") => "Conditions",
        _ => "",
    }
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

fn lines_html(invoice: &InvoiceHeader, locale: &str) -> String {
    invoice
        .lignes
        .iter()
        .map(|line| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}%</td><td class=\"num\">{}</td><td class=\"num\">{}%</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
                line.ordre,
                escape_html(&line.article_code),
                escape_html(&line.libelle),
                format_plain_number(line.quantite, locale),
                format_amount(line.prix_ht, &invoice.devise, locale),
                if line.remise_type == "fixed" {
                    format_amount(line.remise_valeur, &invoice.devise, locale)
                } else {
                    format!("{}%", format_number_fr(line.remise_pct))
                },
                format_amount(line.montant_ht, &invoice.devise, locale),
                format_number_fr(line.taux_tva),
                format_amount(line.montant_tva, &invoice.devise, locale),
                format_number_fr(line.taux_bic),
                format_amount(line.montant_bic, &invoice.devise, locale),
                format_amount(line.montant_ttc, &invoice.devise, locale),
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn tva_breakdown_html(invoice: &InvoiceHeader, locale: &str) -> String {
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
                format_amount(base, &invoice.devise, locale),
                format_amount(tax, &invoice.devise, locale),
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn invoice_line_discount_total(invoice: &InvoiceHeader) -> f64 {
    invoice
        .lignes
        .iter()
        .map(|line| {
            let gross = line.quantite * line.prix_ht;
            (gross - line.montant_ht).max(0.0)
        })
        .sum::<f64>()
}

fn invoice_global_discount_total(invoice: &InvoiceHeader) -> f64 {
    let net_lines = invoice
        .lignes
        .iter()
        .map(|line| line.montant_ht)
        .sum::<f64>();
    (net_lines - invoice.total_ht).max(0.0)
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

fn common_replacements(
    invoice: &InvoiceHeader,
    template: &InvoiceTemplate,
    locale: &str,
) -> Vec<(&'static str, String)> {
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
        (
            "__COMPANY_WEBSITE__",
            escape_html(&template.company_website),
        ),
        ("__COMPANY_BLOCK__", company_block(template)),
        ("__COMPANY_LOGO_HTML__", logo_html(template)),
        ("__INVOICE_NUMERO__", escape_html(&invoice.numero)),
        ("__INVOICE_DATE__", escape_html(&invoice.date)),
        ("__INVOICE_ECHEANCE__", escape_html(&invoice.date_echeance)),
        ("__INVOICE_DEVISE__", escape_html(&invoice.devise)),
        (
            "__INVOICE_NATURE__",
            invoice_title(&invoice.nature).to_string(),
        ),
        ("__TIERS_NOM__", escape_html(&invoice.tiers_nom)),
        ("__TIERS_ADRESSE__", nl2br(&invoice.tiers_adresse)),
        ("__TIERS_CP_VILLE__", escape_html(&tiers_cp_ville(invoice))),
        ("__TIERS_SIRET__", escape_html(&invoice.tiers_siret)),
        ("__TIERS_TVA__", escape_html(&invoice.tiers_tva_intra)),
        ("__LINES_HTML__", lines_html(invoice, locale)),
        (
            "__TOTAL_HT__",
            format_amount(invoice.total_ht, &invoice.devise, locale),
        ),
        (
            "__TOTAL_LINE_DISCOUNT__",
            format_amount(
                invoice_line_discount_total(invoice),
                &invoice.devise,
                locale,
            ),
        ),
        (
            "__TOTAL_GLOBAL_DISCOUNT__",
            format_amount(
                invoice_global_discount_total(invoice),
                &invoice.devise,
                locale,
            ),
        ),
        (
            "__TOTAL_TVA__",
            format_amount(invoice.total_tva, &invoice.devise, locale),
        ),
        (
            "__TOTAL_BIC__",
            format_amount(invoice.total_bic, &invoice.devise, locale),
        ),
        (
            "__TOTAL_TTC__",
            format_amount(invoice.total_ttc, &invoice.devise, locale),
        ),
        (
            "__TVA_BREAKDOWN_HTML__",
            tva_breakdown_html(invoice, locale),
        ),
        ("__NOTES__", nl2br(&invoice.notes)),
        ("__CONDITIONS__", nl2br(&conditions)),
        ("__FOOTER_TEXT__", nl2br(&template.footer_text)),
        ("__LEGAL_MENTIONS__", nl2br(&template.legal_mentions)),
        ("__BANK_HTML__", bank_html(template)),
        ("__MENTION_TVA__", nl2br(&template.mention_tva)),
        (
            "__HTML_LANG__",
            if locale.starts_with("fr") { "fr" } else { "en" }.to_string(),
        ),
        ("__STATUT_BADGE_HTML__", statut_badge_html(invoice)),
        ("__WATERMARK_HTML__", watermark_html(invoice)),
        ("__LABEL_CLIENT__", doc_label(locale, "client").to_string()),
        (
            "__LABEL_DOCUMENT__",
            doc_label(locale, "document").to_string(),
        ),
        ("__LABEL_DATE__", doc_label(locale, "date").to_string()),
        ("__LABEL_DUE__", doc_label(locale, "due").to_string()),
        ("__LABEL_ITEM__", doc_label(locale, "item").to_string()),
        ("__LABEL_LABEL__", doc_label(locale, "label").to_string()),
        ("__LABEL_QTY__", doc_label(locale, "qty").to_string()),
        ("__LABEL_UNIT__", doc_label(locale, "unit").to_string()),
        (
            "__LABEL_UNIT_PRICE__",
            format!("{} ({})", doc_label(locale, "unit_price"), invoice.devise),
        ),
        (
            "__LABEL_DISCOUNT__",
            doc_label(locale, "discount").to_string(),
        ),
        (
            "__LABEL_SUBTOTAL__",
            doc_label(locale, "subtotal").to_string(),
        ),
        (
            "__LABEL_LINE_DISCOUNTS__",
            format!(
                "{} ({})",
                doc_label(locale, "line_discounts"),
                invoice.devise
            ),
        ),
        (
            "__LABEL_GLOBAL_DISCOUNT__",
            format!(
                "{} ({})",
                doc_label(locale, "global_discount"),
                invoice.devise
            ),
        ),
        (
            "__LABEL_VAT_DETAIL__",
            doc_label(locale, "vat_detail").to_string(),
        ),
        (
            "__LABEL_TOTAL_HT__",
            format!("{} ({})", doc_label(locale, "total_ht"), invoice.devise),
        ),
        (
            "__LABEL_TOTAL_VAT__",
            format!("{} ({})", doc_label(locale, "total_vat"), invoice.devise),
        ),
        (
            "__LABEL_TOTAL_BIC__",
            format!("{} ({})", doc_label(locale, "total_bic"), invoice.devise),
        ),
        (
            "__LABEL_TOTAL_TTC__",
            format!("{} ({})", doc_label(locale, "total_ttc"), invoice.devise),
        ),
        ("__LABEL_NOTES__", doc_label(locale, "notes").to_string()),
        ("__LABEL_TERMS__", doc_label(locale, "terms").to_string()),
    ]
}

fn apply_replacements(mut html: String, replacements: Vec<(&'static str, String)>) -> String {
    for (key, value) in replacements {
        html = html.replace(key, &value);
    }
    html
}

fn render_modern(invoice: &InvoiceHeader, template: &InvoiceTemplate, locale: &str) -> String {
    let template_html = r#"<!doctype html>
<html lang="__HTML_LANG__">
<head>
  <meta charset="utf-8" />
  <title>__INVOICE_NATURE__ __INVOICE_NUMERO__</title>
  <style>
    @page { size: A4; margin: 12mm; }
    @media print {
      body { print-color-adjust: exact; -webkit-print-color-adjust: exact; background: #fff; }
      .invoice { width: auto; min-height: auto; margin: 0; padding: 0; box-shadow: none; }
    }
    * { box-sizing: border-box; }
    html, body { width: 100%; margin: 0; padding: 0; }
    body {
      color: #101828;
      font-family: "__FONT_FAMILY__", sans-serif;
      font-size: __FONT_SIZE__px;
      background: #d8e0e7;
    }
    .invoice {
      position: relative;
      width: 210mm;
      min-height: 297mm;
      max-width: 100%;
      margin: 0 auto;
      padding: 12mm;
      background: #fff;
      display: flex;
      flex-direction: column;
      overflow: hidden;
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
      padding: 14px 16px 12px;
      border-radius: 24px 24px 14px 14px;
      display: grid;
      grid-template-columns: 1fr auto;
      gap: 20px;
      align-items: start;
    }
    .hero h1 {
      margin: 0 0 8px;
      font-size: 20px;
      letter-spacing: -0.03em;
    }
    .hero-meta {
      font-size: 8px;
      line-height: 1.35;
      opacity: 0.92;
    }
    .status-badge {
      display: inline-flex;
      margin-top: 12px;
      padding: 4px 7px;
      border-radius: 999px;
      background: rgba(255,255,255,0.18);
      font-size: 7px;
      font-weight: 700;
      letter-spacing: 0.08em;
    }
    .header-grid {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 10px;
      margin: 10px 0 12px;
    }
    .card {
      background: #f8fafc;
      border: 1px solid #e2e8f0;
      border-radius: 16px;
      padding: 10px;
    }
    .card h2 {
      margin: 0 0 10px;
      font-size: 8px;
      letter-spacing: 0.1em;
      text-transform: uppercase;
      color: __PRIMARY_COLOR__;
    }
    table {
      width: 100%;
      max-width: 100%;
      table-layout: fixed;
      border-collapse: collapse;
      margin-top: 10px;
    }
    thead th {
      background: __SECONDARY_COLOR__;
      color: #fff;
      font-size: 7px;
      text-transform: uppercase;
      letter-spacing: 0.08em;
      padding: 5px 4px;
      text-align: left;
    }
    tbody tr:nth-child(even) td { background: #f9fafb; }
    td {
      padding: 4px 4px;
      border-bottom: 1px solid #e5e7eb;
      vertical-align: top;
      overflow-wrap: anywhere;
      word-break: normal;
    }
    .num { text-align: right; white-space: normal; overflow-wrap: anywhere; }
    .totals-wrap {
      display: grid;
      grid-template-columns: minmax(0, 1fr) minmax(180px, 62mm);
      gap: 10px;
      margin-top: 12px;
      align-items: start;
      page-break-inside: avoid;
      max-width: 100%;
    }
    .totals {
      margin-left: auto;
      border-left: 5px solid __PRIMARY_COLOR__;
      background: #f8fafc;
      border-radius: 12px;
      padding: 8px 10px;
      width: 100%;
    }
    .totals-row {
      display: flex;
      justify-content: space-between;
      gap: 16px;
      padding: 3px 0;
      overflow-wrap: anywhere;
    }
    .grand-total {
      margin-top: 10px;
      padding-top: 12px;
      border-top: 2px solid #cbd5e1;
      font-size: 12px;
      font-weight: 800;
      color: __PRIMARY_COLOR__;
    }
    .notes-grid {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: 10px;
      margin-top: 12px;
    }
    .footer {
      margin-top: 14px;
      background: __PRIMARY_COLOR__;
      color: #fff;
      border-radius: 14px;
      padding: 8px 10px;
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
    @media (max-width: 720px) {
      .header-grid, .notes-grid, .totals-wrap { grid-template-columns: 1fr; }
    }
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
        <h2>__LABEL_CLIENT__</h2>
        <strong>__TIERS_NOM__</strong><br />
        __TIERS_ADRESSE__<br />
        __TIERS_CP_VILLE__<br />
        SIRET : __TIERS_SIRET__<br />
        TVA : __TIERS_TVA__
      </div>
      <div class="card">
        <h2>__LABEL_DOCUMENT__</h2>
        <strong>__INVOICE_NATURE__ N° __INVOICE_NUMERO__</strong><br />
        __LABEL_DATE__ : __INVOICE_DATE__<br />
        __LABEL_DUE__ : __INVOICE_ECHEANCE__<br />
        SIRET : __COMPANY_SIRET__<br />
        TVA intra : __COMPANY_TVA__
      </div>
    </div>

    <table>
      <colgroup>
        <col style="width:4%;" />
        <col style="width:10%;" />
        <col style="width:24%;" />
        <col style="width:7%;" />
        <col style="width:9%;" />
        <col style="width:8%;" />
        <col style="width:9%;" />
        <col style="width:6%;" />
        <col style="width:8%;" />
        <col style="width:6%;" />
        <col style="width:8%;" />
        <col style="width:11%;" />
      </colgroup>
      <thead>
        <tr>
          <th>#</th><th>__LABEL_ITEM__</th><th>__LABEL_LABEL__</th><th class="num">__LABEL_QTY__ / __LABEL_UNIT__</th>
          <th class="num">__LABEL_UNIT_PRICE__</th><th class="num">__LABEL_DISCOUNT__</th><th class="num">HT (__INVOICE_DEVISE__)</th><th class="num">TVA</th><th class="num">Mt TVA (__INVOICE_DEVISE__)</th><th class="num">BIC</th><th class="num">Mt BIC (__INVOICE_DEVISE__)</th><th class="num">TTC (__INVOICE_DEVISE__)</th>
        </tr>
      </thead>
      <tbody>__LINES_HTML__</tbody>
    </table>

    <div class="totals-wrap">
      <div class="card">
        <h2>__LABEL_VAT_DETAIL__</h2>
        <table>
          <tbody>__TVA_BREAKDOWN_HTML__</tbody>
        </table>
      </div>
      <div class="totals">
        <div class="totals-row"><span>__LABEL_TOTAL_HT__</span><strong>__TOTAL_HT__</strong></div>
        <div class="totals-row"><span>__LABEL_LINE_DISCOUNTS__</span><strong>__TOTAL_LINE_DISCOUNT__</strong></div>
        <div class="totals-row"><span>__LABEL_GLOBAL_DISCOUNT__</span><strong>__TOTAL_GLOBAL_DISCOUNT__</strong></div>
        <div class="totals-row"><span>__LABEL_TOTAL_VAT__</span><strong>__TOTAL_TVA__</strong></div>
        <div class="totals-row"><span>__LABEL_TOTAL_BIC__</span><strong>__TOTAL_BIC__</strong></div>
        <div class="totals-row grand-total"><span>__LABEL_TOTAL_TTC__</span><strong>__TOTAL_TTC__</strong></div>
      </div>
    </div>

    <div class="notes-grid">
      <div class="card"><h2>__LABEL_NOTES__</h2>__NOTES__</div>
      <div class="card"><h2>__LABEL_TERMS__</h2>__CONDITIONS__<br /><br />__MENTION_TVA__<br />__BANK_HTML__</div>
    </div>

    <footer class="footer">
      <div><strong>__FOOTER_TEXT__</strong><small>__LEGAL_MENTIONS__</small></div>
      <div><small>__COMPANY_EMAIL__<br />__COMPANY_PHONE__<br />__COMPANY_WEBSITE__</small></div>
    </footer>
  </div>
</body>
</html>
"#;
    apply_replacements(
        template_html.to_string(),
        common_replacements(invoice, template, locale),
    )
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
          <th class="num">PU HT</th><th class="num">Remise</th><th class="num">HT</th><th class="num">TVA</th><th class="num">Mt TVA</th><th class="num">BIC</th><th class="num">Mt BIC</th><th class="num">TTC</th>
        </tr>
      </thead>
      <tbody>__LINES_HTML__</tbody>
    </table>

    <div class="totals">
      <div class="totals-row"><span>Total HT</span><strong>__TOTAL_HT__</strong></div>
      <div class="totals-row"><span>Total TVA</span><strong>__TOTAL_TVA__</strong></div>
      <div class="totals-row"><span>Total BIC</span><strong>__TOTAL_BIC__</strong></div>
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
    apply_replacements(
        template_html.to_string(),
        common_replacements(invoice, template, "fr"),
    )
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
          <th class="num">PU HT</th><th class="num">Remise</th><th class="num">HT</th><th class="num">TVA</th><th class="num">Mt TVA</th><th class="num">BIC</th><th class="num">Mt BIC</th><th class="num">TTC</th>
        </tr>
      </thead>
      <tbody>__LINES_HTML__</tbody>
    </table>

    <div class="totals">
      <div class="totals-row"><span>Total HT</span><strong>__TOTAL_HT__</strong></div>
      <div class="totals-row"><span>Total TVA</span><strong>__TOTAL_TVA__</strong></div>
      <div class="totals-row"><span>Total BIC</span><strong>__TOTAL_BIC__</strong></div>
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
    apply_replacements(
        template_html.to_string(),
        common_replacements(invoice, template, "fr"),
    )
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
          <th class="num">PU HT</th><th class="num">Remise</th><th class="num">HT</th><th class="num">TVA</th><th class="num">Mt TVA</th><th class="num">BIC</th><th class="num">Mt BIC</th><th class="num">TTC</th>
        </tr>
      </thead>
      <tbody>__LINES_HTML__</tbody>
      <tbody>
        <tr class="subtotals"><td colspan="7">Sous-totaux</td><td class="num">__TOTAL_HT__</td><td class="num">__TOTAL_TVA__</td><td></td><td class="num">__TOTAL_BIC__</td><td></td><td class="num">__TOTAL_TTC__</td></tr>
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
        <div class="line"><span>Total BIC</span><strong>__TOTAL_BIC__</strong></div>
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
    apply_replacements(
        template_html.to_string(),
        common_replacements(invoice, template, "fr"),
    )
}

pub(crate) fn render_invoice_html_with_template(
    invoice: &InvoiceHeader,
    template: &InvoiceTemplate,
    locale: &str,
) -> String {
    match template.layout.as_str() {
        "classic" | "minimal" | "corporate" => render_modern(invoice, template, locale),
        _ => render_modern(invoice, template, locale),
    }
}

#[tauri::command]
pub fn render_invoice_html(
    state: State<'_, AppState>,
    invoice: InvoiceHeader,
    template_id: String,
    locale: Option<String>,
) -> Result<String, String> {
    let template = resolve_template(&state, &template_id)?;
    Ok(render_invoice_html_with_template(
        &invoice,
        &template,
        locale.as_deref().unwrap_or("fr"),
    ))
}

#[tauri::command]
pub fn render_invoice_html_preview(
    invoice: InvoiceHeader,
    template: InvoiceTemplate,
    locale: Option<String>,
) -> Result<String, String> {
    Ok(render_invoice_html_with_template(
        &invoice,
        &template,
        locale.as_deref().unwrap_or("fr"),
    ))
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
    if !config
        .invoice_templates
        .iter()
        .any(|existing| existing.id == template.id)
        && template_count >= 5
    {
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
    _app: AppHandle,
    _state: State<'_, AppState>,
    invoice: InvoiceHeader,
    _template_id: String,
    file_path: String,
    locale: Option<String>,
) -> Result<String, String> {
    let path = PathBuf::from(&file_path);
    let bytes = generate_invoice_pdf_bytes(&invoice, locale.as_deref().unwrap_or("fr"));
    std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
    Ok(path.to_string_lossy().to_string())
}
