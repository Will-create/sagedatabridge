import {
  startTransition,
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { save } from "@tauri-apps/api/dialog";
import { exists, removeFile, renameFile, writeBinaryFile } from "@tauri-apps/api/fs";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import * as XLSX from "xlsx";

import {
  exportGrandLivreXlsx,
  getBalance,
  getDashboardKpis,
  getDashboardTiers,
  getGrandLivrePage,
  getSettings,
  DASHBOARD_PREVIEW_ROW_LIMIT,
  streamGrandLivreAuxiliaire,
} from "../../hooks/useTauri";
import {
  formatAmount,
  getAmountTextStyle,
  getAmountTone,
  toNumber,
} from "./amounts";
import { useExportJobs } from "../../exportJobs";
import CompteExploitationTab from "./CompteExploitation";
import { EXPLOITATION_MONTHS } from "./exploitationModel";

const TAB_IDS = {
  OVERVIEW: "overview",
  GRAND_LIVRE: "grand_livre",
  BALANCE: "balance",
  FOURNISSEURS: "fournisseurs",
  CLIENTS: "clients",
  COMPTE_EXPLOITATION: "compte_exploitation",
  TIERS: "tiers",
};

const GRAND_LIVRE_GRID = "110px 110px 120px minmax(240px, 1.4fr) 140px 110px 110px 110px 130px";
const AUX_GRID = "110px 92px minmax(260px, 1.5fr) 140px 92px 110px 110px 130px";
const LEDGER_PAGE_SIZE = 500;
const LEDGER_CACHE_PAGES = 40;

const EXCEL_COLORS = {
  headerBg: "0C0C0F",
  headerText: "FFFFFF",
  accent: "1DE8C8",
  groupBg: "16161C",
  totalBg: "1F1F28",
  metaBg: "14141B",
  green: "22D97A",
  red: "FF4F5E",
  neutral: "9499B0",
  text: "E8EAF0",
  border: "27273A",
};

const EXCEL_BORDER = {
  top: { style: "thin", color: { rgb: EXCEL_COLORS.border } },
  bottom: { style: "thin", color: { rgb: EXCEL_COLORS.border } },
  left: { style: "thin", color: { rgb: EXCEL_COLORS.border } },
  right: { style: "thin", color: { rgb: EXCEL_COLORS.border } },
};

const EXCEL_MAX_ROWS = 1_048_576;

function getDefaultRange() {
  const now = new Date();
  return {
    dateFrom: `${now.getFullYear()}-01-01`,
    dateTo: now.toISOString().slice(0, 10),
  };
}

function toIsoDate(date) {
  return date.toISOString().slice(0, 10);
}

function getFiscalRange(startYear, month = 1, day = 1) {
  const start = new Date(Date.UTC(startYear, month - 1, day));
  const end = new Date(Date.UTC(startYear + 1, month - 1, day));
  end.setUTCDate(end.getUTCDate() - 1);
  return { dateFrom: toIsoDate(start), dateTo: toIsoDate(end) };
}

function normalizeText(value) {
  return String(value ?? "")
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, "")
    .toLowerCase();
}

function mergeStyle(base, overrides = {}) {
  return {
    ...base,
    ...overrides,
    font: { ...(base.font || {}), ...(overrides.font || {}) },
    fill: { ...(base.fill || {}), ...(overrides.fill || {}) },
    alignment: { ...(base.alignment || {}), ...(overrides.alignment || {}) },
    border: { ...(base.border || {}), ...(overrides.border || {}) },
  };
}

const BODY_STYLE = {
  font: { name: "Aptos", sz: 10, color: { rgb: EXCEL_COLORS.text } },
  fill: { patternType: "solid", fgColor: { rgb: "111115" } },
  alignment: { vertical: "center", horizontal: "left" },
  border: EXCEL_BORDER,
};

const ALT_BODY_STYLE = mergeStyle(BODY_STYLE, {
  fill: { fgColor: { rgb: "15151D" } },
});

const HEADER_STYLE = mergeStyle(BODY_STYLE, {
  font: { bold: true, color: { rgb: EXCEL_COLORS.headerText } },
  fill: { fgColor: { rgb: EXCEL_COLORS.headerBg } },
  border: {
    ...EXCEL_BORDER,
    bottom: { style: "medium", color: { rgb: EXCEL_COLORS.accent } },
  },
});

const GROUP_STYLE = mergeStyle(BODY_STYLE, {
  font: { bold: true },
  fill: { fgColor: { rgb: EXCEL_COLORS.groupBg } },
});

const TOTAL_STYLE = mergeStyle(BODY_STYLE, {
  font: { bold: true, color: { rgb: EXCEL_COLORS.green } },
  fill: { fgColor: { rgb: EXCEL_COLORS.totalBg } },
});

const TITLE_STYLE = mergeStyle(BODY_STYLE, {
  font: { name: "Aptos Display", sz: 16, bold: true, color: { rgb: EXCEL_COLORS.headerText } },
  fill: { fgColor: { rgb: EXCEL_COLORS.headerBg } },
  alignment: { vertical: "center", horizontal: "left" },
  border: {
    ...EXCEL_BORDER,
    bottom: { style: "medium", color: { rgb: EXCEL_COLORS.accent } },
  },
});

const META_STYLE = mergeStyle(BODY_STYLE, {
  font: { sz: 10, color: { rgb: EXCEL_COLORS.neutral } },
  fill: { fgColor: { rgb: EXCEL_COLORS.metaBg } },
});

function excelNumberFormat(value) {
  const amount = Math.abs(toNumber(value));
  return amount >= 1000 || Number.isInteger(toNumber(value)) ? "#,##0" : "#,##0.00";
}

function amountCellStyle(kind, role, value, alt = false) {
  const amount = toNumber(value);
  const tone = amount > 0 ? EXCEL_COLORS.green : amount < 0 ? EXCEL_COLORS.red : EXCEL_COLORS.neutral;
  const base = kind === "total"
    ? TOTAL_STYLE
    : kind === "group"
      ? GROUP_STYLE
      : alt
        ? ALT_BODY_STYLE
        : BODY_STYLE;
  const roleColor = role === "debit" ? EXCEL_COLORS.red : role === "credit" ? EXCEL_COLORS.green : tone;

  return mergeStyle(base, {
    font: {
      name: "Consolas",
      color: { rgb: kind === "total" ? EXCEL_COLORS.green : roleColor },
    },
    alignment: { horizontal: "right" },
  });
}

function textCellStyle(kind, alt = false) {
  if (kind === "title") return TITLE_STYLE;
  if (kind === "meta" || kind === "spacer") return META_STYLE;
  if (kind === "header") return HEADER_STYLE;
  if (kind === "group") return GROUP_STYLE;
  if (kind === "total") return TOTAL_STYLE;
  return alt ? ALT_BODY_STYLE : BODY_STYLE;
}

function safeSheetName(name) {
  return name.replace(/[\\/?*\[\]:]/g, "_").slice(0, 31) || "Dashboard";
}

function computeColumnWidths(rows, columnRoles, lang) {
  const count = Math.max(...rows.map((row) => row.values.length), 0);

  return Array.from({ length: count }, (_, columnIndex) => {
    const role = columnRoles[columnIndex] || "text";
    const width = rows.reduce((max, row) => {
      const value = row.values[columnIndex];
      const length = typeof value === "number"
        ? formatAmount(value, lang).length
        : String(value ?? "").length;
      return Math.max(max, length);
    }, 0);

    const minWidth = role === "debit" || role === "credit" || role === "balance" || role === "number"
      ? 12
      : 10;

    return { wch: Math.max(minWidth, Math.min(width + 2, 42)) };
  });
}

function buildPreambleRows(title, metaLines, columnCount) {
  if (!title && !metaLines.length) return [];

  const blankRow = Array.from({ length: columnCount }, () => "");
  return [
    ...(title ? [{ kind: "title", values: [title, ...blankRow.slice(1)] }] : []),
    ...metaLines.map((line) => ({ kind: "meta", values: [line, ...blankRow.slice(1)] })),
    { kind: "spacer", values: blankRow },
  ];
}

function buildWorksheet(rows, columnRoles, lang, options = {}) {
  const columnCount = Math.max(
    columnRoles.length,
    ...rows.map((row) => row.values.length),
    1,
  );
  const metaLines = [options.context, options.period, options.generatedAt].filter(Boolean);
  const preambleRows = buildPreambleRows(options.title, metaLines, columnCount);
  const allRows = [...preambleRows, ...rows];
  const sheetName = options.title || "Worksheet";

  if (allRows.length > EXCEL_MAX_ROWS) {
    throw new Error(
      `${sheetName}: ${allRows.length.toLocaleString()} rows exceed Excel's ${EXCEL_MAX_ROWS.toLocaleString()} row limit. Narrow the date range or account filter before exporting.`,
    );
  }

  const headerRowIndex = allRows.findIndex((row) => row.kind === "header");
  const sheet = XLSX.utils.aoa_to_sheet(allRows.map((row) => row.values));
  const range = XLSX.utils.decode_range(sheet["!ref"] || "A1");
  const merges = [];

  allRows.forEach((row, rowIndex) => {
    row.values.forEach((value, columnIndex) => {
      const address = XLSX.utils.encode_cell({ r: rowIndex, c: columnIndex });
      if (!sheet[address]) {
        sheet[address] = { t: "s", v: "" };
      }

      const role = row.roles?.[columnIndex] || columnRoles[columnIndex] || "text";
      const style = role === "debit" || role === "credit" || role === "balance" || role === "number"
        ? amountCellStyle(row.kind, role, value, row.alt)
        : textCellStyle(row.kind, row.alt);

      sheet[address].s = style;

      if (typeof value === "number") {
        sheet[address].t = "n";
        sheet[address].v = value;
        sheet[address].z = excelNumberFormat(value);
      } else {
        sheet[address].t = "s";
        sheet[address].v = value == null ? "" : String(value);
      }
    });
  });

  if (preambleRows.length) {
    preambleRows.forEach((_, rowIndex) => {
      merges.push({
        s: { r: rowIndex, c: 0 },
        e: { r: rowIndex, c: columnCount - 1 },
      });
    });
  }

  sheet["!cols"] = computeColumnWidths(rows, columnRoles, lang);
  sheet["!freeze"] = {
    xSplit: 0,
    ySplit: Math.max(headerRowIndex + 1, 1),
    topLeftCell: XLSX.utils.encode_cell({ r: Math.max(headerRowIndex + 1, 1), c: 0 }),
    activePane: "bottomLeft",
    state: "frozen",
  };
  if (headerRowIndex >= 0) {
    sheet["!autofilter"] = {
      ref: XLSX.utils.encode_range({
        s: { r: headerRowIndex, c: 0 },
        e: { r: range.e.r, c: range.e.c },
      }),
    };
  }
  if (merges.length) sheet["!merges"] = merges;

  return sheet;
}

