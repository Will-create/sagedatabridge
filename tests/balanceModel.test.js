import assert from "node:assert/strict";
import test from "node:test";

import { buildBalanceTotals } from "../src/components/Dashboard/balanceModel.js";

test("balance totals split assessment and management accounts", () => {
  const row = (account_no, amount) => ({
    account_no,
    ouverture_debit: amount,
    ouverture_credit: amount + 1,
    mvt_debit: amount + 2,
    mvt_credit: amount + 3,
    cloture_debit: amount + 4,
    cloture_credit: amount + 5,
  });
  const totals = buildBalanceTotals([
    row("10100000", 10), row("60100000", 20),
    row("80100000", 30), row("99999999", 40),
  ]);

  assert.equal(totals.assessment.ouverture_debit, 50);
  assert.equal(totals.management.ouverture_debit, 50);
  assert.equal(totals.grand.ouverture_debit, 100);
  assert.equal(totals.assessment.cloture_credit, 60);
  assert.equal(totals.management.cloture_credit, 60);
  assert.equal(totals.grand.cloture_credit, 120);
});

test("balance totals tolerate missing and non-numeric amounts", () => {
  const totals = buildBalanceTotals([
    { account_no: "41100000", mvt_debit: "12.5" },
    { account_no: "70100000", mvt_debit: "invalid" },
  ]);
  assert.equal(totals.assessment.mvt_debit, 12.5);
  assert.equal(totals.management.mvt_debit, 0);
  assert.equal(totals.grand.mvt_debit, 12.5);
});
