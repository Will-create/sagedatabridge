import test from "node:test";
import assert from "node:assert/strict";

import {
  confirmationMatches,
  defaultBackupFileName,
  isUserProfilePath,
  needsSqlBackupStage,
  sameBackupPath,
  likelyStripeMatch,
  parentBackupDirectory,
  repairedDatabaseName,
  salvageTargetAllowed,
  stripeNameHints,
  filterTables,
  isTerminalJob,
  jobPercent,
  latestLogMessage,
  replaceVisibleTransferTables,
  showAdvancedTools,
  toggleTransferTable,
  transferEndpointsClash,
  visibleOperationTables,
} from "../src/components/operationsModel.js";

const parent = { schema: "dbo", name: "parent", row_count: 12 };
const child = { schema: "dbo", name: "child", row_count: 4 };
const other = { schema: "sales", name: "orders", row_count: 9 };

test("toggling tables adds then removes a transfer spec", () => {
  const once = toggleTransferTable([], parent);
  assert.deepEqual(once, [{
    schema: "dbo",
    table: "parent",
    targetSchema: "dbo",
    targetTable: "parent",
  }]);
  assert.deepEqual(toggleTransferTable(once, parent), []);
});

test("visible table selection replaces only the filtered set", () => {
  const current = toggleTransferTable([], other);
  const selected = replaceVisibleTransferTables(current, [parent, child], true);
  assert.equal(selected.length, 3);
  const cleared = replaceVisibleTransferTables(selected, [parent, child], false);
  assert.deepEqual(cleared, [{
    schema: "sales",
    table: "orders",
    targetSchema: "sales",
    targetTable: "orders",
  }]);
});

test("table search is case-insensitive and matches schema or name", () => {
  assert.deepEqual(filterTables([parent, child, other], "SALES").map((table) => table.name), ["orders"]);
  assert.deepEqual(filterTables([parent, child, other], "dbo.ch").map((table) => table.name), ["child"]);
});

test("job helpers cover progress, terminal states, and confirmation", () => {
  assert.equal(jobPercent({ percent: 42 }), 42);
  assert.equal(jobPercent({ percent: "bad" }), 0);
  assert.equal(isTerminalJob("interrupted"), true);
  assert.equal(isTerminalJob("running"), false);
  assert.equal(confirmationMatches(" BRAVIA ", "BRAVIA"), true);
  assert.equal(latestLogMessage({ logs: [{ message: "queued" }, { message: "copying" }] }), "copying");
});

test("advanced tools appear only after connections, databases, and tables are chosen", () => {
  assert.equal(showAdvancedTools({
    sourceConnectionId: "a",
    sourceDatabase: "SRC",
    targetConnectionId: "b",
    targetDatabase: "DST",
    tables: [{ schema: "dbo", table: "parent" }],
  }), true);
  assert.equal(showAdvancedTools({
    sourceConnectionId: "a",
    sourceDatabase: "SRC",
    targetConnectionId: "b",
    targetDatabase: "DST",
    tables: [],
  }), false);
});

test("empty tables can be hidden and backup names stay on the server path", () => {
  const tables = [parent, { schema: "dbo", name: "empty", row_count: 0 }];
  assert.deepEqual(visibleOperationTables(tables, "", true).map((table) => table.name), ["parent"]);
  assert.ok(defaultBackupFileName("BRAVIA", "D:\\SQLBackups").startsWith("D:\\SQLBackups\\BRAVIA-"));
  assert.equal(isUserProfilePath("C:\\Users\\USER\\Desktop\\cool\\BF.bak"), true);
  assert.equal(isUserProfilePath("C:\\Program Files\\Microsoft SQL Server\\Backup\\BF.bak"), false);
  assert.equal(needsSqlBackupStage("C:\\Users\\USER\\Downloads\\NTIT.bak"), true);
  assert.equal(needsSqlBackupStage("C:\\Program Files\\Microsoft SQL Server\\Backup\\NTIT.bak"), false);
  assert.equal(sameBackupPath("C:\\SQL\\Backup\\BF.bak", "C:/SQL/Backup/BF.bak"), true);
  assert.equal(sameBackupPath("C:\\SQL\\Backup\\BF.bak", "C:\\SQL\\Backup\\NTIT.bak"), false);
  assert.equal(repairedDatabaseName("BF"), "BF_repaired");
  assert.equal(parentBackupDirectory("C:\\Temp\\NTIT.bak"), "C:\\Temp");
  assert.deepEqual(stripeNameHints("C:\\Temp\\NTIT.bak").slice(0, 2), ["NTIT_1.bak", "NTIT1.bak"]);
  assert.equal(likelyStripeMatch("NTIT_1.bak", "C:\\Temp\\NTIT.bak"), true);
  assert.equal(likelyStripeMatch("NTIT.bak", "C:\\Temp\\NTIT.bak"), false);
  assert.equal(salvageTargetAllowed("BF_repaired"), true);
  assert.equal(salvageTargetAllowed("BF"), false);
  assert.equal(transferEndpointsClash({
    sourceConnectionId: "a",
    targetConnectionId: "a",
    sourceDatabase: "X",
    targetDatabase: "X",
  }), true);
  assert.equal(transferEndpointsClash({
    sourceConnectionId: "a",
    targetConnectionId: "a",
    sourceDatabase: "X",
    targetDatabase: "Y",
  }), false);
});
