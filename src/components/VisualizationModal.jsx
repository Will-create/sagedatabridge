import { useEffect, useMemo, useState } from "react";
import { fetchTableRows } from "../hooks/useTauri";
import { useT } from "../i18n";

const PREVIEW_ROW_LIMIT = 5000;
const CHART_TYPES = ["bar", "line", "donut"];
const METRICS = ["count", "sum", "avg"];

const isNumericType = (type = "") =>
  ["int", "float", "decimal", "numeric", "money", "real", "bigint", "smallint", "tinyint"]
    .some((token) => type.toLowerCase().includes(token));

const parseNumeric = (value) => {
  if (typeof value === "number") return Number.isFinite(value) ? value : null;
  if (value == null || value === "") return null;
  const parsed = Number(String(value).replace(/,/g, ""));
  return Number.isFinite(parsed) ? parsed : null;
};

function ChartCard({ title, value, accent }) {
  return (
    <div className="viz-stat-card">
      <div className="viz-stat-label">{title}</div>
      <div className="viz-stat-value" style={{ color: accent }}>{value}</div>
    </div>
  );
}

function BarChart({ items, formatValue }) {
  const maxValue = Math.max(...items.map((item) => item.value), 1);

  return (
    <div className="viz-chart-canvas">
      {items.map((item) => (
        <div key={item.label} className="viz-bar-row">
          <div className="viz-bar-label" title={item.label}>{item.label}</div>
          <div className="viz-bar-track">
            <div className="viz-bar-fill" style={{ width: `${(item.value / maxValue) * 100}%` }} />
          </div>
          <div className="viz-bar-value">{formatValue(item.value)}</div>
        </div>
      ))}
    </div>
  );
}

function LineChart({ items, formatValue }) {
  const width = 760;
  const height = 260;
  const left = 18;
  const right = 18;
  const top = 18;
  const bottom = 28;
  const maxValue = Math.max(...items.map((item) => item.value), 1);
  const stepX = items.length > 1 ? (width - left - right) / (items.length - 1) : 0;

  const points = items.map((item, index) => {
    const x = left + stepX * index;
    const y = height - bottom - (item.value / maxValue) * (height - top - bottom);
    return { ...item, x, y };
  });

  const path = points.map((point, index) => `${index === 0 ? "M" : "L"} ${point.x} ${point.y}`).join(" ");

  return (
    <div className="viz-chart-canvas">
      <svg viewBox={`0 0 ${width} ${height}`} className="viz-svg">
        <defs>
          <linearGradient id="vizLineFill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="var(--graph-line-fill-start)" />
            <stop offset="100%" stopColor="var(--graph-line-fill-end)" />
          </linearGradient>
        </defs>
        <path
          d={`M ${left} ${height - bottom} ${path.slice(1)} L ${width - right} ${height - bottom} Z`}
          fill="url(#vizLineFill)"
          opacity="0.9"
        />
        <path d={path} fill="none" stroke="var(--accent)" strokeWidth="3" strokeLinecap="round" />
        {points.map((point) => (
          <g key={point.label}>
            <circle cx={point.x} cy={point.y} r="4" fill="var(--accent)" />
            <text x={point.x} y={height - 8} textAnchor="middle" className="viz-axis-label">
              {point.label.length > 10 ? `${point.label.slice(0, 10)}…` : point.label}
            </text>
          </g>
        ))}
      </svg>
      <div className="viz-line-legend">
        {items.map((item) => (
          <div key={item.label} className="viz-legend-item">
            <span className="viz-legend-swatch" />
            <span className="viz-legend-text">{item.label}</span>
            <strong>{formatValue(item.value)}</strong>
          </div>
        ))}
      </div>
    </div>
  );
}

function arcPath(cx, cy, radius, startAngle, endAngle) {
  const largeArcFlag = endAngle - startAngle > Math.PI ? 1 : 0;
  const x1 = cx + radius * Math.cos(startAngle);
  const y1 = cy + radius * Math.sin(startAngle);
  const x2 = cx + radius * Math.cos(endAngle);
  const y2 = cy + radius * Math.sin(endAngle);
  return `M ${cx} ${cy} L ${x1} ${y1} A ${radius} ${radius} 0 ${largeArcFlag} 1 ${x2} ${y2} Z`;
}

