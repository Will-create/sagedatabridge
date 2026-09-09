import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { save } from "@tauri-apps/api/dialog";
import { BarChart3, BookOpen, Download, FileSpreadsheet, RefreshCw } from "lucide-react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Legend,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

import { useExportJobs } from "../../exportJobs";
import {
  exportAccountingJournalsXlsx,
  getAccountingJournalStats,
  getAccountingJournals,
  getSettings,
  streamAccountingJournalEntries,
} from "../../hooks/useTauri";
import { useT } from "../../i18n";
import {
  buildExerciseYears,
  filterJournalOptions,
  getFiscalRange,
  getFiscalStartYear,
  JOURNAL_TYPE_VALUES,
  journalEntryCells,
  journalTypeCounts,
  validateJournalRange,
} from "./journalModel";

const ROW_HEIGHT = 31;
const HEADER_HEIGHT = 34;
const OVERSCAN = 12;
const CHART_COLORS = ["#25d0b1", "#62a8ff", "#f6b84a", "#a78bfa", "#fb7185", "#34d399", "#f97316", "#94a3b8"];

function formatAmount(value, lang) {
  return new Intl.NumberFormat(lang === "fr" ? "fr-FR" : "en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(Number(value || 0));
}

function VirtualJournalGrid({ rows, total, loading, complete, t, lang }) {
  const viewportRef = useRef(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(480);

  useEffect(() => {
    const node = viewportRef.current;
    if (!node) return undefined;
    const update = () => setViewportHeight(node.clientHeight || 480);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (viewportRef.current) viewportRef.current.scrollTop = 0;
    setScrollTop(0);
  }, [total]);

  const start = Math.max(0, Math.floor(Math.max(0, scrollTop - HEADER_HEIGHT) / ROW_HEIGHT) - OVERSCAN);
  const visibleCount = Math.ceil(viewportHeight / ROW_HEIGHT) + (OVERSCAN * 2);
  const end = Math.min(rows.length, start + visibleCount);
  const visibleRows = rows.slice(start, end);

  if (!loading && complete && rows.length === 0) {
    return (
      <div className="journals-grid-empty">
        <FileSpreadsheet size={34} />
        <strong>{t("journals_no_entries")}</strong>
        <span>{t("journals_no_entries_hint")}</span>
      </div>
    );
  }

  return (
    <div
      className="journals-grid-scroll"
      ref={viewportRef}
      onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
      role="table"
      aria-rowcount={total || rows.length}
    >
      <div className="journals-grid-inner">
        <div className="journals-grid-row journals-grid-head" role="row">
          <span role="columnheader">#</span>
          <span role="columnheader">{t("journals_col_date")}</span>
          <span role="columnheader">{t("journals_col_code")}</span>
          <span role="columnheader">{t("journals_col_piece")}</span>
          <span role="columnheader">{t("journals_col_account")}</span>
          <span role="columnheader">{t("journals_col_label")}</span>
          <span role="columnheader" className="number">{t("journals_col_debit")}</span>
          <span role="columnheader" className="number">{t("journals_col_credit")}</span>
        </div>
        <div className="journals-grid-body" style={{ height: rows.length * ROW_HEIGHT }}>
          {visibleRows.map((entry, localIndex) => {
            const index = start + localIndex;
            const cells = journalEntryCells(entry);
            return (
              <div
                className="journals-grid-row"
                key={`${index}-${entry.date}-${entry.journal_code}-${entry.piece_number}-${entry.account_number}`}
                role="row"
                style={{ transform: `translateY(${index * ROW_HEIGHT}px)` }}
              >
                <span role="cell" className="row-number">{index + 1}</span>
                {cells.map((value, cellIndex) => (
                  <span
                    key={cellIndex}
                    role="cell"
                    className={cellIndex >= 5 ? "number" : ""}
                    title={String(value ?? "")}
                  >
                    {cellIndex >= 5 ? formatAmount(value, lang) : String(value ?? "")}
                  </span>
                ))}
              </div>
            );
          })}
        </div>
      </div>
      {loading ? (
        <div className="journals-stream-indicator">
          <span className="spinner" />
          {t("journals_streaming", rows.length, total || 0)}
        </div>
      ) : null}
    </div>
  );
}

function JournalsOverview({ stats, loading, error, t, lang, onRetry }) {
  if (loading) {
    return <div className="journals-panel-state"><span className="spinner" />{t("journals_stats_loading")}</div>;
  }
  if (error) {
    return (
      <div className="journals-error" role="alert">
        <strong>{t("journals_stats_error")}</strong>
        <span>{error}</span>
        <button className="btn btn-sm" type="button" onClick={onRetry}>{t("journals_retry")}</button>
      </div>
    );
  }
  if (!stats) return null;

  const pieData = (stats.by_journal ?? []).slice(0, 8).map((item) => ({
    name: item.journal_code,
    value: item.entry_count,
  }));
  return (
    <div className="journals-overview">
      <div className="journals-kpis">
        <article><span>{t("journals_kpi_entries")}</span><strong>{Number(stats.total_entries || 0).toLocaleString(lang === "fr" ? "fr-FR" : "en-US")}</strong></article>
        <article><span>{t("journals_kpi_debit")}</span><strong>{formatAmount(stats.total_debit, lang)}</strong></article>
        <article><span>{t("journals_kpi_credit")}</span><strong>{formatAmount(stats.total_credit, lang)}</strong></article>
        <article><span>{t("journals_kpi_balance")}</span><strong className={Number(stats.balance) < 0 ? "negative" : ""}>{formatAmount(stats.balance, lang)}</strong></article>
      </div>

      <div className="journals-charts">
        <article className="journals-chart-card journals-chart-wide">
          <header><strong>{t("journals_monthly_chart")}</strong><span>{t("journals_monthly_chart_hint")}</span></header>
          <div className="journals-chart-canvas">
            <ResponsiveContainer width="100%" height="100%">
              <BarChart data={stats.monthly ?? []} margin={{ top: 12, right: 12, left: 8, bottom: 4 }}>
                <CartesianGrid stroke="rgba(148, 163, 184, 0.14)" vertical={false} />
                <XAxis dataKey="month" tick={{ fill: "#94a3b8", fontSize: 10 }} />
                <YAxis tick={{ fill: "#94a3b8", fontSize: 10 }} tickFormatter={(value) => new Intl.NumberFormat(lang === "fr" ? "fr-FR" : "en-US", { notation: "compact" }).format(value)} />
                <Tooltip formatter={(value) => formatAmount(value, lang)} />
                <Legend />
                <Bar dataKey="debit" name={t("journals_col_debit")} fill="#25d0b1" radius={[3, 3, 0, 0]} />
                <Bar dataKey="credit" name={t("journals_col_credit")} fill="#62a8ff" radius={[3, 3, 0, 0]} />
              </BarChart>
            </ResponsiveContainer>
          </div>
        </article>

        <article className="journals-chart-card">
          <header><strong>{t("journals_distribution_chart")}</strong><span>{t("journals_distribution_chart_hint")}</span></header>
          <div className="journals-chart-canvas">
            {pieData.length ? (
              <ResponsiveContainer width="100%" height="100%">
                <PieChart>
                  <Pie data={pieData} dataKey="value" nameKey="name" innerRadius="45%" outerRadius="74%" paddingAngle={2}>
                    {pieData.map((item, index) => <Cell key={item.name} fill={CHART_COLORS[index % CHART_COLORS.length]} />)}
                  </Pie>
                  <Tooltip formatter={(value) => Number(value).toLocaleString(lang === "fr" ? "fr-FR" : "en-US")} />
                  <Legend />
                </PieChart>
              </ResponsiveContainer>
            ) : <div className="journals-chart-empty">{t("journals_no_entries")}</div>}
          </div>
        </article>
      </div>

      <article className="journals-breakdown-card">
        <header><strong>{t("journals_breakdown")}</strong></header>
        <div className="journals-breakdown-list">
          {(stats.by_journal ?? []).map((item) => (
            <div key={item.journal_code}>
              <span className="journals-code-badge">{item.journal_code}</span>
              <span className="journals-breakdown-name">{item.journal_name}</span>
              <span>{Number(item.entry_count).toLocaleString(lang === "fr" ? "fr-FR" : "en-US")}</span>
              <span className="number">{formatAmount(item.debit, lang)}</span>
              <span className="number">{formatAmount(item.credit, lang)}</span>
            </div>
          ))}
        </div>
      </article>
    </div>
  );
}

export default function JournalsView({ connId, databaseName }) {
  const { t, lang } = useT();
  const { activeJobs, beginLocalJob, updateLocalJob } = useExportJobs();
  const [activeTab, setActiveTab] = useState("overview");
  const [journals, setJournals] = useState([]);
  const [dateBounds, setDateBounds] = useState({ min: null, max: null });
  const [fiscalSettings, setFiscalSettings] = useState({ month: 1, day: 1 });
  const [periodReady, setPeriodReady] = useState(false);
  const [exercise, setExercise] = useState("custom");
  const [dateFrom, setDateFrom] = useState("");
  const [dateTo, setDateTo] = useState("");
  const [journalType, setJournalType] = useState("all");
  const [journalCode, setJournalCode] = useState("");
  const [catalogLoading, setCatalogLoading] = useState(true);
  const [catalogWarnings, setCatalogWarnings] = useState([]);
  const [catalogError, setCatalogError] = useState("");
  const [stats, setStats] = useState(null);
  const [statsLoading, setStatsLoading] = useState(false);
  const [statsError, setStatsError] = useState("");
  const [statsWarnings, setStatsWarnings] = useState([]);
  const [entries, setEntries] = useState([]);
  const [entriesTotal, setEntriesTotal] = useState(0);
  const [entriesLoading, setEntriesLoading] = useState(false);
  const [entriesComplete, setEntriesComplete] = useState(false);
  const [entryWarnings, setEntryWarnings] = useState([]);
  const [entryError, setEntryError] = useState("");
  const [exportError, setExportError] = useState("");
  const [reloadKey, setReloadKey] = useState(0);

  useEffect(() => {
    let active = true;
    setPeriodReady(false);
    setJournalType("all");
    setJournalCode("");
    getSettings()
      .then((settings) => {
        if (!active) return;
        const month = Number(settings?.fiscal_year_start_month) || 1;
        const day = Number(settings?.fiscal_year_start_day) || 1;
        const currentExercise = getFiscalStartYear(new Date(), month, day);
        const range = getFiscalRange(currentExercise, month, day);
        setFiscalSettings({ month, day });
        setExercise(String(currentExercise));
        setDateFrom(range.dateFrom);
        setDateTo(range.dateTo);
        setPeriodReady(true);
      })
      .catch(() => {
        if (!active) return;
        const currentExercise = getFiscalStartYear(new Date(), 1, 1);
        const range = getFiscalRange(currentExercise, 1, 1);
        setFiscalSettings({ month: 1, day: 1 });
        setExercise(String(currentExercise));
        setDateFrom(range.dateFrom);
        setDateTo(range.dateTo);
        setPeriodReady(true);
      });
    return () => { active = false; };
  }, [connId]);

  useEffect(() => {
    let active = true;
    setCatalogLoading(true);
    setCatalogError("");
    getAccountingJournals(connId)
      .then((response) => {
        if (!active) return;
        setJournals(response.journals ?? []);
        setDateBounds({ min: response.date_min ?? null, max: response.date_max ?? null });
        setCatalogWarnings(response.warnings ?? []);
      })
      .catch((reason) => {
        if (!active) return;
        setJournals([]);
        setDateBounds({ min: null, max: null });
        setCatalogWarnings([]);
        setCatalogError(String(reason));
      })
      .finally(() => active && setCatalogLoading(false));
    return () => { active = false; };
  }, [connId, reloadKey]);

  const rangeError = useMemo(() => validateJournalRange(dateFrom, dateTo), [dateFrom, dateTo]);
  const selectedType = journalType === "all" ? null : journalType;
  const selectedCode = journalCode || null;

  useEffect(() => {
    if (activeTab !== "overview") return undefined;
    if (!periodReady || rangeError) {
      setStats(null);
      setStatsLoading(false);
      return undefined;
    }
    let active = true;
    setStats(null);
    setStatsLoading(true);
    setStatsError("");
    const timer = window.setTimeout(() => {
      getAccountingJournalStats(connId, dateFrom, dateTo, selectedType, selectedCode)
        .then((response) => {
          if (!active) return;
          setStats(response);
          setStatsWarnings(response.warnings ?? []);
        })
        .catch((reason) => {
          if (!active) return;
          setStats(null);
          setStatsWarnings([]);
          setStatsError(String(reason));
        })
        .finally(() => active && setStatsLoading(false));
    }, 180);
    return () => { active = false; window.clearTimeout(timer); };
  }, [activeTab, connId, dateFrom, dateTo, periodReady, rangeError, reloadKey, selectedCode, selectedType]);

  useEffect(() => {
    if (activeTab !== "entries") return undefined;
    setEntries([]);
    setEntriesTotal(0);
    setEntriesComplete(false);
    setEntryError("");
    if (!periodReady || rangeError) {
      setEntriesLoading(false);
      return undefined;
    }
    let active = true;
    const requestId = crypto.randomUUID();
    setEntriesLoading(true);
    setEntryWarnings([]);
    const timer = window.setTimeout(() => {
      streamAccountingJournalEntries(
        connId,
        requestId,
        dateFrom,
        dateTo,
        selectedType,
        selectedCode,
        {
          onTotal: (payload) => {
            if (!active) return;
            setEntriesTotal(Number(payload.total ?? 0));
            setEntryWarnings(payload.warnings ?? []);
          },
          onChunk: (chunk) => {
            if (active) setEntries((current) => [...current, ...chunk]);
          },
          onComplete: (payload) => {
            if (!active) return;
            setEntryWarnings(payload.warnings ?? []);
            setEntriesComplete(true);
          },
          onError: (reason) => active && setEntryError(String(reason)),
        },
      ).catch((reason) => {
        if (active) setEntryError(String(reason));
      }).finally(() => {
        if (active) {
          setEntriesLoading(false);
          setEntriesComplete(true);
        }
      });
    }, 180);
    return () => { active = false; window.clearTimeout(timer); };
  }, [activeTab, connId, dateFrom, dateTo, periodReady, rangeError, reloadKey, selectedCode, selectedType]);

  const journalOptions = useMemo(() => filterJournalOptions(journals, journalType), [journalType, journals]);
  const typeCounts = useMemo(() => journalTypeCounts(journals), [journals]);
  const currentStartYear = getFiscalStartYear(new Date(), fiscalSettings.month, fiscalSettings.day);
  const exerciseYears = useMemo(
    () => buildExerciseYears(dateBounds.min, dateBounds.max, currentStartYear, fiscalSettings.month, fiscalSettings.day),
    [currentStartYear, dateBounds.max, dateBounds.min, fiscalSettings.day, fiscalSettings.month],
  );
  const warnings = useMemo(
    () => [...new Set([...catalogWarnings, ...statsWarnings, ...entryWarnings])],
    [catalogWarnings, entryWarnings, statsWarnings],
  );
  const exportDedupeKey = `journals:${connId}:${dateFrom}:${dateTo}:${selectedType || "all"}:${selectedCode || "all"}`;
  const exportActive = activeJobs.some((job) => job.dedupe_key === exportDedupeKey);

  const applyExercise = useCallback((value) => {
    setExercise(value);
    if (value === "custom") return;
    const range = getFiscalRange(Number(value), fiscalSettings.month, fiscalSettings.day);
    setDateFrom(range.dateFrom);
    setDateTo(range.dateTo);
  }, [fiscalSettings.day, fiscalSettings.month]);

  const handleExport = useCallback(async () => {
    if (rangeError || entriesTotal <= 0 || exportActive) return;
    setExportError("");
    const filePath = await save({
      defaultPath: `journaux-${dateFrom}-${dateTo}.xlsx`,
      filters: [{ name: "Excel Workbook", extensions: ["xlsx"] }],
    });
    if (!filePath) return;
    const tracked = beginLocalJob(t("journals_export_job"), "journals-xlsx", exportDedupeKey);
    if (tracked.existing) {
      if (tracked.capacityReached) setExportError(t("journals_export_capacity"));
      return;
    }
    const outputPaths = new Set();
    try {
      const paths = await exportAccountingJournalsXlsx(
        connId,
        crypto.randomUUID(),
        dateFrom,
        dateTo,
        selectedType,
        selectedCode,
        filePath,
        lang,
        {
          onProgress: (progress) => {
            if (progress.output_path) outputPaths.add(progress.output_path);
            updateLocalJob(tracked.job.id, {
              phase: progress.phase,
              processed_rows: progress.loaded_rows,
              total_rows: progress.total_rows,
              percent: progress.percent,
              output_paths: [...outputPaths],
            });
          },
        },
      );
      (paths ?? []).forEach((path) => outputPaths.add(path));
      updateLocalJob(tracked.job.id, {
        status: "completed",
        phase: "complete",
        percent: 100,
        output_paths: [...outputPaths],
        finished_at: new Date().toISOString(),
      });
    } catch (reason) {
      setExportError(String(reason));
      updateLocalJob(tracked.job.id, {
        status: "failed",
        phase: "failed",
        error: String(reason),
        finished_at: new Date().toISOString(),
      });
    }
  }, [beginLocalJob, connId, dateFrom, dateTo, entriesTotal, exportActive, exportDedupeKey, lang, rangeError, selectedCode, selectedType, t, updateLocalJob]);

  const displayError = rangeError ? t(`journals_date_error_${rangeError}`) : catalogError;
  const resultCount = activeTab === "overview" ? stats?.total_entries : entriesTotal;

  return (
    <div className="journals-view">
      <header className="journals-header">
        <div className="journals-brand">
          <span className="journals-logo-mark"><BookOpen size={18} /></span>
          <span><span className="journals-eyebrow">{t("journals_eyebrow")}</span><strong>{t("journals_title")}</strong><small>{databaseName}</small></span>
        </div>
        <button className="btn journals-refresh" type="button" onClick={() => setReloadKey((value) => value + 1)} disabled={catalogLoading || statsLoading || entriesLoading}>
          <RefreshCw size={13} />{t("refresh")}
        </button>
      </header>

      <nav className="journals-tabs" aria-label={t("journals_tabs_label")}>
        <button type="button" className={activeTab === "overview" ? "active" : ""} onClick={() => setActiveTab("overview")}><BarChart3 size={14} />{t("journals_tab_overview")}</button>
        <button type="button" className={activeTab === "entries" ? "active" : ""} onClick={() => setActiveTab("entries")}><FileSpreadsheet size={14} />{t("journals_tab_entries")}</button>
      </nav>

      <section className="journals-toolbar" aria-label={t("journals_filters") }>
        <label className="dashboard-field"><span>{t("dashboard_exercise")}</span><select className="dashboard-input" value={exercise} onChange={(event) => applyExercise(event.target.value)} disabled={!periodReady}><option value="custom">{t("dashboard_custom_period")}</option>{exerciseYears.map((year) => { const range = getFiscalRange(year, fiscalSettings.month, fiscalSettings.day); return <option key={year} value={year}>{range.dateFrom} - {range.dateTo}</option>; })}</select></label>
        <label className="dashboard-field"><span>{t("dashboard_date_from")}</span><input type="date" className="dashboard-input" value={dateFrom} onChange={(event) => { setDateFrom(event.target.value); setExercise("custom"); }} /></label>
        <label className="dashboard-field"><span>{t("dashboard_date_to")}</span><input type="date" className="dashboard-input" value={dateTo} onChange={(event) => { setDateTo(event.target.value); setExercise("custom"); }} /></label>
        <label className="dashboard-field"><span>{t("journals_type")}</span><select className="dashboard-input" value={journalType} onChange={(event) => { setJournalType(event.target.value); setJournalCode(""); }} disabled={catalogLoading}><option value="all">{t("journals_all_types")} ({journals.length})</option>{JOURNAL_TYPE_VALUES.map((value) => <option key={value} value={value}>{t(`journals_type_${value}`)} ({typeCounts[value] ?? 0})</option>)}</select></label>
        <label className="dashboard-field journals-code-field"><span>{t("journals_code")}</span><select className="dashboard-input" value={journalCode} onChange={(event) => setJournalCode(event.target.value)} disabled={catalogLoading}><option value="">{t("journals_all_codes")}</option>{journalOptions.map((journal) => <option key={journal.code} value={journal.code}>{journal.code}{journal.name && journal.name !== journal.code ? ` · ${journal.name}` : ""}</option>)}</select></label>
        <div className="journals-toolbar-status">
          <span className="journals-result-count">{resultCount == null ? t("journals_loading") : t("journals_entry_count", resultCount)}</span>
          {activeTab === "entries" ? <button className="btn journals-export" type="button" onClick={handleExport} disabled={!!rangeError || entriesTotal <= 0 || exportActive || activeJobs.length >= 2}><Download size={13} />{exportActive ? t("journals_exporting") : t("journals_export_excel")}</button> : null}
        </div>
      </section>

      {warnings.length > 0 ? <div className="journals-notice" role="status">{t("journals_schema_fallback")}</div> : null}
      {displayError ? <div className="journals-inline-error" role="alert">{displayError}</div> : null}
      {exportError ? <div className="journals-inline-error" role="alert">{t("journals_export_error")}: {exportError}</div> : null}

      {activeTab === "overview" ? (
        <JournalsOverview stats={stats} loading={statsLoading || !periodReady} error={statsError} t={t} lang={lang} onRetry={() => setReloadKey((value) => value + 1)} />
      ) : entryError ? (
        <div className="journals-error" role="alert"><strong>{t("journals_load_error")}</strong><span>{entryError}</span><button className="btn btn-sm" type="button" onClick={() => setReloadKey((value) => value + 1)}>{t("journals_retry")}</button></div>
      ) : (
        <VirtualJournalGrid rows={entries} total={entriesTotal} loading={entriesLoading} complete={entriesComplete} t={t} lang={lang} />
      )}
    </div>
  );
}
