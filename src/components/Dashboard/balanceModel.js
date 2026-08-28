export const BALANCE_AMOUNT_FIELDS = [
  "ouverture_debit",
  "ouverture_credit",
  "mvt_debit",
  "mvt_credit",
  "cloture_debit",
  "cloture_credit",
];

function emptyTotals() {
  return Object.fromEntries(BALANCE_AMOUNT_FIELDS.map((field) => [field, 0]));
}

function addRow(totals, row) {
  BALANCE_AMOUNT_FIELDS.forEach((field) => {
    const value = Number(row?.[field]);
    totals[field] += Number.isFinite(value) ? value : 0;
  });
}

export function buildBalanceTotals(rows = []) {
  const assessment = emptyTotals();
  const management = emptyTotals();
  const grand = emptyTotals();

  rows.forEach((row) => {
    const accountClass = String(row?.account_no ?? "").trim().charAt(0);
    addRow(["6", "7", "8"].includes(accountClass) ? management : assessment, row);
    addRow(grand, row);
  });

  return { assessment, management, grand };
}