function buildExploitationWorksheet(report, t, lang, options = {}) {
  const preambleCount = 5;
  const comparing = !!report.hasBudget;
  const sheetRows = [
    { kind: "header", values: [t("dashboard_label"), ...(comparing ? EXPLOITATION_MONTHS.flatMap((month) => [month, "", ""]) : EXPLOITATION_MONTHS), ...(comparing ? ["TOTAL", "", ""] : ["TOTAL"])] },
    ...(comparing ? [{ kind: "header", values: ["", ...EXPLOITATION_MONTHS.flatMap(() => [t("exploitation_budget"), t("exploitation_actual"), t("exploitation_variance")]), t("exploitation_budget"), t("exploitation_actual"), t("exploitation_variance")] }] : []),
    ...report.rows.map((row) => ({
      kind: row.kind === "section" ? "group" : row.kind === "total" ? "total" : "data",
      values: row.kind === "section"
        ? [row.label, ...Array(comparing ? 39 : 13).fill("")]
        : comparing
          ? [row.label, ...Array.from({ length: 12 }, (_, month) => [row.budget[month], row.actual[month], row.variance[month]]).flat(), row.totalBudget, row.totalActual, row.totalVariance]
          : [row.label, ...row.actual, row.totalActual],
    })),
  ];
  const valueColumnCount = comparing ? 39 : 13;
  const worksheet = buildWorksheet(sheetRows, ["text", ...Array(valueColumnCount).fill("balance")], lang, options);
  const monthHeaderRow = preambleCount;
  if (comparing) worksheet["!merges"] = [
      ...(worksheet["!merges"] || []),
      ...Array.from({ length: 12 }, (_, month) => ({ s: { r: monthHeaderRow, c: 1 + month * 3 }, e: { r: monthHeaderRow, c: 3 + month * 3 } })),
      { s: { r: monthHeaderRow, c: 37 }, e: { r: monthHeaderRow, c: 39 } },
      { s: { r: monthHeaderRow, c: 0 }, e: { r: monthHeaderRow + 1, c: 0 } },
    ];
  report.rows.forEach((row, rowIndex) => {
    if (row.format !== "percent") return;
    for (let column = 1; column <= valueColumnCount; column += 1) {
      const cell = worksheet[XLSX.utils.encode_cell({ r: preambleCount + (comparing ? 2 : 1) + rowIndex, c: column })];
      if (cell?.t === "n") cell.z = "0.00%";
    }
  });
  return worksheet;
}

function buildExploitationDetailWorksheet(detail, t, lang, options = {}) {
  const rows = [
    { kind: "group", values: [detail.line.lineLabel, "", "", "", "", "", ""] },
    { kind: "header", values: [t("dashboard_date"), t("dashboard_journal"), t("dashboard_account_no"), t("dashboard_label"), t("dashboard_ref_piece"), t("dashboard_debit"), t("dashboard_credit")] },
    ...detail.movements.map((row) => ({ kind: "data", values: [row.date || "", row.journal || "", row.account_no || "", row.account_label || row.description || "", row.ref_piece || "", toNumber(row.debit), toNumber(row.credit)] })),
    { kind: "total", values: [t("dashboard_total"), "", "", "", "", detail.accounts.reduce((sum, row) => sum + toNumber(row.debit), 0), detail.accounts.reduce((sum, row) => sum + toNumber(row.credit), 0)] },
  ];
  return buildWorksheet(rows, ["text", "text", "text", "text", "text", "debit", "credit"], lang, options);
}

function yieldToUi() {
  return new Promise((resolve) => window.setTimeout(resolve, 0));
}

async function saveWorkbook(defaultPath, buildSheets, onProgress = () => {}) {
  const filePath = await save({
    defaultPath,
    filters: [{ name: "Excel Workbook", extensions: ["xlsx"] }],
  });
  if (!filePath) return null;

  onProgress({ phase: "preparing data", percent: 10 });
  await yieldToUi();
  const sheets = await buildSheets();
  onProgress({ phase: "building workbook", percent: 35 });
  await yieldToUi();
  const workbook = XLSX.utils.book_new();
  workbook.Props = {
    Title: defaultPath,
    Subject: "Sage Data Bridge export",
    Author: "Sage Data Bridge",
    Company: "Sage Data Bridge",
  };
  sheets.forEach(({ name, worksheet }) => {
    XLSX.utils.book_append_sheet(workbook, worksheet, safeSheetName(name));
  });

  onProgress({ phase: "serializing workbook", percent: 60 });
  await yieldToUi();
  const contents = new Uint8Array(
    XLSX.write(workbook, { bookType: "xlsx", type: "array", cellStyles: true }),
  );
  onProgress({ phase: "writing file", percent: 85, bytes_written: contents.byteLength });
  await yieldToUi();
  const tempPath = `${filePath}.sdb-${crypto.randomUUID()}.tmp`;
  const backupPath = `${filePath}.sdb-${crypto.randomUUID()}.bak`;
  let movedExistingFile = false;
  try {
    await writeBinaryFile(tempPath, contents);
    onProgress({ phase: "finalizing", percent: 95, bytes_written: contents.byteLength });
    if (await exists(filePath)) {
      await renameFile(filePath, backupPath);
      movedExistingFile = true;
    }
    await renameFile(tempPath, filePath);
    if (movedExistingFile) await removeFile(backupPath).catch(() => {});
  } catch (error) {
    await removeFile(tempPath).catch(() => {});
    if (movedExistingFile && !(await exists(filePath))) {
      await renameFile(backupPath, filePath).catch(() => {});
    }
    throw error;
  }
  return filePath;
}

function buildGrandLivreRows(rows, search) {
  const groups = [];
  let current = null;

  rows.forEach((row) => {
    const key = `${row.account_no}::${row.account_label}`;
    if (!current || current.key !== key) {
      current = { key, account_no: row.account_no, account_label: row.account_label, rows: [] };
      groups.push(current);
    }
    current.rows.push(row);
  });

  const query = normalizeText(search);
  const flattened = [];

  groups.forEach((group) => {
    const groupMatch = !query || normalizeText(`${group.account_no} ${group.account_label}`).includes(query);
    const subtotal = group.rows.find((row) => row.row_type === "subtotal");
    const entries = group.rows.filter((row) => row.row_type !== "subtotal");
    const visibleEntries = groupMatch
      ? entries
      : entries.filter((row) =>
          normalizeText(
            `${row.account_no} ${row.account_label} ${row.journal ?? ""} ${row.ref_piece ?? ""} ${row.lettrage ?? ""}`,
          ).includes(query)
        );

    if (!visibleEntries.length && !(groupMatch && subtotal)) return;

    flattened.push({
      type: "account-header",
      key: `header-${group.key}`,
      account_no: group.account_no,
      account_label: group.account_label,
    });

    visibleEntries.forEach((row, index) => {
      flattened.push({
        type: "entry",
        key: `${group.key}-${row.date ?? "nd"}-${row.journal ?? "nj"}-${row.ref_piece ?? "nr"}-${index}`,
        alt: index % 2 === 1,
        ...row,
      });
    });

    if (subtotal) {
      flattened.push({
        type: "subtotal",
        key: `subtotal-${group.key}`,
        ...subtotal,
      });
    }
  });

  return flattened;
}

function addGrandLivreSubtotalRows(rows) {
  if (!Array.isArray(rows) || !rows.length) return [];
  if (rows.some((row) => row.row_type === "subtotal")) return rows;

  const result = [];
  let currentKey = "";
  let subtotal = null;

  const flushSubtotal = () => {
    if (!subtotal) return;
    result.push({
      row_type: "subtotal",
      date: null,
      journal: null,
      account_no: subtotal.account_no,
      account_label: subtotal.account_label,
      ref_piece: null,
      lettrage: null,
      debit: 0,
      credit: 0,
      solde_cumule: subtotal.solde,
      total_debit: subtotal.total_debit,
      total_credit: subtotal.total_credit,
      solde: subtotal.solde,
    });
  };

  rows.forEach((row) => {
    const key = `${row.account_no}::${row.account_label}`;
    if (key !== currentKey) {
      flushSubtotal();
      currentKey = key;
      subtotal = {
        account_no: row.account_no,
        account_label: row.account_label,
        total_debit: 0,
        total_credit: 0,
        solde: 0,
      };
    }

    result.push(row);
    subtotal.total_debit += toNumber(row.debit);
    subtotal.total_credit += toNumber(row.credit);
    subtotal.solde += toNumber(row.debit) - toNumber(row.credit);
  });

  flushSubtotal();
  return result;
}

function buildBalanceGroups(rows, search) {
  const query = normalizeText(search);
  const filtered = rows.filter((row) =>
    !query || normalizeText(`${row.account_no} ${row.account_label}`).includes(query)
  );
  const groups = new Map();

  filtered.forEach((row) => {
    const classCode = String(row.account_no ?? "").slice(0, 1) || "?";
    if (!groups.has(classCode)) groups.set(classCode, []);
    groups.get(classCode).push(row);
  });

  return {
    filtered,
    groups: [...groups.entries()].sort((a, b) => a[0].localeCompare(b[0])),
  };
}

function buildTierItems(rows) {
  const totals = new Map();

  rows.forEach((row) => {
    const key = row.tiers_code;
    if (!key) return;

    if (!totals.has(key)) {
      totals.set(key, {
        tiers_code: row.tiers_code,
        tiers_name: row.tiers_name,
        account_no: row.account_no,
        account_label: row.account_label,
        total_debit: 0,
        total_credit: 0,
        solde: 0,
      });
    }

    const current = totals.get(key);
    if (row.is_total_row) {
      current.total_debit = toNumber(row.debit);
      current.total_credit = toNumber(row.credit);
      current.solde = toNumber(row.running_balance);
      current.tiers_name = row.tiers_name || current.tiers_name;
      current.account_no = row.account_no || current.account_no;
      current.account_label = row.account_label || current.account_label;
      return;
    }

    current.total_debit += toNumber(row.debit);
    current.total_credit += toNumber(row.credit);
    current.solde = toNumber(row.running_balance);
  });

  return [...totals.values()].sort((a, b) => a.tiers_code.localeCompare(b.tiers_code));
}

function buildAuxiliaryDisplayRows(rows, selectedTier = null) {
  const filtered = selectedTier ? rows.filter((row) => row.tiers_code === selectedTier) : rows;
  const flattened = [];
  let lastAccountKey = "";
  let lastTierKey = "";
  let transactionIndex = 0;

  filtered.forEach((row, index) => {
    const accountKey = `${row.account_no}::${row.account_label}`;
    const tierKey = `${row.tiers_code}::${row.tiers_name}`;

    if (accountKey !== lastAccountKey) {
      flattened.push({
        type: "account-header",
        key: `aux-account-${accountKey}`,
        account_no: row.account_no,
        account_label: row.account_label,
      });
      lastAccountKey = accountKey;
      lastTierKey = "";
    }

    if (tierKey !== lastTierKey) {
      flattened.push({
        type: "tiers-header",
        key: `aux-tier-${tierKey}-${index}`,
        tiers_code: row.tiers_code,
        tiers_name: row.tiers_name,
      });
      lastTierKey = tierKey;
    }

    flattened.push({
      type: row.is_total_row ? "total" : "entry",
      key: `aux-row-${accountKey}-${tierKey}-${index}`,
      alt: !row.is_total_row && transactionIndex % 2 === 1,
      ...row,
    });

    if (!row.is_total_row) transactionIndex += 1;
  });

  return flattened;
}

function buildOverviewWorksheet(data, t, lang, options = {}) {
  const rows = [
    { kind: "header", values: [t("dashboard_label").toUpperCase(), t("dashboard_amount").toUpperCase(), ""], roles: ["text", "balance", "text"] },
    { kind: "data", values: [t("dashboard_kpi_revenue"), toNumber(data.total_ventes), ""], roles: ["text", "balance", "text"] },
    { kind: "data", values: [t("dashboard_kpi_purchases"), toNumber(data.total_achats), ""], roles: ["text", "balance", "text"] },
    { kind: "data", values: [t("dashboard_kpi_clients"), toNumber(data.solde_clients), ""], roles: ["text", "balance", "text"] },
    { kind: "data", values: [t("dashboard_kpi_suppliers"), toNumber(data.solde_fournisseurs), ""], roles: ["text", "balance", "text"] },
    { kind: "data", values: [t("dashboard_entries_count"), toNumber(data.nb_ecritures), ""], roles: ["text", "number", "text"] },
    { kind: "data", values: [t("dashboard_active_tiers"), toNumber(data.nb_tiers_actifs), ""], roles: ["text", "number", "text"] },
    { kind: "group", values: ["", "", ""] },
    { kind: "header", values: [t("dashboard_month").toUpperCase(), t("dashboard_sales").toUpperCase(), t("dashboard_purchases").toUpperCase()] },
    ...(data.evolution_mensuelle ?? []).map((item, index) => ({
      kind: "data",
      alt: index % 2 === 1,
      values: [item.month, toNumber(item.ventes), toNumber(item.achats)],
      roles: ["text", "credit", "debit"],
    })),
  ];

  return buildWorksheet(rows, ["text", "balance", "debit"], lang, options);
}

