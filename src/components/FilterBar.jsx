import { useState } from "react";
import { useT } from "../i18n";

const OPERATORS = [
  { value:"=", label:"=" }, { value:"!=", label:"≠" },
  { value:">", label:">" }, { value:"<", label:"<" },
  { value:">=", label:"≥" }, { value:"<=", label:"≤" },
  { value:"LIKE", label:"LIKE" }, { value:"NOT LIKE", label:"NOT LIKE" },
  { value:"IS NULL", label:"IS NULL" }, { value:"IS NOT NULL", label:"IS NOT NULL" },
  { value:"BETWEEN", label:"BETWEEN" }, { value:"IN", label:"IN" },
];
const NO_VALUE_OPS = ["IS NULL", "IS NOT NULL"];
const DUAL_VALUE_OPS = ["BETWEEN"];

function FilterModal({ columns, onAdd, onClose }) {
  const { t } = useT();
  const [col, setCol] = useState(columns[0]?.name || "");
  const [op, setOp] = useState("=");
  const [val, setVal] = useState("");
  const [val2, setVal2] = useState("");

  const colInfo = columns.find((c) => c.name === col);
  const noValue = NO_VALUE_OPS.includes(op);
  const dualValue = DUAL_VALUE_OPS.includes(op);

  const handleAdd = () => {
    if (!col) return;
    onAdd({ column:col, operator:op,
      value: noValue ? null : (val || null),
      value2: dualValue ? (val2 || null) : null,
      data_type: colInfo?.data_type || null });
    onClose();
  };

  return (
    <div className="modal-overlay" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" style={{ "--modal-width": "420px", "--modal-min-width": "420px" }}>
        <div className="modal-title">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3" />
          </svg>
          {t("filter_modal_title")}
        </div>

        <div className="modal-body">
          <div className="filter-modal-form">
            <div className="form-group">
              <label>{t("filter_col")}</label>
              <select value={col} onChange={(e) => setCol(e.target.value)}>
                {columns.map((c) => (
                  <option key={c.name} value={c.name}>{c.name} ({c.data_type})</option>
                ))}
              </select>
            </div>

            <div className="form-group">
              <label>{t("filter_condition")}</label>
              <select value={op} onChange={(e) => setOp(e.target.value)}>
                {OPERATORS.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
              </select>
            </div>

            {!noValue && (
              <div className={dualValue ? "form-row" : "form-group"}>
                <div className="form-group">
                  <label>{dualValue ? t("filter_from") : t("filter_value")}</label>
                  <input type="text" value={val} onChange={(e) => setVal(e.target.value)}
                    placeholder={op === "LIKE" ? t("filter_ph_like") : op === "IN" ? t("filter_ph_in") : t("filter_ph_value")}
                    autoFocus onKeyDown={(e) => e.key === "Enter" && handleAdd()} />
                </div>
                {dualValue && (
                  <div className="form-group">
                    <label>{t("filter_to")}</label>
                    <input type="text" value={val2} onChange={(e) => setVal2(e.target.value)}
                      placeholder={t("filter_ph_upper")} onKeyDown={(e) => e.key === "Enter" && handleAdd()} />
                  </div>
                )}
              </div>
            )}

            {colInfo && (
              <div style={{ fontSize:11, color:"var(--text-lo)" }}>
                {t("filter_type")}: <span style={{ color:"var(--text-mid)", fontFamily:"var(--font-mono)" }}>{colInfo.data_type}</span>
                {" · "}{colInfo.is_nullable ? t("filter_nullable") : t("filter_notnull")}
                {colInfo.is_primary_key ? " · " + t("filter_pk") : ""}
              </div>
            )}
          </div>
        </div>

        <div className="modal-actions">
          <button className="btn btn-ghost" onClick={onClose}>{t("cancel")}</button>
          <button className="btn btn-accent" onClick={handleAdd}>{t("filter_add")}</button>
        </div>
      </div>
    </div>
  );
}

export default function FilterBar({ filters, columns, onFiltersChange }) {
  const { t } = useT();
  const [showModal, setShowModal] = useState(false);

  const removeFilter = (idx) => onFiltersChange(filters.filter((_, i) => i !== idx));
  const addFilter    = (f)   => onFiltersChange([...filters, f]);
  const clearAll     = ()    => onFiltersChange([]);

  const renderChip = (f, idx) => {
    const noVal  = NO_VALUE_OPS.includes(f.operator);
    const dual   = DUAL_VALUE_OPS.includes(f.operator);
    return (
      <div key={idx} className="filter-chip">
        <span className="col-name">{f.column}</span>
        <span className="op">{f.operator}</span>
        {!noVal && f.value != null && (
          <span className="val">{dual ? `${f.value} … ${f.value2}` : f.value}</span>
        )}
        <button className="remove-btn" onClick={() => removeFilter(idx)} title="×">×</button>
      </div>
    );
  };

  return (
    <>
      <div className="filter-bar">
        <div className="filter-row">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--text-lo)" strokeWidth="2" style={{ flexShrink:0 }}>
            <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3" />
          </svg>

          {filters.length === 0
            ? <span style={{ fontSize:11.5, color:"var(--text-lo)" }}>{t("filter_none")}</span>
            : filters.map((f, i) => renderChip(f, i))
          }

          <button className="filter-add-btn" onClick={() => setShowModal(true)}>
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
              <line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" />
            </svg>
            {t("filter_add")}
          </button>

          {filters.length > 0 && (
            <button className="btn btn-ghost btn-sm" style={{ marginLeft:"auto" }} onClick={clearAll}>
              {t("filter_clear")}
            </button>
          )}
        </div>
      </div>

      {showModal && columns.length > 0 && (
        <FilterModal columns={columns} onAdd={addFilter} onClose={() => setShowModal(false)} />
      )}
    </>
  );
}
