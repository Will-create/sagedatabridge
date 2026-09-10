export const TERMINAL_JOB_STATUSES = ["completed", "failed", "cancelled", "interrupted"];

export function tableKey(schema, table) {
  return `${schema}.${table}`;
}

export function sourceTableKey(table) {
  return tableKey(table?.schema, table?.name);
}

export function selectedTableKey(table) {
  return tableKey(table?.schema, table?.table);
}

export function toTransferTable(table) {
  return {
    schema: table.schema,
    table: table.name,
    targetSchema: table.schema,
    targetTable: table.name,
  };
}

export function toggleTransferTable(tables, table) {
  const key = sourceTableKey(table);
  const selected = (tables || []).some((item) => selectedTableKey(item) === key);
  if (selected) {
    return tables.filter((item) => selectedTableKey(item) !== key);
  }
  return [...(tables || []), toTransferTable(table)];
}

export function replaceVisibleTransferTables(tables, visibleTables, selected) {
  const visibleKeys = new Set((visibleTables || []).map(sourceTableKey));
  const kept = (tables || []).filter((item) => !visibleKeys.has(selectedTableKey(item)));
  if (!selected) return kept;
  return [...kept, ...(visibleTables || []).map(toTransferTable)];
}

export function filterTables(tables, query) {
  const needle = String(query || "").trim().toLowerCase();
  if (!needle) return tables || [];
  return (tables || []).filter((table) => sourceTableKey(table).toLowerCase().includes(needle));
}

export function isTerminalJob(status) {
  return TERMINAL_JOB_STATUSES.includes(status);
}

export function jobPercent(job) {
  const value = Number(job?.percent);
  return Number.isFinite(value) ? Math.min(100, Math.max(0, value)) : 0;
}

export function confirmationMatches(value, required) {
  return String(value || "").trim() === String(required || "").trim();
}

export function latestLogMessage(job) {
  const logs = job?.logs;
  if (!Array.isArray(logs) || logs.length === 0) return "";
  return logs[logs.length - 1]?.message || "";
}

export function groupTablesBySchema(tables) {
  return (tables || []).reduce((groups, table) => {
    const schema = table.schema || "dbo";
    if (!groups[schema]) groups[schema] = [];
    groups[schema].push(table);
    return groups;
  }, {});
}

export function visibleOperationTables(tables, query, hideEmpty) {
  return filterTables(tables, query).filter((table) => !hideEmpty || Number(table.row_count || 0) > 0);
}

export function showAdvancedTools(spec) {
  return Boolean(
    spec?.sourceConnectionId
    && spec?.sourceDatabase
    && spec?.targetConnectionId
    && spec?.targetDatabase
    && spec?.tables?.length,
  );
}

export function isUserProfilePath(path) {
  const normalized = String(path || "").replace(/\//g, "\\").toLowerCase();
  return normalized.includes("\\users\\")
    || normalized.includes("\\documents and settings\\")
    || normalized.includes("\\onedrive");
}

export function sameBackupPath(left, right) {
  const normalize = (value) => String(value || "").replace(/\//g, "\\").replace(/\\+$/, "").toLowerCase();
  return Boolean(left) && Boolean(right) && normalize(left) === normalize(right);
}

export function needsSqlBackupStage(path) {
  return isUserProfilePath(path);
}

export function defaultBackupFileName(database, directory = "") {
  const stamp = new Date().toISOString().slice(0, 16).replace(/[-:T]/g, "");
  const file = `${(database || "database").replace(/[^\w.-]+/g, "_")}-${stamp}.bak`;
  if (!directory) return file;
  const trimmed = directory.replace(/[\\/]+$/, "");
  return `${trimmed}\\${file}`;
}

export function salvageTargetAllowed(name) {
  return String(name || "").toLowerCase().includes("_repaired");
}

export function repairedDatabaseName(source) {
  const base = String(source || "").trim();
  if (!base) return "database_repaired";
  return salvageTargetAllowed(base) ? base : `${base}_repaired`;
}

export function parentBackupDirectory(path) {
  const normalized = String(path || "").replace(/\//g, "\\").replace(/[\\]+$/, "");
  const index = normalized.lastIndexOf("\\");
  return index > 0 ? normalized.slice(0, index) : "";
}

export function backupFileName(path) {
  const normalized = String(path || "").replace(/\//g, "\\");
  return normalized.split("\\").pop() || "";
}

export function stripeNameHints(path) {
  const name = backupFileName(path);
  if (!name.toLowerCase().endsWith(".bak")) return [];
  const stem = name.slice(0, -4);
  const hints = [];
  const add = (candidate) => {
    if (candidate && candidate.toLowerCase() !== name.toLowerCase() && !hints.some((existing) => existing.toLowerCase() === candidate.toLowerCase())) {
      hints.push(candidate);
    }
  };
  add(`${stem}_1.bak`);
  add(`${stem}1.bak`);
  add(`${stem}-1.bak`);
  add(`${stem}_2.bak`);
  add(`${stem}2.bak`);
  const stripped = stem.replace(/[-_]?\d+$/, "");
  if (stripped && stripped !== stem) add(`${stripped}.bak`);
  return hints;
}

export function likelyStripeMatch(candidateName, primaryPath, hints = stripeNameHints(primaryPath)) {
  const name = backupFileName(candidateName).toLowerCase();
  const primary = backupFileName(primaryPath).toLowerCase();
  if (!name.endsWith(".bak") || name === primary) return false;
  if (hints.map((hint) => hint.toLowerCase()).includes(name)) return true;
  const stem = primary.replace(/\.bak$/, "").replace(/[-_]?\d+$/, "");
  return stem.length >= 3 && name.startsWith(stem);
}

export function transferEndpointsClash(spec) {
  return spec.sourceConnectionId
    && spec.sourceConnectionId === spec.targetConnectionId
    && String(spec.sourceDatabase || "").toLowerCase() === String(spec.targetDatabase || "").toLowerCase();
}
