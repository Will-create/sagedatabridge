export function withTimeout(promise, timeoutMs, label) {
  let timeoutId;
  const safeTimeoutMs = Math.max(Number(timeoutMs) || 0, 1);

  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timeoutId = globalThis.setTimeout(() => {
        reject(new Error(`${label} timed out after ${Math.round(safeTimeoutMs / 1000)}s`));
      }, safeTimeoutMs);
    }),
  ]).finally(() => {
    if (timeoutId) globalThis.clearTimeout(timeoutId);
  });
}

export function createInvokeWithTimeout(invokeImpl) {
  return function invokeWithTimeout(command, payload, { timeoutMs, label }) {
    return withTimeout(invokeImpl(command, payload), timeoutMs, label);
  };
}

export function secondsToTimeoutMs(value, fallbackMs) {
  const secs = Number(value);
  if (!Number.isFinite(secs) || secs <= 0) return fallbackMs;
  return Math.max(Math.round(secs * 1000), 1);
}
