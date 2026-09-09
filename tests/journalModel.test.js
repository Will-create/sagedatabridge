import test from "node:test";
import assert from "node:assert/strict";

import {
  buildExerciseYears,
  filterJournalOptions,
  getFiscalRange,
  getFiscalStartYear,
  journalEntryCells,
  journalTypeCounts,
  validateJournalRange,
} from "../src/components/Journals/journalModel.js";

const journals = [
  { code: "AC", name: "Achats", journal_type: "purchase" },
  { code: "VE", name: "Ventes", journal_type: "sales" },
  { code: "BQ", name: "Banque", journal_type: "treasury" },
  { code: "SI", name: "Situation", journal_type: "other" },
];

test("journal options cascade from the selected type", () => {
  assert.deepEqual(filterJournalOptions(journals, "purchase").map((item) => item.code), ["AC"]);
  assert.deepEqual(filterJournalOptions(journals, "all"), journals);
});

test("journal type counts include supported empty categories", () => {
  assert.deepEqual(journalTypeCounts(journals), {
    purchase: 1,
    sales: 1,
    treasury: 1,
    general: 0,
  });
});

test("journal entries map to the virtual grid cells", () => {
  assert.deepEqual(journalEntryCells({
    date: "2026-09-04",
    journal_code: "VE",
    piece_number: "FAC-1",
    account_number: "701000",
    label: "Vente",
    debit: 0,
    credit: 125,
  }), ["2026-09-04", "VE", "FAC-1", "701000", "Vente", 0, 125]);
});

test("fiscal ranges span the configured accounting exercise", () => {
  assert.deepEqual(getFiscalRange(2025, 7, 1), {
    dateFrom: "2025-07-01",
    dateTo: "2026-06-30",
  });
  assert.equal(getFiscalStartYear(new Date("2026-03-15T00:00:00Z"), 7, 1), 2025);
  assert.equal(getFiscalStartYear(new Date("2026-08-01T00:00:00Z"), 7, 1), 2026);
});

test("exercise choices cover Sage data bounds and retain the current exercise", () => {
  assert.deepEqual(buildExerciseYears("2023-08-01", "2026-01-10", 2026, 7, 1), [2026, 2025, 2024, 2023]);
  assert.deepEqual(buildExerciseYears(null, null, 2026, 1, 1), [2026]);
});

test("journal date intervals must be complete and ordered", () => {
  assert.equal(validateJournalRange("", "2026-12-31"), "required");
  assert.equal(validateJournalRange("2027-01-01", "2026-12-31"), "order");
  assert.equal(validateJournalRange("2026-01-01", "2026-12-31"), null);
});
