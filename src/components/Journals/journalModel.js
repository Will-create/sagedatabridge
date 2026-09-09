export const JOURNAL_TYPE_VALUES = ["purchase", "sales", "treasury", "general"];

export function filterJournalOptions(journals, journalType) {
  if (!journalType || journalType === "all") return journals;
  return journals.filter((journal) => journal.journal_type === journalType);
}

export function journalTypeCounts(journals) {
  return JOURNAL_TYPE_VALUES.reduce((counts, value) => ({
    ...counts,
    [value]: journals.filter((journal) => journal.journal_type === value).length,
  }), {});
}

export function toIsoDate(date) {
  return date.toISOString().slice(0, 10);
}

export function getFiscalRange(startYear, month = 1, day = 1) {
  const start = new Date(Date.UTC(Number(startYear), Number(month) - 1, Number(day)));
  const end = new Date(Date.UTC(Number(startYear) + 1, Number(month) - 1, Number(day)));
  end.setUTCDate(end.getUTCDate() - 1);
  return { dateFrom: toIsoDate(start), dateTo: toIsoDate(end) };
}

export function getFiscalStartYear(date = new Date(), month = 1, day = 1) {
  const year = date.getUTCFullYear();
  const boundary = new Date(Date.UTC(year, Number(month) - 1, Number(day)));
  return date >= boundary ? year : year - 1;
}

export function buildExerciseYears(dateMin, dateMax, currentStartYear, month = 1, day = 1) {
  const years = new Set([Number(currentStartYear)]);
  const min = /^\d{4}-\d{2}-\d{2}$/.test(dateMin || "")
    ? new Date(`${dateMin}T00:00:00Z`)
    : null;
  const max = /^\d{4}-\d{2}-\d{2}$/.test(dateMax || "")
    ? new Date(`${dateMax}T00:00:00Z`)
    : null;
  if (min && max && !Number.isNaN(min.valueOf()) && !Number.isNaN(max.valueOf())) {
    const first = getFiscalStartYear(min, month, day);
    const last = getFiscalStartYear(max, month, day);
    for (let year = first; year <= last; year += 1) years.add(year);
  }
  return [...years].sort((left, right) => right - left);
}

export function validateJournalRange(dateFrom, dateTo) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(dateFrom || "") || !/^\d{4}-\d{2}-\d{2}$/.test(dateTo || "")) {
    return "required";
  }
  const from = new Date(`${dateFrom}T00:00:00Z`);
  const to = new Date(`${dateTo}T00:00:00Z`);
  if (Number.isNaN(from.valueOf()) || Number.isNaN(to.valueOf())) return "invalid";
  return from > to ? "order" : null;
}

export function journalEntryCells(entry) {
  return [
    entry.date,
    entry.journal_code,
    entry.piece_number,
    entry.account_number,
    entry.label,
    entry.debit,
    entry.credit,
  ];
}