function DonutChart({ items, formatValue, totalLabel }) {
  const total = items.reduce((sum, item) => sum + item.value, 0) || 1;
  const colors = ["#e8c444", "#57c7ff", "#58d68d", "#ff8f6b", "#c18bff", "#f15a8c", "#8fd8d2", "#c0d55d"];
  let start = -Math.PI / 2;

  return (
    <div className="viz-chart-canvas viz-donut-layout">
      <svg viewBox="0 0 320 260" className="viz-svg" style={{ maxWidth: 320 }}>
        <circle cx="130" cy="130" r="92" fill="var(--donut-track)" />
        {items.map((item, index) => {
          const angle = (item.value / total) * Math.PI * 2;
          const end = start + angle;
          const d = arcPath(130, 130, 92, start, end);
          const color = colors[index % colors.length];
          start = end;
          return <path key={item.label} d={d} fill={color} stroke="var(--bg-base)" strokeWidth="2" />;
        })}
        <circle cx="130" cy="130" r="54" fill="var(--bg-base)" />
        <text x="130" y="122" textAnchor="middle" className="viz-donut-total-label">{totalLabel}</text>
        <text x="130" y="146" textAnchor="middle" className="viz-donut-total-value">{formatValue(total)}</text>
      </svg>

      <div className="viz-donut-legend">
        {items.map((item, index) => {
          const color = colors[index % colors.length];
          return (
            <div key={item.label} className="viz-legend-item">
              <span className="viz-legend-swatch" style={{ background: color }} />
              <span className="viz-legend-text">{item.label}</span>
              <strong>{formatValue(item.value)}</strong>
            </div>
          );
        })}
      </div>
    </div>
  );
}