function buildGrandLivreWorksheet(displayRows, t, lang, options = {}) {
  const rows = [
    {
      kind: "header",
      values: [
        t("dashboard_date").toUpperCase(),
        t("dashboard_journal").toUpperCase(),
        t("dashboard_account_no").toUpperCase(),
        t("dashboard_label").toUpperCase(),
        t("dashboard_ref_piece").toUpperCase(),
        t("dashboard_matching").toUpperCase(),
        t("dashboard_debit").toUpperCase(),
        t("dashboard_credit").toUpperCase(),
        t("dashboard_running_balance").toUpperCase(),
      ],
    },
    ...displayRows.map((row) => {
      if (row.type === "account-header") {
        return {
          kind: "group",
          values: [`${row.account_no} - ${row.account_label}`, "", "", "", "", "", "", "", ""],
        };
      }

      if (row.type === "subtotal") {
        return {
          kind: "total",
          values: [
            t("dashboard_total"),
            "",
            row.account_no,
            row.account_label,
            "",
            "",
            toNumber(row.total_debit),
            toNumber(row.total_credit),
            toNumber(row.solde),
          ],
        };
      }

      return {
        kind: "data",
        values: [
          row.date || "",
          row.journal || "",
          row.account_no,
          row.account_label,
          row.ref_piece || "",
          row.lettrage || "",
          toNumber(row.debit),
          toNumber(row.credit),
          toNumber(row.solde_cumule),
        ],
      };
    }),
  ];

  return buildWorksheet(rows, ["text", "text", "text", "text", "text", "text", "debit", "credit", "balance"], lang, options);
}

function buildBalanceWorksheet(rows, t, lang, options = {}) {
  const grouped = buildBalanceGroups(rows, "");
  const grandTotal = grouped.filtered.reduce(
    (totals, row) => ({
      ouverture_debit: totals.ouverture_debit + toNumber(row.ouverture_debit),
      ouverture_credit: totals.ouverture_credit + toNumber(row.ouverture_credit),
      mvt_debit: totals.mvt_debit + toNumber(row.mvt_debit),
      mvt_credit: totals.mvt_credit + toNumber(row.mvt_credit),
      cloture_debit: totals.cloture_debit + toNumber(row.cloture_debit),
      cloture_credit: totals.cloture_credit + toNumber(row.cloture_credit),
    }),
    {
      ouverture_debit: 0,
      ouverture_credit: 0,
      mvt_debit: 0,
      mvt_credit: 0,
      cloture_debit: 0,
      cloture_credit: 0,
    },
  );

  const exportRows = [
    {
      kind: "header",
      values: [
        t("dashboard_account_no").toUpperCase(),
        t("dashboard_label").toUpperCase(),
        t("dashboard_opening_debit").toUpperCase(),
        t("dashboard_opening_credit").toUpperCase(),
        t("dashboard_movement_debit").toUpperCase(),
        t("dashboard_movement_credit").toUpperCase(),
        t("dashboard_closing_debit").toUpperCase(),
        t("dashboard_closing_credit").toUpperCase(),
      ],
    },
    ...grouped.groups.flatMap(([classCode, groupRows]) => ([
      {
        kind: "group",
        values: [t("dashboard_class_label", classCode), "", "", "", "", "", "", ""],
      },
      ...groupRows.map((row, index) => ({
        kind: "data",
        alt: index % 2 === 1,
        values: [
          row.account_no,
          row.account_label,
          toNumber(row.ouverture_debit),
          toNumber(row.ouverture_credit),
          toNumber(row.mvt_debit),
          toNumber(row.mvt_credit),
          toNumber(row.cloture_debit),
          toNumber(row.cloture_credit),
        ],
      })),
    ])),
    {
      kind: "total",
      values: [
        t("dashboard_grand_total"),
        "",
        grandTotal.ouverture_debit,
        grandTotal.ouverture_credit,
        grandTotal.mvt_debit,
        grandTotal.mvt_credit,
        grandTotal.cloture_debit,
        grandTotal.cloture_credit,
      ],
    },
  ];

  return buildWorksheet(exportRows, ["text", "text", "debit", "credit", "debit", "credit", "debit", "credit"], lang, options);
}

function buildAuxiliaryWorksheet(displayRows, t, lang, options = {}) {
  const rows = [
    {
      kind: "header",
      values: [
        t("dashboard_date").toUpperCase(),
        t("dashboard_journal").toUpperCase(),
        t("dashboard_label").toUpperCase(),
        t("dashboard_ref_piece").toUpperCase(),
        t("dashboard_matching").toUpperCase(),
        t("dashboard_debit").toUpperCase(),
        t("dashboard_credit").toUpperCase(),
        t("dashboard_running_balance").toUpperCase(),
      ],
    },
    ...displayRows.map((row) => {
      if (row.type === "account-header") {
        return {
          kind: "group",
          values: [`${row.account_no} - ${row.account_label}`, "", "", "", "", "", "", ""],
        };
      }
      if (row.type === "tiers-header") {
        return {
          kind: "group",
          values: [`${row.tiers_code} - ${row.tiers_name || "-"}`, "", "", "", "", "", "", ""],
        };
      }
      if (row.type === "total") {
        return {
          kind: "total",
          values: [
            t("dashboard_total"),
            "",
            row.tiers_name || row.tiers_code,
            "",
            "",
            toNumber(row.debit),
            toNumber(row.credit),
            toNumber(row.running_balance),
          ],
        };
      }

      return {
        kind: "data",
        values: [
          row.date || "",
          row.journal || "",
          row.description || "",
          row.ref_piece || "",
          row.lettrage || "",
          toNumber(row.debit),
          toNumber(row.credit),
          toNumber(row.running_balance),
        ],
      };
    }),
  ];

  return buildWorksheet(rows, ["text", "text", "text", "text", "text", "debit", "credit", "balance"], lang, options);
}

function Amount({ value, lang, className = "" }) {
  return (
    <span
      className={`dashboard-amount ${getAmountTone(value)} ${className}`.trim()}
      style={getAmountTextStyle(value)}
    >
      {formatAmount(value, lang)}
    </span>
  );
}

function DashboardBanner({ tone = "warning", children }) {
  if (!children) return null;
  return <div className={`dashboard-banner ${tone}`}>{children}</div>;
}

function percentFromCounts(loaded, total, complete = false) {
  if (complete) return 100;
  if (!Number.isFinite(total) || total <= 0) return 0;
  return Math.max(0, Math.min(99, Math.floor((Number(loaded || 0) / Number(total)) * 100)));
}

function DashboardProgress({ loaded = 0, total = null, percent = 0, label }) {
  const safePercent = Math.max(0, Math.min(100, Number(percent) || 0));
  const detail = Number.isFinite(total) && total > 0
    ? `${Number(loaded || 0).toLocaleString()} / ${Number(total).toLocaleString()} · ${safePercent}%`
    : `${safePercent}%`;

  return (
    <div className="dashboard-progress" aria-label={label}>
      <div className="dashboard-progress-track">
        <div className="dashboard-progress-fill" style={{ width: `${safePercent}%` }} />
      </div>
      <div className="dashboard-progress-text">{label ? `${label} · ${detail}` : detail}</div>
    </div>
  );
}

function DashboardLoader({ label, progress = null }) {
  return (
    <div className="dashboard-state">
      <div className="spinner" />
      <div>{label}</div>
      {progress ? <DashboardProgress {...progress} /> : null}
    </div>
  );
}

function DashboardRefreshing({ active, label, progress = null }) {
  if (!active) return null;
  return (
    <div className="dashboard-refreshing">
      <span className="spinner" style={{ width: 12, height: 12 }} />
      <span>{label}</span>
      {progress ? <DashboardProgress {...progress} /> : null}
    </div>
  );
}

function DashboardEmpty({ title, detail }) {
  return (
    <div className="dashboard-empty">
      <svg width="34" height="34" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
        <rect x="3" y="4" width="18" height="16" rx="2" />
        <path d="M7 9h10M7 13h10M7 17h6" />
      </svg>
      <strong>{title}</strong>
      {detail ? <span>{detail}</span> : null}
    </div>
  );
}

function DashboardCard({ label, value, lang, accent, subtitle, trend }) {
  return (
    <div className="dashboard-kpi-card" style={{ borderLeftColor: accent }}>
      <div className="dashboard-kpi-label">{label}</div>
      <div className="dashboard-kpi-value-row">
        <Amount value={value} lang={lang} className="dashboard-kpi-value" />
        {trend ? <span className="dashboard-kpi-trend">{trend}</span> : null}
      </div>
      {subtitle ? <div className="dashboard-kpi-subtitle">{subtitle}</div> : null}
    </div>
  );
}

