import test from "node:test";
import assert from "node:assert/strict";

import {
  createInvokeWithTimeout,
  secondsToTimeoutMs,
  withTimeout,
} from "../src/utils/tauriTimeout.js";

test("withTimeout resolves when the promise completes before the deadline", async () => {
  const result = await withTimeout(Promise.resolve("ok"), 50, "Quick request");
  assert.equal(result, "ok");
});

test("withTimeout rejects with the action label when the deadline is exceeded", async () => {
  await assert.rejects(
    withTimeout(new Promise(() => {}), 600, "Connect database"),
    /Connect database timed out after/,
  );
});

test("createInvokeWithTimeout delegates to the provided invoke implementation", async () => {
  const calls = [];
  const invokeWithTimeout = createInvokeWithTimeout((command, payload) => {
    calls.push({ command, payload });
    return Promise.resolve({ ok: true, command });
  });

  const result = await invokeWithTimeout("get_tables", { id: "conn-1" }, {
    timeoutMs: 100,
    label: "Load tables",
  });

  assert.deepEqual(calls, [{ command: "get_tables", payload: { id: "conn-1" } }]);
  assert.deepEqual(result, { ok: true, command: "get_tables" });
});

test("secondsToTimeoutMs converts saved seconds and falls back for invalid values", () => {
  assert.equal(secondsToTimeoutMs(180, 90_000), 180_000);
  assert.equal(secondsToTimeoutMs("2.5", 90_000), 2_500);
  assert.equal(secondsToTimeoutMs(0, 90_000), 90_000);
  assert.equal(secondsToTimeoutMs("bad", 90_000), 90_000);
});