export default function VisualizationModal({ activeConn, activeTable, filters, onClose, onToast }) {
  const { t, lang } = useT();
  const [dataset, setDataset] = useState(null);
  const [loading, setLoading] = useState(true);
  const [chartType, setChartType] = useState("bar");
  const [dimensionCol, setDimensionCol] = useState("");
  const [metric, setMetric] = useState("count");
  const [valueCol, setValueCol] = useState("");
  const locale = lang === "fr" ? "fr-FR" : "en-US";

  useEffect(() => {
    let cancelled = false;

    const load = async () => {
      setLoading(true);
      try {
        const result = await fetchTableRows(
          activeConn,
          activeTable.schema,
          activeTable.name,
          filters,
          { pageSize: 2000, maxRows: PREVIEW_ROW_LIMIT },
        );
        if (!cancelled) setDataset(result);
      } catch (err) {
        if (!cancelled) {
          onToast({ type: "error", msg: String(err) });
          onClose();
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    };

    load();
    return () => { cancelled = true; };
  }, [activeConn, activeTable, filters, onClose, onToast]);

  const numericColumns = useMemo(
    () => (dataset?.columns || []).filter((column) => isNumericType(column.data_type)),
    [dataset],
  );

  useEffect(() => {
    if (!dataset?.columns?.length) return;
    if (!dimensionCol) setDimensionCol(dataset.columns[0].name);
    if (!valueCol && numericColumns.length > 0) setValueCol(numericColumns[0].name);
  }, [dataset, dimensionCol, valueCol, numericColumns]);

  const chartData = useMemo(() => {
    if (!dataset || !dimensionCol) return [];
    const dimIndex = dataset.columns.findIndex((column) => column.name === dimensionCol);
    const valueIndex = dataset.columns.findIndex((column) => column.name === valueCol);
    const grouped = new Map();

    dataset.rows.forEach((row) => {
      const rawLabel = row[dimIndex];
      const label = rawLabel == null || rawLabel === "" ? t("viz_null") : String(rawLabel);
      const entry = grouped.get(label) || { sum: 0, count: 0 };

      if (metric === "count") {
        entry.sum += 1;
        entry.count += 1;
      } else {
        const numeric = parseNumeric(row[valueIndex]);
        if (numeric == null) return;
        entry.sum += numeric;
        entry.count += 1;
      }

      grouped.set(label, entry);
    });

    const points = [...grouped.entries()]
      .map(([label, entry]) => ({
        label,
        value: metric === "avg" ? entry.sum / Math.max(entry.count, 1) : entry.sum,
      }))
      .filter((item) => Number.isFinite(item.value));

    if (chartType === "line") {
      return points
        .sort((a, b) => a.label.localeCompare(b.label, locale))
        .slice(0, 16);
    }

    return points
      .sort((a, b) => b.value - a.value)
      .slice(0, 10);
  }, [chartType, dataset, dimensionCol, locale, metric, t, valueCol]);

  const groupCount = useMemo(() => new Set(chartData.map((item) => item.label)).size, [chartData]);
  const formatValue = (value) => {
    const formatter = new Intl.NumberFormat(locale, { maximumFractionDigits: metric === "count" ? 0 : 2 });
    return formatter.format(value);
  };

  const renderChart = () => {
    if (!chartData.length) {
      return <div className="viz-empty-state">{t("viz_empty")}</div>;
    }

    if (chartType === "line") return <LineChart items={chartData} formatValue={formatValue} />;
    if (chartType === "donut") return <DonutChart items={chartData} formatValue={formatValue} totalLabel={t("viz_total")} />;
    return <BarChart items={chartData} formatValue={formatValue} />;
  };

  return (
    <div className="modal-overlay" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal viz-modal">
        <div className="modal-title">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M3 3v18h18" />
            <path d="M7 15l4-4 3 3 5-6" />
          </svg>
          {t("viz_title", activeTable.full_name)}
        </div>

        <div className="modal-body modal-body-fill">
          {loading ? (
            <div className="viz-loading">
              <div className="spinner" />
              <div>{t("viz_loading")}</div>
            </div>
          ) : (
            <div className="viz-modal-content">
              <div className="viz-toolbar">
                <div className="viz-field">
                  <label>{t("viz_chart")}</label>
                  <select value={chartType} onChange={(e) => setChartType(e.target.value)}>
                    {CHART_TYPES.map((type) => (
                      <option key={type} value={type}>{t(`viz_type_${type}`)}</option>
                    ))}
                  </select>
                </div>

                <div className="viz-field">
                  <label>{t("viz_dimension")}</label>
                  <select value={dimensionCol} onChange={(e) => setDimensionCol(e.target.value)}>
                    {(dataset?.columns || []).map((column) => (
                      <option key={column.name} value={column.name}>{column.name}</option>
                    ))}
                  </select>
                </div>

                <div className="viz-field">
                  <label>{t("viz_metric")}</label>
                  <select value={metric} onChange={(e) => setMetric(e.target.value)}>
                    {METRICS.map((metricId) => (
                      <option key={metricId} value={metricId}>{t(`viz_metric_${metricId}`)}</option>
                    ))}
                  </select>
                </div>

                <div className="viz-field">
                  <label>{t("viz_value")}</label>
                  <select
                    value={valueCol}
                    onChange={(e) => setValueCol(e.target.value)}
                    disabled={metric === "count" || numericColumns.length === 0}
                  >
                    {numericColumns.length === 0 ? (
                      <option value="">{t("viz_no_numeric")}</option>
                    ) : (
                      numericColumns.map((column) => (
                        <option key={column.name} value={column.name}>{column.name}</option>
                      ))
                    )}
                  </select>
                </div>
              </div>

              <div className="viz-summary-grid">
                <ChartCard title={t("viz_rows")} value={dataset.rows.length.toLocaleString(locale)} accent="var(--accent)" />
                <ChartCard title={t("viz_distinct")} value={groupCount.toLocaleString(locale)} accent="var(--blue)" />
                <ChartCard title={t("viz_metric_label")} value={t(`viz_metric_${metric}`)} accent="var(--green)" />
              </div>

              {dataset.totalCount > dataset.rows.length && (
                <div className="viz-preview-note">
                  {t("viz_preview", dataset.rows.length.toLocaleString(locale), dataset.totalCount.toLocaleString(locale))}
                </div>
              )}

              {renderChart()}
            </div>
          )}
        </div>

        <div className="modal-actions">
          <button className="btn btn-ghost" onClick={onClose}>{t("close")}</button>
        </div>
      </div>
    </div>
  );
}