function VirtualRows({ rows, rowHeight = 36, renderRow }) {
  const bodyRef = useRef(null);
  const [viewportHeight, setViewportHeight] = useState(560);
  const [scrollTop, setScrollTop] = useState(0);

  useEffect(() => {
    const node = bodyRef.current;
    if (!node) return undefined;

    const updateSize = () => setViewportHeight(node.clientHeight || 560);
    updateSize();

    const observer = new ResizeObserver(updateSize);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  const totalHeight = rows.length * rowHeight;
  const overscan = 10;
  const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const visibleCount = Math.ceil(viewportHeight / rowHeight) + overscan * 2;
  const end = Math.min(rows.length, start + visibleCount);
  const offsetTop = start * rowHeight;

  return (
    <div
      className="dashboard-virtual-body"
      ref={bodyRef}
      onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
    >
      <div style={{ height: totalHeight, position: "relative" }}>
        <div style={{ transform: `translateY(${offsetTop}px)` }}>
          {rows.slice(start, end).map((row, index) => renderRow(row, start + index))}
        </div>
      </div>
    </div>
  );
}

function VirtualPagedRows({
  rowCount,
  rowsByIndex,
  rowHeight = 36,
  renderRow,
  renderPlaceholder,
  loadRange,
}) {
  const bodyRef = useRef(null);
  const [viewportHeight, setViewportHeight] = useState(560);
  const [scrollTop, setScrollTop] = useState(0);

  useEffect(() => {
    const node = bodyRef.current;
    if (!node) return undefined;

    const updateSize = () => setViewportHeight(node.clientHeight || 560);
    updateSize();

    const observer = new ResizeObserver(updateSize);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  const totalHeight = Math.max(0, rowCount) * rowHeight;
  const overscan = 20;
  const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const visibleCount = Math.ceil(viewportHeight / rowHeight) + overscan * 2;
  const end = Math.min(rowCount, start + visibleCount);
  const offsetTop = start * rowHeight;

  useEffect(() => {
    if (rowCount <= 0) return;
    loadRange(start, end);
  }, [end, loadRange, rowCount, start]);

  const rendered = [];
  for (let index = start; index < end; index += 1) {
    const row = rowsByIndex.get(index);
    rendered.push(row ? renderRow(row, index) : renderPlaceholder(index));
  }

  return (
    <div
      className="dashboard-virtual-body"
      ref={bodyRef}
      onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
    >
      <div style={{ height: totalHeight, position: "relative" }}>
        <div style={{ transform: `translateY(${offsetTop}px)` }}>
          {rendered}
        </div>
      </div>
    </div>
  );
}

function LettrageDot({ value }) {
  const text = String(value ?? "").trim();
  const tone = !text ? "empty" : text.length > 1 ? "matched" : "partial";
  return (
    <span className={`dashboard-lettrage ${tone}`}>
      <span className="dot" />
      {text || "-"}
    </span>
  );
}

function LettrageBadge({ value }) {
  const text = String(value ?? "").trim().toUpperCase();
  const tone = text === "A" ? "matched" : text === "B" ? "info" : text === "H" || !text ? "empty" : "partial";
  return <span className={`dashboard-badge ${tone}`}>{text || "-"}</span>;
}

function balanceIndicator(row) {
  const debit = toNumber(row.mvt_debit);
  const credit = toNumber(row.mvt_credit);
  const total = Math.max(Math.abs(debit) + Math.abs(credit), 1);
  const debitPct = Math.max(12, (Math.abs(debit) / total) * 100);
  const creditPct = Math.max(12, (Math.abs(credit) / total) * 100);
  return { debitPct, creditPct };
}

function useDashboardQuery(fetcher, deps, enabled = true) {
  const [state, setState] = useState({
    loading: enabled,
    error: null,
    warning: null,
    data: null,
  });

  useEffect(() => {
    if (!enabled) return undefined;

    let cancelled = false;
    setState((current) => ({
      ...current,
      loading: true,
      error: null,
    }));

    fetcher()
      .then((response) => {
        if (cancelled) return;
        setState({
          loading: false,
          error: null,
          warning: response?.warning ?? null,
          data: response?.data ?? null,
        });
      })
      .catch((error) => {
        if (cancelled) return;
        setState((current) => ({
          ...current,
          loading: false,
          error: String(error),
        }));
      });

    return () => {
      cancelled = true;
    };
  }, deps);

  return state;
}

function useStreamingDashboardQuery(fetcher, deps, enabled = true) {
  const [state, setState] = useState({
    loading: enabled,
    error: null,
    warning: null,
    data: null,
    total: null,
    loaded: 0,
    percent: 0,
    retained: 0,
    limited: false,
    complete: !enabled,
  });

  useEffect(() => {
    if (!enabled) return undefined;

    let cancelled = false;
    setState((current) => ({
      ...current,
      loading: true,
      error: null,
      warning: null,
      total: null,
      data: null,
      loaded: 0,
      percent: 0,
      retained: 0,
      limited: false,
      complete: false,
    }));

    fetcher({
      onTotal: ({ total, warning }) => {
        if (cancelled) return;
        setState((current) => ({
          ...current,
          loading: true,
          warning: warning ?? current.warning,
          total,
          loaded: 0,
          percent: 0,
          complete: false,
        }));
      },
      onRows: (rows, meta = {}) => {
        if (cancelled) return;
        setState((current) => {
          const total = meta.total ?? current.total;
          return {
            ...current,
            loading: true,
            warning: meta.warning ?? current.warning,
            total,
            loaded: meta.loaded ?? rows.length,
            percent: percentFromCounts(meta.loaded ?? rows.length, total, false),
            complete: false,
            data: rows,
            retained: meta.retained ?? rows.length,
            limited: Boolean(meta.limited),
          };
        });
      },
    })
      .then((response) => {
        if (cancelled) return;
        const limited = Boolean(response?.limited);
        const loaded = response?.loaded ?? response?.data?.length ?? 0;
        const total = response?.total ?? null;
        setState({
          loading: false,
          error: null,
          warning: response?.warning ?? null,
          total,
          loaded,
          percent: limited ? percentFromCounts(loaded, total, false) : 100,
          complete: true,
          data: response?.data ?? [],
          retained: response?.retained ?? response?.data?.length ?? 0,
          limited,
        });
      })
      .catch((error) => {
        if (cancelled) return;
        setState((current) => ({
          ...current,
          loading: false,
          complete: false,
          error: String(error),
        }));
      });

    return () => {
      cancelled = true;
    };
  }, deps);

  return state;
}

function usePagedDashboardQuery(fetcher, deps, enabled = true) {
  const rowsRef = useRef(new Map());
  const pagesRef = useRef(new Set());
  const pendingRef = useRef(new Set());
  const pageOrderRef = useRef([]);
  const tokenRef = useRef(0);
  const [state, setState] = useState({
    initialLoading: enabled,
    loading: false,
    error: null,
    warning: null,
    total: null,
    loaded: 0,
    rowsByIndex: new Map(),
    complete: !enabled,
  });

  const resetCache = useCallback(() => {
    rowsRef.current = new Map();
    pagesRef.current = new Set();
    pendingRef.current = new Set();
    pageOrderRef.current = [];
  }, []);

  const loadPage = useCallback((pageIndex) => {
    if (!enabled || pageIndex < 0 || pagesRef.current.has(pageIndex) || pendingRef.current.has(pageIndex)) {
      return Promise.resolve();
    }

    const requestToken = tokenRef.current;
    pendingRef.current.add(pageIndex);
    setState((current) => ({
      ...current,
      loading: true,
      error: null,
    }));

    const offset = pageIndex * LEDGER_PAGE_SIZE;
    return fetcher(offset, LEDGER_PAGE_SIZE)
      .then((response) => {
        if (requestToken !== tokenRef.current) return;
        const pageOffset = Number(response?.offset ?? offset);
        const rows = Array.isArray(response?.data) ? response.data : [];

        rows.forEach((row, rowIndex) => {
          rowsRef.current.set(pageOffset + rowIndex, row);
        });
        pagesRef.current.add(pageIndex);
        pageOrderRef.current.push(pageIndex);

        while (pageOrderRef.current.length > LEDGER_CACHE_PAGES) {
          const evicted = pageOrderRef.current.shift();
          if (evicted == null) break;
          pagesRef.current.delete(evicted);
          const evictedOffset = evicted * LEDGER_PAGE_SIZE;
          for (let rowIndex = evictedOffset; rowIndex < evictedOffset + LEDGER_PAGE_SIZE; rowIndex += 1) {
            rowsRef.current.delete(rowIndex);
          }
        }

        setState({
          initialLoading: false,
          loading: pendingRef.current.size > 1,
          error: null,
          warning: response?.warning ?? null,
          total: Number(response?.total ?? 0),
          loaded: rowsRef.current.size,
          rowsByIndex: new Map(rowsRef.current),
          complete: true,
        });
      })
      .catch((error) => {
        if (requestToken !== tokenRef.current) return;
        setState((current) => ({
          ...current,
          initialLoading: false,
          loading: false,
          complete: false,
          error: String(error),
        }));
      })
      .finally(() => {
        pendingRef.current.delete(pageIndex);
        if (requestToken === tokenRef.current) {
          setState((current) => ({
            ...current,
            loading: pendingRef.current.size > 0,
          }));
        }
      });
  }, deps);

  const loadRange = useCallback((start, end) => {
    const firstPage = Math.floor(Math.max(0, start) / LEDGER_PAGE_SIZE);
    const lastPage = Math.floor(Math.max(0, end - 1) / LEDGER_PAGE_SIZE);
    for (let pageIndex = firstPage; pageIndex <= lastPage; pageIndex += 1) {
      loadPage(pageIndex);
    }
  }, [loadPage]);

  useEffect(() => {
    tokenRef.current += 1;
    resetCache();

    if (!enabled) {
      setState({
        initialLoading: false,
        loading: false,
        error: null,
        warning: null,
        total: null,
        loaded: 0,
        rowsByIndex: new Map(),
        complete: true,
      });
      return undefined;
    }

    setState({
      initialLoading: true,
      loading: true,
      error: null,
      warning: null,
      total: null,
      loaded: 0,
      rowsByIndex: new Map(),
      complete: false,
    });
    loadPage(0);

    return () => {
      tokenRef.current += 1;
      resetCache();
    };
  }, [enabled, loadPage, resetCache]);

  return {
    ...state,
    loadRange,
  };
}

function SectionExportButton({ onClick, disabled, busy, label }) {
  const { canStartExport } = useExportJobs();
  return (
    <button className="btn btn-accent" onClick={onClick} disabled={disabled || busy || !canStartExport}>
      {busy ? <><span className="spinner" style={{ width: 12, height: 12 }} /> {label}</> : label}
    </button>
  );
}

function OverviewTab({ state, t, lang, onExport, exporting }) {
  if (state.loading && !state.data) {
    return <DashboardLoader label={t("dashboard_loading_kpis")} />;
  }

  if (state.error) {
    return <DashboardBanner tone="error">{state.error}</DashboardBanner>;
  }

  const data = state.data ?? {};
  const topCharges = data.top_charges ?? [];
  const topProduits = data.top_produits ?? [];
  const evolution = (data.evolution_mensuelle ?? []).map((item) => ({
    month: item.month,
    ventes: toNumber(item.ventes),
    achats: toNumber(item.achats),
  }));

  const compareAccounts = new Map();
  topCharges.forEach((item) => {
    compareAccounts.set(`charge-${item.account}`, {
      name: item.account,
      label: item.label,
      charges: toNumber(item.amount),
      produits: 0,
    });
  });
  topProduits.forEach((item) => {
    const key = compareAccounts.has(`charge-${item.account}`) ? `charge-${item.account}` : `produit-${item.account}`;
    const current = compareAccounts.get(key) ?? {
      name: item.account,
      label: item.label,
      charges: 0,
      produits: 0,
    };
    current.produits = toNumber(item.amount);
    current.label = current.label || item.label;
    compareAccounts.set(key, current);
  });

  const compareData = [...compareAccounts.values()];

  return (
    <div className="dashboard-stack">
      <DashboardBanner tone="warning">{state.warning}</DashboardBanner>
      <DashboardRefreshing active={state.loading && !!state.data} label={t("loading")} />

      <div className="dashboard-section-toolbar">
        <div className="dashboard-section-caption">{t("dashboard_overview_export_hint")}</div>
        <div className="toolbar-spacer" />
        <SectionExportButton
          onClick={onExport}
          disabled={!state.data}
          busy={exporting}
          label={t("dashboard_export_tab")}
        />
      </div>

      <div className="dashboard-kpi-grid">
        <DashboardCard
          label={t("dashboard_kpi_revenue")}
          value={data.total_ventes}
          lang={lang}
          accent="var(--green)"
          trend="^"
        />
        <DashboardCard
          label={t("dashboard_kpi_purchases")}
          value={data.total_achats}
          lang={lang}
          accent="var(--red)"
        />
        <DashboardCard
          label={t("dashboard_kpi_clients")}
          value={data.solde_clients}
          lang={lang}
          accent="var(--blue)"
          subtitle={t("dashboard_subtitle_outstanding")}
        />
        <DashboardCard
          label={t("dashboard_kpi_suppliers")}
          value={data.solde_fournisseurs}
          lang={lang}
          accent="var(--orange)"
        />
      </div>

      <div className="dashboard-chart-grid">
        <div className="dashboard-card">
          <div className="dashboard-card-title">{t("dashboard_chart_evolution")}</div>
          <div className="dashboard-chart">
            <ResponsiveContainer width="100%" height="100%">
              <LineChart data={evolution}>
                <CartesianGrid stroke="var(--border)" strokeDasharray="3 3" />
                <XAxis dataKey="month" stroke="var(--text-mid)" tickLine={false} axisLine={false} />
                <YAxis
                  stroke="var(--text-mid)"
                  tickLine={false}
                  axisLine={false}
                  tickFormatter={(value) => formatAmount(value, lang)}
                />
                <Tooltip
                  formatter={(value) => formatAmount(value, lang)}
                  contentStyle={{
                    background: "var(--bg-elevated)",
                    border: "1px solid var(--border-mid)",
                    borderRadius: "10px",
                    color: "var(--text-hi)",
                  }}
                />
                <Legend />
                <Line type="monotone" dataKey="ventes" stroke="var(--green)" strokeWidth={2.5} dot={false} name={t("dashboard_sales")} />
                <Line type="monotone" dataKey="achats" stroke="var(--red)" strokeWidth={2.5} dot={false} name={t("dashboard_purchases")} />
              </LineChart>
            </ResponsiveContainer>
          </div>
        </div>

        <div className="dashboard-card">
          <div className="dashboard-card-title">{t("dashboard_chart_compare")}</div>
          <div className="dashboard-chart">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={compareData}>
                <CartesianGrid stroke="var(--border)" strokeDasharray="3 3" />
                <XAxis dataKey="name" stroke="var(--text-mid)" tickLine={false} axisLine={false} />
                <YAxis
                  stroke="var(--text-mid)"
                  tickLine={false}
                  axisLine={false}
                  tickFormatter={(value) => formatAmount(value, lang)}
                />
                <Tooltip
                  formatter={(value) => formatAmount(value, lang)}
                  contentStyle={{
                    background: "var(--bg-elevated)",
                    border: "1px solid var(--border-mid)",
                    borderRadius: "10px",
                    color: "var(--text-hi)",
                  }}
                />
                <Legend />
                <Bar dataKey="charges" fill="var(--red)" radius={[4, 4, 0, 0]} name={t("dashboard_top_charges")} />
                <Bar dataKey="produits" fill="var(--green)" radius={[4, 4, 0, 0]} name={t("dashboard_top_products")} />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </div>
      </div>

      <div className="dashboard-mini-grid">
        <div className="dashboard-card">
          <div className="dashboard-card-title">{t("dashboard_top_charges")}</div>
          <table className="dashboard-mini-table">
            <thead>
              <tr>
                <th>{t("dashboard_account")}</th>
                <th>{t("dashboard_label")}</th>
                <th>{t("dashboard_amount")}</th>
              </tr>
            </thead>
            <tbody>
              {topCharges.map((item) => (
                <tr key={`charge-${item.account}`}>
                  <td>{item.account}</td>
                  <td>{item.label}</td>
                  <td><Amount value={item.amount} lang={lang} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div className="dashboard-card">
          <div className="dashboard-card-title">{t("dashboard_top_products")}</div>
          <table className="dashboard-mini-table">
            <thead>
              <tr>
                <th>{t("dashboard_account")}</th>
                <th>{t("dashboard_label")}</th>
                <th>{t("dashboard_amount")}</th>
              </tr>
            </thead>
            <tbody>
              {topProduits.map((item) => (
                <tr key={`produit-${item.account}`}>
                  <td>{item.account}</td>
                  <td>{item.label}</td>
                  <td><Amount value={item.amount} lang={lang} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}

function GrandLivreTab({ state, t, lang, search, onSearchChange, onExport, exporting, exportProgress }) {
  const rowCount = Number(state.total ?? 0);
  const exportReady = !state.initialLoading && state.complete && !state.error;

  if (state.initialLoading) {
    return <DashboardLoader label={t("dashboard_loading_gl")} />;
  }

  if (state.error) {
    return <DashboardBanner tone="error">{state.error}</DashboardBanner>;
  }

  return (
    <div className="dashboard-stack">
      <DashboardBanner tone="warning">{state.warning}</DashboardBanner>
      <DashboardBanner tone="info">{t("dashboard_full_range_rows", rowCount)}</DashboardBanner>
      <DashboardRefreshing active={state.loading} label={t("loading")} />
      {exporting && exportProgress ? (
        <DashboardProgress
          loaded={exportProgress.loaded_rows}
          total={exportProgress.total_rows}
          percent={exportProgress.percent}
          label={t(
            "dashboard_export_progress",
            exportProgress.total_parts > 1
              ? `${exportProgress.current_part}/${exportProgress.total_parts} · ${exportProgress.current_sheet}`
              : exportProgress.current_sheet,
          )}
        />
      ) : null}

      <div className="dashboard-section-toolbar">
        <input
          className="dashboard-input"
          value={search}
          onChange={(event) => startTransition(() => onSearchChange(event.target.value))}
          placeholder={t("dashboard_gl_search_ph")}
        />
        <div className="toolbar-spacer" />
        <SectionExportButton
          onClick={onExport}
          disabled={!exportReady}
          busy={exporting}
          label={t("dashboard_export_tab")}
        />
      </div>

      {!rowCount ? (
        <DashboardEmpty title={t("dashboard_no_data")} detail={t("dashboard_no_matches")} />
      ) : (
        <div className="dashboard-ledger">
          <div className="dashboard-ledger-head" style={{ gridTemplateColumns: GRAND_LIVRE_GRID }}>
            <div>{t("dashboard_date")}</div>
            <div>{t("dashboard_journal")}</div>
            <div>{t("dashboard_account_no")}</div>
            <div>{t("dashboard_label")}</div>
            <div>{t("dashboard_ref_piece")}</div>
            <div>{t("dashboard_matching")}</div>
            <div className="right">{t("dashboard_debit")}</div>
            <div className="right">{t("dashboard_credit")}</div>
            <div className="right">{t("dashboard_running_balance")}</div>
          </div>

          <VirtualPagedRows
            rowCount={rowCount}
            rowsByIndex={state.rowsByIndex}
            loadRange={state.loadRange}
            renderPlaceholder={(index) => (
              <div key={`loading-${index}`} className="dashboard-ledger-row alt" style={{ gridTemplateColumns: GRAND_LIVRE_GRID }}>
                <div className="dashboard-span-cell">{t("loading")}</div>
              </div>
            )}
            renderRow={(row, index) => {
              return (
                <div
                  key={`${index}-${row.account_no}-${row.date ?? ""}-${row.ref_piece ?? ""}`}
                  className={`dashboard-ledger-row ${index % 2 === 1 ? "alt" : ""}`}
                  style={{ gridTemplateColumns: GRAND_LIVRE_GRID }}
                >
                  <div>{row.date || "-"}</div>
                  <div>{row.journal || "-"}</div>
                  <div>{row.account_no}</div>
                  <div>{row.account_label}</div>
                  <div>{row.ref_piece || "-"}</div>
                  <div><LettrageDot value={row.lettrage} /></div>
                  <div className="right"><Amount value={row.debit} lang={lang} /></div>
                  <div className="right"><Amount value={row.credit} lang={lang} /></div>
                  <div className="right"><Amount value={row.solde_cumule} lang={lang} /></div>
                </div>
              );
            }}
          />
        </div>
      )}
    </div>
  );
}

function BalanceTab({
  state,
  t,
  lang,
  search,
  onSearchChange,
  collapsedClasses,
  onToggleClass,
  onExport,
  exporting,
}) {
  const deferredSearch = useDeferredValue(search);
  const { filtered, groups } = useMemo(
    () => buildBalanceGroups(state.data ?? [], deferredSearch),
    [state.data, deferredSearch],
  );

  const grandTotal = useMemo(
    () =>
      filtered.reduce(
        (totals, row) => ({
          ouverture_debit: totals.ouverture_debit + toNumber(row.ouverture_debit),
          ouverture_credit: totals.ouverture_credit + toNumber(row.ouverture_credit),
          mvt_debit: totals.mvt_debit + toNumber(row.mvt_debit),
          mvt_credit: totals.mvt_credit + toNumber(row.mvt_credit),
          cloture_debit: totals.cloture_debit + toNumber(row.cloture_debit),
          cloture_credit: totals.cloture_credit + toNumber(row.cloture_credit),
        }),
        {
          ouverture_debit: 0,
          ouverture_credit: 0,
          mvt_debit: 0,
          mvt_credit: 0,
          cloture_debit: 0,
          cloture_credit: 0,
        },
      ),
    [filtered],
  );

  if (state.loading && !state.data) {
    return <DashboardLoader label={t("dashboard_loading_balance")} />;
  }

  if (state.error) {
    return <DashboardBanner tone="error">{state.error}</DashboardBanner>;
  }

  return (
    <div className="dashboard-stack">
      <DashboardBanner tone="warning">{state.warning}</DashboardBanner>
      <DashboardRefreshing active={state.loading && !!state.data} label={t("loading")} />

      <div className="dashboard-section-toolbar">
        <input
          className="dashboard-input"
          value={search}
          onChange={(event) => startTransition(() => onSearchChange(event.target.value))}
          placeholder={t("dashboard_balance_search_ph")}
        />
        <div className="toolbar-spacer" />
        <SectionExportButton
          onClick={() => onExport(filtered)}
          disabled={!filtered.length}
          busy={exporting}
          label={t("dashboard_export_tab")}
        />
      </div>

      {!filtered.length ? (
        <DashboardEmpty title={t("dashboard_no_data")} detail={t("dashboard_no_matches")} />
      ) : (
        <div className="dashboard-card dashboard-balance-card">
          <div className="dashboard-balance-wrap">
            <table className="dashboard-balance-table">
              <thead>
                <tr>
                  <th>{t("dashboard_account_no")}</th>
                  <th>{t("dashboard_label")}</th>
                  <th>{t("dashboard_opening_debit")}</th>
                  <th>{t("dashboard_opening_credit")}</th>
                  <th>{t("dashboard_movement_debit")}</th>
                  <th>{t("dashboard_movement_credit")}</th>
                  <th>{t("dashboard_closing_debit")}</th>
                  <th>{t("dashboard_closing_credit")}</th>
                </tr>
              </thead>
              <tbody>
                {groups.flatMap(([classCode, rows]) => {
                  const collapsed = !!collapsedClasses[classCode];
                  const rendered = [
                    (
                      <tr key={`group-${classCode}`} className="dashboard-group-row">
                        <td colSpan={8}>
                          <button className="dashboard-group-toggle" onClick={() => onToggleClass(classCode)}>
                            <span>{collapsed ? ">" : "v"}</span>
                            {t("dashboard_class_label", classCode)}
                          </button>
                        </td>
                      </tr>
                    ),
                  ];

                  if (collapsed) return rendered;

                  rows.forEach((row) => {
                    const movement = balanceIndicator(row);
                    rendered.push(
                      <tr key={`${classCode}-${row.account_no}`}>
                        <td>{row.account_no}</td>
                        <td>{row.account_label}</td>
                        <td className="right"><Amount value={row.ouverture_debit} lang={lang} /></td>
                        <td className="right"><Amount value={row.ouverture_credit} lang={lang} /></td>
                        <td className="right">
                          <Amount value={row.mvt_debit} lang={lang} />
                          <div className="dashboard-balance-spark">
                            <span style={{ width: `${movement.debitPct}%`, background: "var(--red)" }} />
                            <span style={{ width: `${movement.creditPct}%`, background: "var(--green)" }} />
                          </div>
                        </td>
                        <td className="right"><Amount value={row.mvt_credit} lang={lang} /></td>
                        <td className="right"><Amount value={row.cloture_debit} lang={lang} /></td>
                        <td className="right"><Amount value={row.cloture_credit} lang={lang} /></td>
                      </tr>,
                    );
                  });

                  return rendered;
                })}
              </tbody>
            </table>
          </div>

          <div className="dashboard-total-bar">
            <strong>{t("dashboard_grand_total")}</strong>
            <div className="dashboard-total-grid">
              <Amount value={grandTotal.ouverture_debit} lang={lang} />
              <Amount value={grandTotal.ouverture_credit} lang={lang} />
              <Amount value={grandTotal.mvt_debit} lang={lang} />
              <Amount value={grandTotal.mvt_credit} lang={lang} />
              <Amount value={grandTotal.cloture_debit} lang={lang} />
              <Amount value={grandTotal.cloture_credit} lang={lang} />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function TiersTab({ state, t, lang, tiersType, onTypeChange, search, onSearchChange, onExport, exporting }) {
  if (state.loading && !state.data) return <DashboardLoader label={t("dashboard_loading_tiers")} />;
  if (state.error) return <DashboardBanner tone="error">{state.error}</DashboardBanner>;
  const rows = state.data ?? [];
  const possiblyLimited = rows.length >= 100;
  return (
    <div className="dashboard-stack">
      <DashboardBanner tone="warning">{state.warning}</DashboardBanner>
      {possiblyLimited ? <DashboardBanner tone="info">{t("dashboard_tiers_export_limited")}</DashboardBanner> : null}
      <DashboardRefreshing active={state.loading && !!state.data} label={t("loading")} />
      <div className="dashboard-section-toolbar">
        <select className="dashboard-input" value={tiersType} onChange={(event) => onTypeChange(event.target.value)}>
          <option value="all">{t("dashboard_all_tiers")}</option>
          <option value="clients">{t("dashboard_clients")}</option>
          <option value="fournisseurs">{t("dashboard_suppliers")}</option>
        </select>
        <input className="dashboard-input" value={search} onChange={(event) => onSearchChange(event.target.value)} placeholder={t("dashboard_tiers_search_ph")} />
        <div className="toolbar-spacer" />
        <SectionExportButton onClick={() => onExport(rows)} disabled={!rows.length || possiblyLimited} busy={exporting} label={t("dashboard_export_tab")} />
      </div>
      {!rows.length ? <DashboardEmpty title={t("dashboard_no_data")} detail={t("dashboard_no_matches")} /> : (
        <div className="dashboard-card dashboard-balance-card"><div className="dashboard-balance-wrap">
          <table className="dashboard-balance-table">
            <thead><tr><th>{t("dashboard_tiers_code")}</th><th>{t("dashboard_label")}</th><th>{t("dashboard_type")}</th><th>{t("dashboard_account_no")}</th><th>{t("dashboard_debit")}</th><th>{t("dashboard_credit")}</th><th>{t("dashboard_running_balance")}</th><th>{t("dashboard_last_movement")}</th></tr></thead>
            <tbody>{rows.map((row) => <tr key={`${row.code}-${row.tiers_type}`}><td>{row.code}</td><td>{row.name}</td><td>{row.tiers_type || "unknown"}</td><td>{row.account_no || "-"}</td><td className="right">{row.debit == null ? "-" : <Amount value={row.debit} lang={lang} />}</td><td className="right">{row.credit == null ? "-" : <Amount value={row.credit} lang={lang} />}</td><td className="right">{row.balance == null ? "-" : <Amount value={row.balance} lang={lang} />}</td><td>{row.last_movement_date || "-"}</td></tr>)}</tbody>
          </table>
        </div></div>
      )}
    </div>
  );
}

function AuxiliaireTab({
  state,
  t,
  lang,
  tiersSearch,
  onTiersSearchChange,
  selectedTier,
  onSelectTier,
  title,
  onExport,
  exporting,
}) {
  const deferredSearch = useDeferredValue(tiersSearch);
  const tiers = useMemo(() => buildTierItems(state.data ?? []), [state.data]);
  const visibleTiers = useMemo(
    () =>
      tiers.filter((tier) =>
        !deferredSearch || normalizeText(`${tier.tiers_code} ${tier.tiers_name}`).includes(normalizeText(deferredSearch))
      ),
    [tiers, deferredSearch],
  );
  const visibleRows = useMemo(
    () => buildAuxiliaryDisplayRows(state.data ?? [], selectedTier),
    [state.data, selectedTier],
  );
  const maxSolde = useMemo(
    () => Math.max(1, ...visibleTiers.map((tier) => Math.abs(toNumber(tier.solde)))),
    [visibleTiers],
  );
  const progress = {
    loaded: state.loaded ?? 0,
    total: state.total,
    percent: state.percent ?? percentFromCounts(state.loaded, state.total, state.complete),
    label: t("dashboard_streaming_progress"),
  };
  const exportDisabled = state.loading || state.limited || !visibleRows.length;

  if (state.loading && !state.data) {
    return <DashboardLoader label={title} progress={progress} />;
  }

  if (state.error) {
    return <DashboardBanner tone="error">{state.error}</DashboardBanner>;
  }

  return (
    <div className="dashboard-stack">
      <DashboardBanner tone="warning">{state.warning}</DashboardBanner>
      {state.limited ? (
        <DashboardBanner tone="info">{t("dashboard_aux_preview_limited", state.retained, state.total)}</DashboardBanner>
      ) : null}
      <DashboardRefreshing active={state.loading && !!state.data} label={t("loading")} progress={progress} />

      <div className="dashboard-section-toolbar">
        <div className="dashboard-section-caption">{t("dashboard_aux_export_hint")}</div>
        <div className="toolbar-spacer" />
        <SectionExportButton
          onClick={() => onExport(visibleRows)}
          disabled={exportDisabled}
          busy={exporting}
          label={t("dashboard_export_tab")}
        />
      </div>

      <div className="dashboard-aux-layout">
        <aside className="dashboard-aux-sidebar">
          <input
            className="dashboard-input"
            value={tiersSearch}
            onChange={(event) => startTransition(() => onTiersSearchChange(event.target.value))}
            placeholder={t("dashboard_tiers_search_ph")}
          />

          <button
            className={`dashboard-tier-item ${!selectedTier ? "active" : ""}`}
            onClick={() => onSelectTier(null)}
          >
            <div>
              <strong>{t("dashboard_all_tiers")}</strong>
              <span>{visibleRows.filter((row) => row.type === "entry").length}</span>
            </div>
          </button>

          <div className="dashboard-tier-list">
            {visibleTiers.map((tier) => {
              const width = `${(Math.abs(toNumber(tier.solde)) / maxSolde) * 100}%`;
              return (
                <button
                  key={tier.tiers_code}
                  className={`dashboard-tier-item ${selectedTier === tier.tiers_code ? "active" : ""}`}
                  onClick={() => onSelectTier(tier.tiers_code)}
                >
                  <div className="dashboard-tier-top">
                    <strong>{tier.tiers_code}</strong>
                    <Amount value={tier.solde} lang={lang} />
                  </div>
                  <div className="dashboard-tier-name">{tier.tiers_name || tier.account_label || "-"}</div>
                  <div className="dashboard-tier-meta">
                    <span>{formatAmount(tier.total_debit, lang)}</span>
                    <span>{formatAmount(tier.total_credit, lang)}</span>
                  </div>
                  <div className="dashboard-tier-bar">
                    <span style={{ width, background: toNumber(tier.solde) >= 0 ? "var(--green)" : "var(--red)" }} />
                  </div>
                </button>
              );
            })}
          </div>
        </aside>

        <div className="dashboard-ledger">
          <div className="dashboard-ledger-head" style={{ gridTemplateColumns: AUX_GRID }}>
            <div>{t("dashboard_date")}</div>
            <div>{t("dashboard_journal")}</div>
            <div>{t("dashboard_label")}</div>
            <div>{t("dashboard_ref_piece")}</div>
            <div>{t("dashboard_matching")}</div>
            <div className="right">{t("dashboard_debit")}</div>
            <div className="right">{t("dashboard_credit")}</div>
            <div className="right">{t("dashboard_running_balance")}</div>
          </div>

          {!visibleRows.length ? (
            <DashboardEmpty title={t("dashboard_no_data")} detail={t("dashboard_select_tier")} />
          ) : (
            <VirtualRows
              rows={visibleRows}
              renderRow={(row) => {
                if (row.type === "account-header") {
                  return (
                    <div key={row.key} className="dashboard-ledger-row account-header-row" style={{ gridTemplateColumns: AUX_GRID }}>
                      <div className="dashboard-span-cell">{row.account_no} - {row.account_label}</div>
                    </div>
                  );
                }

                if (row.type === "tiers-header") {
                  return (
                    <div key={row.key} className="dashboard-ledger-row tiers-header-row" style={{ gridTemplateColumns: AUX_GRID }}>
                      <div className="dashboard-span-cell">{row.tiers_code} - {row.tiers_name || "-"}</div>
                    </div>
                  );
                }

                if (row.type === "total") {
                  return (
                    <div key={row.key} className="dashboard-ledger-row subtotal-row" style={{ gridTemplateColumns: AUX_GRID }}>
                      <div>{t("dashboard_total")}</div>
                      <div />
                      <div>{row.tiers_name || row.tiers_code}</div>
                      <div />
                      <div />
                      <div className="right"><Amount value={row.debit} lang={lang} /></div>
                      <div className="right"><Amount value={row.credit} lang={lang} /></div>
                      <div className="right"><Amount value={row.running_balance} lang={lang} /></div>
                    </div>
                  );
                }

                return (
                  <div
                    key={row.key}
                    className={`dashboard-ledger-row ${row.alt ? "alt" : ""}`}
                    style={{ gridTemplateColumns: AUX_GRID }}
                  >
                    <div>{row.date || "-"}</div>
                    <div>{row.journal || "-"}</div>
                    <div>{row.description || "-"}</div>
                    <div>{row.ref_piece || "-"}</div>
                    <div><LettrageBadge value={row.lettrage} /></div>
                    <div className="right"><Amount value={row.debit} lang={lang} /></div>
                    <div className="right"><Amount value={row.credit} lang={lang} /></div>
                    <div className="right"><Amount value={row.running_balance} lang={lang} /></div>
                  </div>
                );
              }}
            />
          )}
        </div>
      </div>
    </div>
  );
}

export default function Dashboard({
  connId,
  connectionName,
  databaseName,
  t,
  lang,
  adminMode,
  onBackToTables,
  onOpenSettings,
}) {
  const { activeJobs, beginLocalJob, updateLocalJob } = useExportJobs();
  const range = useMemo(() => getDefaultRange(), []);
  const [activeTab, setActiveTab] = useState(TAB_IDS.OVERVIEW);
  const [dateFrom, setDateFrom] = useState(range.dateFrom);
  const [dateTo, setDateTo] = useState(range.dateTo);
  const [grandLivreSearch, setGrandLivreSearch] = useState("");
  const [balanceSearch, setBalanceSearch] = useState("");
  const [supplierSearch, setSupplierSearch] = useState("");
  const [clientSearch, setClientSearch] = useState("");
  const [tiersSearch, setTiersSearch] = useState("");
  const [tiersType, setTiersType] = useState("all");
  const [fiscalSettings, setFiscalSettings] = useState({ fiscal_year_start_month: 1, fiscal_year_start_day: 1 });
  const [periodPreset, setPeriodPreset] = useState("custom");
  const [monthPreset, setMonthPreset] = useState("all");
  const [selectedSupplierTier, setSelectedSupplierTier] = useState(null);
  const [selectedClientTier, setSelectedClientTier] = useState(null);
  const [collapsedClasses, setCollapsedClasses] = useState({});
  const [exportingWorkbook, setExportingWorkbook] = useState(false);
  const [exportError, setExportError] = useState(null);
  const [grandLivreExportProgress, setGrandLivreExportProgress] = useState(null);
  const [refreshTick, setRefreshTick] = useState(0);
  const [exploitationExportData, setExploitationExportData] = useState(null);
  const deferredGrandLivreSearch = useDeferredValue(grandLivreSearch);
  const deferredTiersSearch = useDeferredValue(tiersSearch);
  const tabExportIsActive = useCallback((tabId) => activeJobs.some(
    (job) => job.dedupe_key === `dashboard:${connId}:${tabId}:${dateFrom}:${dateTo}`,
  ), [activeJobs, connId, dateFrom, dateTo]);
  const globalExportIsActive = activeJobs.some(
    (job) => job.dedupe_key === `dashboard-global:${connId}:${dateFrom}:${dateTo}`,
  );
  const exportsAtCapacity = activeJobs.length >= 2;
  const grandLivreExportIsActive = activeJobs.some((job) => job.dedupe_key === "grand-livre-global");

  useEffect(() => {
    getSettings()
      .then((settings) => setFiscalSettings({
        fiscal_year_start_month: Number(settings?.fiscal_year_start_month) || 1,
        fiscal_year_start_day: Number(settings?.fiscal_year_start_day) || 1,
      }))
      .catch(() => setFiscalSettings({ fiscal_year_start_month: 1, fiscal_year_start_day: 1 }));
  }, []);

  const handleRefresh = useCallback(() => {
    setExportError(null);
    setRefreshTick((current) => current + 1);
  }, []);

  const overviewState = useDashboardQuery(
    () => getDashboardKpis(connId, dateFrom, dateTo, null),
    [connId, dateFrom, dateTo, refreshTick],
    !!connId,
  );

  const grandLivreState = usePagedDashboardQuery(
    (offset, limit) => getGrandLivrePage(connId, dateFrom, dateTo, null, offset, limit, deferredGrandLivreSearch),
    [connId, dateFrom, dateTo, activeTab, refreshTick, deferredGrandLivreSearch],
    !!connId && activeTab === TAB_IDS.GRAND_LIVRE,
  );

  const balanceState = useDashboardQuery(
    () => getBalance(connId, dateFrom, dateTo, null),
    [connId, dateFrom, dateTo, activeTab, refreshTick],
    !!connId && activeTab === TAB_IDS.BALANCE,
  );

  const tiersState = useDashboardQuery(
    () => getDashboardTiers(connId, tiersType, deferredTiersSearch),
    [connId, tiersType, deferredTiersSearch, activeTab, refreshTick],
    !!connId && activeTab === TAB_IDS.TIERS,
  );

  const fournisseursState = useStreamingDashboardQuery(
    (handlers) => streamGrandLivreAuxiliaire(connId, dateFrom, dateTo, "fournisseurs", null, handlers, {
      label: "GL fournisseurs",
      previewLimit: DASHBOARD_PREVIEW_ROW_LIMIT,
      maxRetainedRows: DASHBOARD_PREVIEW_ROW_LIMIT,
    }),
    [connId, dateFrom, dateTo, activeTab, refreshTick],
    !!connId && activeTab === TAB_IDS.FOURNISSEURS,
  );

  const clientsState = useStreamingDashboardQuery(
    (handlers) => streamGrandLivreAuxiliaire(connId, dateFrom, dateTo, "clients", null, handlers, {
      label: "GL clients",
      previewLimit: DASHBOARD_PREVIEW_ROW_LIMIT,
      maxRetainedRows: DASHBOARD_PREVIEW_ROW_LIMIT,
    }),
    [connId, dateFrom, dateTo, activeTab, refreshTick],
    !!connId && activeTab === TAB_IDS.CLIENTS,
  );

  const activeTabLoading = activeTab === TAB_IDS.OVERVIEW
    ? overviewState.loading
    : activeTab === TAB_IDS.GRAND_LIVRE
      ? grandLivreState.initialLoading || grandLivreState.loading
      : activeTab === TAB_IDS.BALANCE
        ? balanceState.loading
        : activeTab === TAB_IDS.FOURNISSEURS
          ? fournisseursState.loading
          : activeTab === TAB_IDS.CLIENTS
            ? clientsState.loading
            : activeTab === TAB_IDS.COMPTE_EXPLOITATION
              ? false
              : tiersState.loading;

  useEffect(() => {
    setActiveTab(TAB_IDS.OVERVIEW);
    setGrandLivreSearch("");
    setBalanceSearch("");
    setSupplierSearch("");
    setClientSearch("");
    setTiersSearch("");
    setTiersType("all");
    setSelectedSupplierTier(null);
    setSelectedClientTier(null);
    setCollapsedClasses({});
    setExportError(null);
    setGrandLivreExportProgress(null);
  }, [connId, databaseName]);

  useEffect(() => {
    const handler = (event) => {
      if (event.key === "F5" || ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "r")) {
        event.preventDefault();
        handleRefresh();
      }
    };

    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [handleRefresh]);

  const tabs = [
    { id: TAB_IDS.OVERVIEW, label: t("dashboard_tab_overview") },
    { id: TAB_IDS.GRAND_LIVRE, label: t("dashboard_tab_grand_livre") },
    { id: TAB_IDS.BALANCE, label: t("dashboard_tab_balance") },
    { id: TAB_IDS.COMPTE_EXPLOITATION, label: t("dashboard_tab_exploitation") },
    { id: TAB_IDS.TIERS, label: t("dashboard_tab_tiers") },
    { id: TAB_IDS.FOURNISSEURS, label: t("dashboard_tab_suppliers") },
    { id: TAB_IDS.CLIENTS, label: t("dashboard_tab_clients") },
  ];

  const buildExportOptions = useCallback((sheetTitle) => ({
    title: `${sheetTitle} · ${databaseName || "Sage Bridge"}`,
    context: connectionName || "Sage Data Bridge",
    period: `${t("dashboard_export_period")} ${dateFrom} → ${dateTo}`,
    generatedAt: `${t("dashboard_export_generated")} ${new Intl.DateTimeFormat(lang === "fr" ? "fr-FR" : "en-US", {
      dateStyle: "medium",
      timeStyle: "short",
    }).format(new Date())}`,
  }), [connectionName, databaseName, dateFrom, dateTo, lang, t]);

  const fetchAllSheetsData = useCallback(async () => {
    const [overview, balance] = await Promise.all([
      overviewState.data ? Promise.resolve({ data: overviewState.data }) : getDashboardKpis(connId, dateFrom, dateTo, null),
      balanceState.data ? Promise.resolve({ data: balanceState.data }) : getBalance(connId, dateFrom, dateTo, null),
    ]);

    return {
      overview: overview?.data ?? {},
      balance: balance?.data ?? [],
    };
  }, [
    balanceState.data,
    connId,
    dateFrom,
    dateTo,
    overviewState.data,
  ]);

  const exportSheet = useCallback(async (tabId, buildSheets) => {
    const tracked = beginLocalJob(`Dashboard ${tabId}`, "dashboard-xlsx", `dashboard:${connId}:${tabId}:${dateFrom}:${dateTo}`);
    if (tracked.existing) return;
    setExportError(null);
    try {
      const filePath = await saveWorkbook(
        `dashboard-${tabId}-${dateFrom}-${dateTo}.xlsx`,
        buildSheets,
        (progress) => updateLocalJob(tracked.job.id, progress),
      );
      if (!filePath) {
        updateLocalJob(tracked.job.id, { status: "cancelled", phase: "cancelled", finished_at: new Date().toISOString() });
        return;
      }
      updateLocalJob(tracked.job.id, { status: "completed", phase: "completed", percent: 100, finished_at: new Date().toISOString() });
    } catch (error) {
      setExportError(String(error));
      updateLocalJob(tracked.job.id, { status: "failed", phase: "failed", error: String(error), finished_at: new Date().toISOString() });
      throw error;
    }
  }, [beginLocalJob, connId, dateFrom, dateTo, updateLocalJob]);

  const exportOverview = useCallback(async () => {
    if (!overviewState.data) return;
    await exportSheet(TAB_IDS.OVERVIEW, async () => [
      { name: "Vue Générale", worksheet: buildOverviewWorksheet(overviewState.data, t, lang, buildExportOptions(t("dashboard_tab_overview"))) },
    ]);
  }, [buildExportOptions, exportSheet, lang, overviewState.data, t]);

  const exportGrandLivreTab = useCallback(async () => {
    if (grandLivreState.loading || !grandLivreState.complete || grandLivreState.error) return;

    // Grand Livre currently uses shared legacy progress event names, so serialize it until
    // the backend event contract carries a job id.
    const tracked = beginLocalJob("Grand Livre XLSX", "grand-livre-xlsx", "grand-livre-global");
    if (tracked.existing) return;
    setExportError(null);
    try {
      const filePath = await save({
        defaultPath: `dashboard-${TAB_IDS.GRAND_LIVRE}-${dateFrom}-${dateTo}.xlsx`,
        filters: [{ name: "Excel Workbook", extensions: ["xlsx"] }],
      });
      if (!filePath) {
        updateLocalJob(tracked.job.id, { status: "cancelled", phase: "cancelled", finished_at: new Date().toISOString() });
        return;
      }

      setGrandLivreExportProgress({
        loaded_rows: 0,
        total_rows: grandLivreState.total ?? 0,
        percent: 0,
        current_sheet: 1,
        phase: "starting",
      });
      const outputPaths = new Set();
      await exportGrandLivreXlsx(connId, dateFrom, dateTo, null, filePath, {
        onProgress: (progress) => {
          if (progress.output_path) outputPaths.add(progress.output_path);
          setGrandLivreExportProgress(progress);
          updateLocalJob(tracked.job.id, {
            phase: progress.phase,
            percent: progress.percent,
            processed_rows: progress.loaded_rows,
            total_rows: progress.total_rows,
            output_paths: [...outputPaths],
          });
        },
        onComplete: (progress) => {
          setGrandLivreExportProgress(progress);
          updateLocalJob(tracked.job.id, { status: "completed", phase: "completed", percent: 100, finished_at: new Date().toISOString() });
        },
        onError: (error) => setExportError(String(error)),
      });
      updateLocalJob(tracked.job.id, {
        status: "completed",
        phase: "completed",
        percent: 100,
        output_paths: [...outputPaths],
        finished_at: new Date().toISOString(),
      });
    } catch (error) {
      setExportError(String(error));
      updateLocalJob(tracked.job.id, { status: "failed", phase: "failed", error: String(error), finished_at: new Date().toISOString() });
      throw error;
    } finally {
      setGrandLivreExportProgress(null);
    }
  }, [
    connId,
    beginLocalJob,
    dateFrom,
    dateTo,
    grandLivreState.complete,
    grandLivreState.error,
    grandLivreState.loading,
    grandLivreState.total,
    updateLocalJob,
  ]);

  const exportBalanceTab = useCallback(async (rows) => {
    if (!rows.length) return;
    await exportSheet(TAB_IDS.BALANCE, async () => [
      { name: "Balance", worksheet: buildBalanceWorksheet(rows, t, lang, buildExportOptions(t("dashboard_tab_balance"))) },
    ]);
  }, [buildExportOptions, exportSheet, lang, t]);

  const exportExploitationTab = useCallback(async (report) => {
    if (!report?.rows?.length) return;
    await exportSheet(TAB_IDS.COMPTE_EXPLOITATION, async () => [
      { name: "Compte exploitation", worksheet: buildExploitationWorksheet(report, t, lang, {
        ...buildExportOptions(report.title || `${t("dashboard_tab_exploitation")} ${report.year || ""}`),
        title: `${report.title || databaseName || t("dashboard_tab_exploitation")} · ${report.year || ""}`,
        period: `${t("exploitation_year")}: ${report.year || ""}`,
      }) },
    ]);
  }, [buildExportOptions, databaseName, exportSheet, lang, t]);

  const exportExploitationDetail = useCallback(async (detail) => {
    await exportSheet(`${TAB_IDS.COMPTE_EXPLOITATION}-detail`, async () => [{
      name: "Detail exploitation",
      worksheet: buildExploitationDetailWorksheet(detail, t, lang, buildExportOptions(detail.line.lineLabel)),
    }]);
  }, [buildExportOptions, exportSheet, lang, t]);

  const exportTiersTab = useCallback(async (rows) => {
    if (!rows.length) return;
    await exportSheet(TAB_IDS.TIERS, async () => {
      const sheetRows = [
        { kind: "header", values: [t("dashboard_tiers_code"), t("dashboard_label"), t("dashboard_type"), t("dashboard_account_no"), t("dashboard_debit"), t("dashboard_credit"), t("dashboard_running_balance"), t("dashboard_last_movement")] },
        ...rows.map((row) => ({ kind: "data", values: [row.code, row.name, row.tiers_type, row.account_no || "", row.debit, row.credit, row.balance, row.last_movement_date || ""] })),
      ];
      return [
        { name: "Tiers", worksheet: buildWorksheet(sheetRows, ["text", "text", "text", "text", "debit", "credit", "balance", "text"], lang, buildExportOptions(t("dashboard_tab_tiers"))) },
      ];
    });
  }, [buildExportOptions, exportSheet, lang, t]);

  const exerciseYears = useMemo(() => {
    const current = new Date().getUTCFullYear();
    return Array.from({ length: 8 }, (_, index) => current + 1 - index);
  }, []);

  const applyExercise = useCallback((value) => {
    setPeriodPreset(value);
    setMonthPreset("all");
    if (value === "custom") return;
    const next = getFiscalRange(Number(value), fiscalSettings.fiscal_year_start_month, fiscalSettings.fiscal_year_start_day);
    setDateFrom(next.dateFrom);
    setDateTo(next.dateTo);
  }, [fiscalSettings]);

  const applyMonth = useCallback((value) => {
    setMonthPreset(value);
    if (value === "all") {
      if (periodPreset !== "custom") applyExercise(periodPreset);
      return;
    }
    const month = Number(value);
    const year = periodPreset === "custom"
      ? Number(dateFrom.slice(0, 4))
      : Number(periodPreset) + (month < fiscalSettings.fiscal_year_start_month ? 1 : 0);
    const start = new Date(Date.UTC(year, month - 1, 1));
    const end = new Date(Date.UTC(year, month, 0));
    setDateFrom(toIsoDate(start));
    setDateTo(toIsoDate(end));
  }, [applyExercise, dateFrom, fiscalSettings.fiscal_year_start_month, periodPreset]);

  const exportAuxiliaryTab = useCallback(async (tabId, displayRows, sheetName) => {
    if (!displayRows.length) return;
    await exportSheet(tabId, async () => [
      { name: sheetName, worksheet: buildAuxiliaryWorksheet(displayRows, t, lang, buildExportOptions(sheetName)) },
    ]);
  }, [buildExportOptions, exportSheet, lang, t]);

  const exportDashboardWorkbook = useCallback(async () => {
    const tracked = beginLocalJob("Global dashboard XLSX", "dashboard-xlsx", `dashboard-global:${connId}:${dateFrom}:${dateTo}`);
    if (tracked.existing) return;
    setExportingWorkbook(true);
    setExportError(null);
    try {
      const filePath = await saveWorkbook(
        `dashboard-${databaseName}-${dateFrom}-${dateTo}.xlsx`,
        async () => {
          updateLocalJob(tracked.job.id, { phase: "loading dashboard data", percent: 15 });
          const data = await fetchAllSheetsData();
          return [
            { name: "Vue Générale", worksheet: buildOverviewWorksheet(data.overview, t, lang, buildExportOptions(t("dashboard_tab_overview"))) },
            { name: "Balance", worksheet: buildBalanceWorksheet(data.balance, t, lang, buildExportOptions(t("dashboard_tab_balance"))) },
            ...(exploitationExportData?.rows?.length ? [{
              name: "Compte exploitation",
              worksheet: buildExploitationWorksheet(exploitationExportData, t, lang, {
                ...buildExportOptions(exploitationExportData.title || t("dashboard_tab_exploitation")),
                title: `${exploitationExportData.title || databaseName || t("dashboard_tab_exploitation")} · ${exploitationExportData.year || ""}`,
                period: `${t("exploitation_year")}: ${exploitationExportData.year || ""}`,
              }),
            }] : []),
            {
              name: "Notes",
              worksheet: buildWorksheet(
                [
                  { kind: "header", values: [t("dashboard_export_notes")] },
                  { kind: "data", values: [t("dashboard_full_export_limited")] },
                ],
                ["text"],
                lang,
                buildExportOptions(t("dashboard_export_notes")),
              ),
            },
          ];
        },
        (progress) => updateLocalJob(tracked.job.id, progress),
      );
      if (!filePath) {
        updateLocalJob(tracked.job.id, { status: "cancelled", phase: "cancelled", finished_at: new Date().toISOString() });
        return;
      }
      updateLocalJob(tracked.job.id, { status: "completed", phase: "completed", percent: 100, finished_at: new Date().toISOString() });
    } catch (error) {
      setExportError(String(error));
      updateLocalJob(tracked.job.id, { status: "failed", phase: "failed", error: String(error), finished_at: new Date().toISOString() });
      throw error;
    } finally {
      setExportingWorkbook(false);
    }
  }, [beginLocalJob, buildExportOptions, connId, databaseName, dateFrom, dateTo, exploitationExportData, fetchAllSheetsData, lang, t, updateLocalJob]);

  return (
    <div className="dashboard-view">
      <div className="dashboard-header">
        <div className="dashboard-header-start">
          {adminMode ? (
            <button className="dashboard-back-btn" onClick={onBackToTables}>
              {t("dashboard_back_to_tables")}
            </button>
          ) : null}
          <div className="dashboard-brand">
            <div className="dashboard-logo-mark">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M12 2L2 7l10 5 10-5-10-5z" />
                <path d="M2 17l10 5 10-5" />
                <path d="M2 12l10 5 10-5" />
              </svg>
            </div>
            <div>
              <div className="dashboard-header-eyebrow">{connectionName || "Sage Bridge"}</div>
              <div className="dashboard-header-title">{databaseName || t("dashboard_title")}</div>
            </div>
          </div>
        </div>

        <div className="dashboard-header-range">
          <label className="dashboard-field">
            <span>{t("dashboard_exercise")}</span>
            <select className="dashboard-input" value={periodPreset} onChange={(event) => applyExercise(event.target.value)}>
              <option value="custom">{t("dashboard_custom_period")}</option>
              {exerciseYears.map((year) => {
                const exercise = getFiscalRange(year, fiscalSettings.fiscal_year_start_month, fiscalSettings.fiscal_year_start_day);
                return <option key={year} value={year}>{exercise.dateFrom} - {exercise.dateTo}</option>;
              })}
            </select>
          </label>
          <label className="dashboard-field">
            <span>{t("dashboard_month")}</span>
            <select className="dashboard-input" value={monthPreset} onChange={(event) => applyMonth(event.target.value)}>
              <option value="all">{t("dashboard_all_months")}</option>
              {Array.from({ length: 12 }, (_, index) => <option key={index + 1} value={index + 1}>{String(index + 1).padStart(2, "0")}</option>)}
            </select>
          </label>
          <label className="dashboard-field">
            <span>{t("dashboard_date_from")}</span>
            <input type="date" className="dashboard-input" value={dateFrom} onChange={(event) => { setDateFrom(event.target.value); setPeriodPreset("custom"); setMonthPreset("all"); }} />
          </label>
          <label className="dashboard-field">
            <span>{t("dashboard_date_to")}</span>
            <input type="date" className="dashboard-input" value={dateTo} onChange={(event) => { setDateTo(event.target.value); setPeriodPreset("custom"); setMonthPreset("all"); }} />
          </label>
        </div>

        <div className="dashboard-header-actions">
          <button className="btn btn-ghost btn-icon dashboard-settings-trigger" onClick={onOpenSettings} title={t("sidebar_settings")}>
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <circle cx="12" cy="12" r="3" />
              <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01A1.65 1.65 0 0 0 10.09 3H10a2 2 0 1 1 4 0h-.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01A1.65 1.65 0 0 0 21 10.09V10a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
            </svg>
          </button>
          <button className="btn btn-ghost dashboard-refresh-btn" onClick={handleRefresh} title={t("toolbar_refresh")} disabled={activeTabLoading}>
            {activeTabLoading ? (
              <span className="spinner" style={{ width: 12, height: 12 }} />
            ) : (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M21 12a9 9 0 1 1-2.64-6.36" />
                <path d="M21 3v6h-6" />
              </svg>
            )}
            {activeTabLoading ? t("loading") : t("refresh")}
          </button>
          <button className="btn btn-accent dashboard-export-btn" onClick={exportDashboardWorkbook} disabled={exportingWorkbook || globalExportIsActive || exportsAtCapacity || activeTabLoading}>
            {exportingWorkbook || globalExportIsActive ? <><span className="spinner" style={{ width: 12, height: 12 }} /> {t("dashboard_exporting")}</> : t("dashboard_export_dashboard")}
          </button>
        </div>
      </div>

      {exportError ? (
        <div className="dashboard-export-error">
          <DashboardBanner tone="error">{exportError}</DashboardBanner>
        </div>
      ) : null}

      <div className="dashboard-tabbar">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            className={`dashboard-tab ${activeTab === tab.id ? "active" : ""}`}
            onClick={() => setActiveTab(tab.id)}
          >
            {tab.label}
          </button>
        ))}
      </div>

      <div className="dashboard-content">
        {activeTab === TAB_IDS.OVERVIEW ? (
          <OverviewTab
            state={overviewState}
            t={t}
            lang={lang}
            onExport={exportOverview}
            exporting={tabExportIsActive(TAB_IDS.OVERVIEW)}
          />
        ) : null}

        {activeTab === TAB_IDS.GRAND_LIVRE ? (
          <GrandLivreTab
            state={grandLivreState}
            t={t}
            lang={lang}
            search={grandLivreSearch}
            onSearchChange={setGrandLivreSearch}
            onExport={exportGrandLivreTab}
            exporting={grandLivreExportIsActive}
            exportProgress={grandLivreExportProgress}
          />
        ) : null}

        {activeTab === TAB_IDS.BALANCE ? (
          <BalanceTab
            state={balanceState}
            t={t}
            lang={lang}
            search={balanceSearch}
            onSearchChange={setBalanceSearch}
            collapsedClasses={collapsedClasses}
            onToggleClass={(classCode) =>
              setCollapsedClasses((current) => ({ ...current, [classCode]: !current[classCode] }))
            }
            onExport={exportBalanceTab}
            exporting={tabExportIsActive(TAB_IDS.BALANCE)}
          />
        ) : null}

        {activeTab === TAB_IDS.COMPTE_EXPLOITATION ? (
          <CompteExploitationTab connId={connId} databaseName={databaseName} t={t} lang={lang} refreshTick={refreshTick} onExport={exportExploitationTab} onExportDetail={exportExploitationDetail} onReportChange={setExploitationExportData} exporting={tabExportIsActive(TAB_IDS.COMPTE_EXPLOITATION)} />
        ) : null}

        {activeTab === TAB_IDS.TIERS ? (
          <TiersTab state={tiersState} t={t} lang={lang} tiersType={tiersType} onTypeChange={setTiersType} search={tiersSearch} onSearchChange={setTiersSearch} onExport={exportTiersTab} exporting={tabExportIsActive(TAB_IDS.TIERS)} />
        ) : null}

        {activeTab === TAB_IDS.FOURNISSEURS ? (
          <AuxiliaireTab
            state={fournisseursState}
            t={t}
            lang={lang}
            tiersSearch={supplierSearch}
            onTiersSearchChange={setSupplierSearch}
            selectedTier={selectedSupplierTier}
            onSelectTier={setSelectedSupplierTier}
            title={t("dashboard_loading_suppliers")}
            onExport={(rows) => exportAuxiliaryTab(TAB_IDS.FOURNISSEURS, rows, "GL Fournisseurs")}
            exporting={tabExportIsActive(TAB_IDS.FOURNISSEURS)}
          />
        ) : null}

        {activeTab === TAB_IDS.CLIENTS ? (
          <AuxiliaireTab
            state={clientsState}
            t={t}
            lang={lang}
            tiersSearch={clientSearch}
            onTiersSearchChange={setClientSearch}
            selectedTier={selectedClientTier}
            onSelectTier={setSelectedClientTier}
            title={t("dashboard_loading_clients")}
            onExport={(rows) => exportAuxiliaryTab(TAB_IDS.CLIENTS, rows, "GL Clients")}
            exporting={tabExportIsActive(TAB_IDS.CLIENTS)}
          />
        ) : null}
      </div>
    </div>
  );
}
