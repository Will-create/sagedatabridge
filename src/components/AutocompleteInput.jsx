import { useEffect, useRef, useState } from "react";
import { getFieldHistory, removeFieldHistoryEntry } from "../hooks/useTauri";

export default function AutocompleteInput({
  fieldKey,
  value,
  onChange,
  placeholder,
  type = "text",
  autoFocus,
  autoComplete,
  disabled,
}) {
  const [suggestions, setSuggestions] = useState([]);
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(-1);
  const wrapperRef = useRef(null);
  const inputRef = useRef(null);

  useEffect(() => {
    if (!fieldKey) return;
    getFieldHistory(fieldKey)
      .then(setSuggestions)
      .catch(() => {});
  }, [fieldKey]);

  useEffect(() => {
    const handler = (e) => {
      if (wrapperRef.current && !wrapperRef.current.contains(e.target)) {
        setOpen(false);
        setActiveIndex(-1);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, []);

  const filtered = value.trim()
    ? suggestions.filter((s) =>
        s.toLowerCase().startsWith(value.trim().toLowerCase()),
      )
    : suggestions;

  const handleFocus = () => {
    if (suggestions.length > 0) setOpen(true);
  };

  const handleChange = (e) => {
    onChange(e.target.value);
    setOpen(true);
    setActiveIndex(-1);
  };

  const selectSuggestion = (s) => {
    onChange(s);
    setOpen(false);
    setActiveIndex(-1);
    inputRef.current?.focus();
  };

  const handleKeyDown = (e) => {
    if (!open || filtered.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIndex((i) => Math.min(i + 1, filtered.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIndex((i) => Math.max(i - 1, -1));
    } else if (e.key === "Enter" && activeIndex >= 0) {
      e.preventDefault();
      selectSuggestion(filtered[activeIndex]);
    } else if (e.key === "Escape") {
      setOpen(false);
      setActiveIndex(-1);
    }
  };

  const handleDelete = async (e, entry) => {
    e.stopPropagation();
    try {
      await removeFieldHistoryEntry(fieldKey, entry);
      const next = suggestions.filter((s) => s !== entry);
      setSuggestions(next);
      if (next.length === 0) setOpen(false);
    } catch {}
  };

  const showDropdown = open && filtered.length > 0;

  return (
    <div ref={wrapperRef} style={{ position: "relative" }}>
      <input
        ref={inputRef}
        type={type}
        value={value}
        onChange={handleChange}
        onFocus={handleFocus}
        onKeyDown={handleKeyDown}
        placeholder={placeholder}
        autoFocus={autoFocus}
        autoComplete={autoComplete ?? "off"}
        disabled={disabled}
      />
      {showDropdown && (
        <div
          style={{
            position: "absolute",
            top: "100%",
            left: 0,
            width: "100%",
            zIndex: 200,
            background: "var(--bg-modal)",
            border: "1px solid var(--border-mid)",
            borderRadius: "var(--r-md)",
            boxShadow: "0 8px 24px rgba(0,0,0,0.4)",
            maxHeight: 220,
            overflowY: "auto",
            marginTop: 2,
          }}
        >
          {filtered.map((entry, idx) => (
            <div
              key={entry}
              onMouseDown={() => selectSuggestion(entry)}
              style={{
                display: "flex",
                alignItems: "center",
                padding: "7px 12px",
                fontSize: "12.5px",
                fontFamily: "var(--font-mono)",
                color: idx === activeIndex ? "var(--accent)" : "var(--text-mid)",
                background: idx === activeIndex ? "var(--accent-mute)" : "transparent",
                cursor: "pointer",
                gap: 6,
              }}
              onMouseEnter={(e) => {
                if (idx !== activeIndex) {
                  e.currentTarget.style.background = "var(--bg-hover)";
                  e.currentTarget.style.color = "var(--text-hi)";
                }
              }}
              onMouseLeave={(e) => {
                if (idx !== activeIndex) {
                  e.currentTarget.style.background = "transparent";
                  e.currentTarget.style.color = "var(--text-mid)";
                }
              }}
            >
              <span style={{ flexShrink: 0, opacity: 0.5, fontSize: 11 }}>⏱</span>
              <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {entry}
              </span>
              <span
                onMouseDown={(e) => handleDelete(e, entry)}
                style={{
                  flexShrink: 0,
                  opacity: 0.4,
                  fontSize: 13,
                  lineHeight: 1,
                  padding: "0 2px",
                  cursor: "pointer",
                }}
                title="Remove from history"
              >
                ×
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
