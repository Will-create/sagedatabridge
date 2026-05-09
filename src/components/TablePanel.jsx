import { useState, useMemo } from "react";
import { useT } from "../i18n";

export default function TablePanel({ tables, loading, activeTable, onSelectTable, onCollapse }) {
  const { t } = useT();
  const [search, setSearch] = useState("");

  const grouped = useMemo(() => {
    const q = search.toLowerCase();
    const filtered = tables.filter((tbl) =>
      !q || tbl.name.toLowerCase().includes(q) || tbl.schema.toLowerCase().includes(q)
    );
    return filtered.reduce((acc, tbl) => {
      if (!acc[tbl.schema]) acc[tbl.schema] = [];
      acc[tbl.schema].push(tbl);
      return acc;
    }, {});
  }, [tables, search]);

  const fmt = (n) => {
    if (n == null) return "";
    if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
    if (n >= 1_000) return (n / 1_000).toFixed(0) + "k";
    return String(n);
  };

  return (
    <div className="table-panel">
      <div className="table-panel-header">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="var(--text-lo)" strokeWidth="2">
          <rect x="3" y="3" width="18" height="18" rx="2" />
          <line x1="3" y1="9" x2="21" y2="9" /><line x1="3" y1="15" x2="21" y2="15" />
          <line x1="9" y1="3" x2="9" y2="21" />
        </svg>
        <input className="table-panel-search" placeholder={t("tables_search_ph")}
          value={search} onChange={(e) => setSearch(e.target.value)} />
        <button className="panel-header-btn" onClick={onCollapse} title={t("panel_hide_tables")}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M15 18l-6-6 6-6" />
          </svg>
        </button>
      </div>

      <div className="table-panel-list">
        {loading ? (
          <div style={{ display:"flex", justifyContent:"center", padding:20 }}>
            <div className="spinner" />
          </div>
        ) : tables.length === 0 ? (
          <div style={{ padding:"16px 12px", color:"var(--text-lo)", fontSize:11 }}>
            {t("tables_none")}
          </div>
        ) : Object.entries(grouped).map(([schema, schemaTables]) => (
          <div key={schema}>
            <div className="table-group-label">{schema}</div>
            {schemaTables.map((tbl) => {
              const isActive = activeTable?.schema === tbl.schema && activeTable?.name === tbl.name;
              return (
                <div key={tbl.full_name} className={`table-item ${isActive ? "active" : ""}`}
                  onClick={() => onSelectTable(tbl)} title={tbl.full_name}>
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
                    <rect x="3" y="3" width="18" height="18" rx="1" />
                    <line x1="3" y1="9" x2="21" y2="9" /><line x1="3" y1="15" x2="21" y2="15" />
                    <line x1="9" y1="9" x2="9" y2="21" />
                  </svg>
                  <span style={{ overflow:"hidden", textOverflow:"ellipsis", whiteSpace:"nowrap" }}>
                    {tbl.name}
                  </span>
                  {tbl.row_count != null && <span className="count">{fmt(tbl.row_count)}</span>}
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}
