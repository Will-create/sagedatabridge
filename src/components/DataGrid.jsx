import { useState } from "react";
import { useT } from "../i18n";

function cellClass(value, colType) {
  if (value === null || value === undefined) return "null";
  if (typeof value === "boolean") return value ? "bool-true" : "bool-false";
  if (typeof value === "number") return "number";
  const dt = (colType || "").toLowerCase();
  if (dt.includes("date") || dt.includes("time")) return "date";
  if (["int","float","decimal","money","numeric","real"].some((x) => dt.includes(x))) return "number";
  return "";
}

export default function DataGrid({
  data,
  loading,
  page,
  pageSize,
  onPageChange,
  onPageSizeChange,
  pageSizeOptions = [50, 100, 250, 500, 1000],
}) {
  const { t, lang } = useT();
  const [sortCol, setSortCol] = useState(null);
  const [sortDir, setSortDir] = useState("asc");
  const [selectedCell, setSelectedCell] = useState(null);
  const [expandedCell, setExpandedCell] = useState(null);

  if (loading) {
    return (
      <div className="data-grid-wrap" style={{ display:"flex", alignItems:"center", justifyContent:"center" }}>
        <div style={{ textAlign:"center" }}>
          <div className="spinner" style={{ margin:"0 auto 9.84px" }} />
          <div style={{ fontSize:9.84, color:"var(--text-lo)" }}>{t("grid_loading")}</div>
        </div>
      </div>
    );
  }

  if (!data) {
    return (
      <div className="data-grid-wrap">
        <div className="empty-state">
          <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.2">
            <rect x="3" y="3" width="18" height="18" rx="2" />
            <line x1="3" y1="9" x2="21" y2="9" /><line x1="3" y1="15" x2="21" y2="15" />
            <line x1="9" y1="9" x2="9" y2="21" />
          </svg>
          <h3>{t("grid_no_table")}</h3>
          <p>{t("grid_no_table_hint")}</p>
        </div>
      </div>
    );
  }

  const { columns, rows, total_count } = data;

  if (rows.length === 0) {
    return (
      <div className="data-grid-wrap">
        <div className="empty-state">
          <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.2">
            <circle cx="12" cy="12" r="10" /><line x1="4.93" y1="4.93" x2="19.07" y2="19.07" />
          </svg>
          <h3>{t("grid_no_rows")}</h3>
          <p>{t("grid_no_rows_hint")}</p>
        </div>
      </div>
    );
  }

  const effectivePageSize = pageSize === 0 ? Math.max(total_count, 1) : pageSize;
  const totalPages = Math.max(1, Math.ceil(total_count / effectivePageSize));

  let displayRows = [...rows];
  if (sortCol !== null) {
    displayRows.sort((a, b) => {
      const av = a[sortCol], bv = b[sortCol];
      if (av === null && bv === null) return 0;
      if (av === null) return sortDir === "asc" ? 1 : -1;
      if (bv === null) return sortDir === "asc" ? -1 : 1;
      if (typeof av === "number" && typeof bv === "number") return sortDir === "asc" ? av - bv : bv - av;
      return sortDir === "asc"
        ? String(av).toLowerCase().localeCompare(String(bv).toLowerCase())
        : String(bv).toLowerCase().localeCompare(String(av).toLowerCase());
    });
  }

  const handleSort = (ci) => {
    if (sortCol === ci) setSortDir((d) => d === "asc" ? "desc" : "asc");
    else { setSortCol(ci); setSortDir("asc"); }
  };

  const rowStart = page * effectivePageSize + 1;
  const rowEnd   = Math.min((page + 1) * effectivePageSize, total_count);
  const locale   = lang === "fr" ? "fr-FR" : "en-US";

  return (
    <>
      <div className="data-grid-wrap">
        <table className="data-grid">
          <thead>
            <tr>
              <th style={{ width:37.72, cursor:"default" }} className="row-num">#</th>
              {columns.map((col, ci) => (
                <th key={ci} className={col.is_primary_key ? "pk" : ""} onClick={() => handleSort(ci)}>
                  <div className="th-inner">
                    {col.is_primary_key && (
                      <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" style={{flexShrink:0,opacity:0.7}}>
                        <circle cx="8" cy="15" r="4"/><line x1="11" y1="12" x2="20" y2="3"/>
                        <line x1="15" y1="5" x2="20" y2="5"/><line x1="20" y1="5" x2="20" y2="10"/>
                      </svg>
                    )}
                    {col.name}
                    <span className="type-badge">{col.data_type}</span>
                    {sortCol === ci && (
                      <span style={{ marginLeft:"auto", color:"var(--accent)", fontSize:8.2 }}>
                        {sortDir === "asc" ? "↑" : "↓"}
                      </span>
                    )}
                  </div>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {displayRows.map((row, ri) => (
              <tr key={ri}>
                <td className="row-num">{rowStart + ri}</td>
                {row.map((cell, ci) => {
                  const col = columns[ci];
                  const cls = cellClass(cell, col?.data_type);
                  const isSelected = selectedCell?.[0] === ri && selectedCell?.[1] === ci;
                  const display = cell === null || cell === undefined ? t("null") : String(cell);
                  return (
                    <td key={ci} className={cls} title={display}
                      onClick={() => setSelectedCell([ri, ci])}
                      onDoubleClick={() => cell !== null && String(cell).length > 40 && setExpandedCell({ value: String(cell) })}
                      style={isSelected ? { outline:"1px solid var(--accent)", outlineOffset:"-1px", background:"var(--accent-mute)" } : {}}>
                      {display}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Pagination */}
      <div className="pagination">
        <span className="pagination-info">
          {t("grid_rows_of",
            rowStart.toLocaleString(locale),
            rowEnd.toLocaleString(locale),
            total_count.toLocaleString(locale)
          )}
        </span>
        <div style={{ flex:1 }} />
        <span style={{ color:"var(--text-lo)", fontSize:9.02 }}>{t("grid_page_size")}</span>
        <select className="page-size-select" value={pageSize} onChange={(e) => onPageSizeChange(Number(e.target.value))}>
          {pageSizeOptions.map((size) => <option key={size} value={size}>{size === 0 ? t("all") : size.toLocaleString(locale)}</option>)}
        </select>
        <button className="btn btn-sm" onClick={() => onPageChange(0)} disabled={page === 0}>«</button>
        <button className="btn btn-sm" onClick={() => onPageChange(page - 1)} disabled={page === 0}>‹</button>
        <span style={{ fontSize:9.84, color:"var(--text-mid)", minWidth:65.6, textAlign:"center" }}>
          {page + 1} / {totalPages}
        </span>
        <button className="btn btn-sm" onClick={() => onPageChange(page + 1)} disabled={page >= totalPages - 1}>›</button>
        <button className="btn btn-sm" onClick={() => onPageChange(totalPages - 1)} disabled={page >= totalPages - 1}>»</button>
      </div>

      {/* Expanded cell */}
      {expandedCell && (
        <div className="modal-overlay" onClick={() => setExpandedCell(null)}>
          <div className="modal" style={{ "--modal-width": "492px", "--modal-min-width": "492px" }}>
            <div className="modal-title" style={{ marginBottom:9.84 }}>{t("grid_cell_title")}</div>
            <div className="modal-body">
              <textarea readOnly value={expandedCell.value} rows={10}
                style={{ width:"100%", fontFamily:"var(--font-mono)", fontSize:9.84, resize:"vertical" }} />
            </div>
            <div className="modal-actions">
              <button className="btn" onClick={() => navigator.clipboard.writeText(expandedCell.value)}>{t("copy")}</button>
              <button className="btn btn-ghost" onClick={() => setExpandedCell(null)}>{t("close")}</button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
