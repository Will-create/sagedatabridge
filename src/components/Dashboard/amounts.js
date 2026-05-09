const BILLION = 1_000_000_000;
const MILLION = 1_000_000;
const THOUSAND = 1_000;

function localeFor(lang) {
  return lang === "fr" ? "fr-FR" : "en-US";
}

export function toNumber(value) {
  if (typeof value === "number") return Number.isFinite(value) ? value : 0;
  if (value == null || value === "") return 0;
  const normalized = String(value).replace(/\s+/g, "").replace(",", ".");
  const parsed = Number(normalized);
  return Number.isFinite(parsed) ? parsed : 0;
}

export function getAmountTone(value) {
  const amount = toNumber(value);
  if (amount > 0) return "positive";
  if (amount < 0) return "negative";
  return "neutral";
}

export function getAmountTextStyle(value) {
  const abs = Math.abs(toNumber(value));
  if (abs >= BILLION) {
    return {
      fontSize: "10px",
      letterSpacing: "-0.03em",
    };
  }
  if (abs >= MILLION) {
    return {
      fontSize: "11px",
    };
  }
  return {
    fontSize: "13px",
  };
}

export function formatAmount(value, lang) {
  const amount = toNumber(value);
  const abs = Math.abs(amount);

  const formatter = new Intl.NumberFormat(localeFor(lang), {
    minimumFractionDigits: !Number.isInteger(amount) && abs < THOUSAND ? 2 : 0,
    maximumFractionDigits: !Number.isInteger(amount) && abs < THOUSAND ? 2 : 0,
  });

  return formatter.format(amount).replace(/\u202f|\u00a0/g, " ");
}
