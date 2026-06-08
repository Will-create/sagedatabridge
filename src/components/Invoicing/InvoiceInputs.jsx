import { useEffect, useState } from "react";

import { useT } from "../../i18n";

function useDebouncedValue(value, delay = 300) {
  const [debounced, setDebounced] = useState(value);

  useEffect(() => {
    const timeoutId = globalThis.setTimeout(() => setDebounced(value), delay);
    return () => globalThis.clearTimeout(timeoutId);
  }, [delay, value]);

  return debounced;
}

export function SearchSelect({
  placeholder,
  value,
  displayValue,
  onSelect,
  searchFn,
  emptyLabel,
  renderOption,
}) {
  const { t } = useT();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState(displayValue || "");
  const [options, setOptions] = useState([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const debouncedQuery = useDebouncedValue(query, 300);

  useEffect(() => {
    if (!open) setQuery(displayValue || "");
  }, [displayValue, open]);

  useEffect(() => {
    if (!open) return undefined;
    const handle = () => setOpen(false);
    window.addEventListener("click", handle);
    return () => window.removeEventListener("click", handle);
  }, [open]);

  useEffect(() => {
    if (!open) return undefined;
    let cancelled = false;
    if (debouncedQuery.trim().length < 2) {
      setOptions([]);
      setLoading(false);
      return undefined;
    }

    setLoading(true);
    setError("");
    searchFn(debouncedQuery)
      .then((items) => {
        if (!cancelled) setOptions(items.slice(0, 12));
      })
      .catch((nextError) => {
        if (!cancelled) {
          setOptions([]);
          setError(String(nextError));
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [debouncedQuery, open, searchFn]);

  return (
    <div className="invoice-search-select" onClick={(event) => event.stopPropagation()}>
      <input
        value={query}
        placeholder={placeholder}
        onFocus={() => {
          if (query === displayValue) setQuery("");
          setOpen(true);
        }}
        onChange={(event) => {
          setQuery(event.target.value);
          setOpen(true);
        }}
      />
      {open ? (
        <div className="invoice-search-select-menu">
          {loading ? <div className="invoice-search-select-empty">{t("loading")}</div> : null}
          {!loading && error ? <div className="invoice-search-select-empty">{error}</div> : null}
          {!loading && !error && !options.length && debouncedQuery.trim().length >= 2 ? (
            <div className="invoice-search-select-empty">{emptyLabel}</div>
          ) : null}
          {!loading && !error ? options.map((option) => (
            <button
              key={option.id || option.code}
              type="button"
              className={`invoice-search-select-option ${value === option.id ? "active" : ""}`}
              onClick={() => {
                onSelect(option);
                setQuery("");
                setOpen(false);
              }}
            >
              {renderOption(option)}
            </button>
          )) : null}
        </div>
      ) : null}
    </div>
  );
}

export function TierSummary({ tier }) {
  const { t } = useT();
  if (!tier?.tiers_code && !tier?.code) return null;

  const value = (entry) => entry || t("invoice_not_provided");
  const code = tier.tiers_code || tier.code;
  const name = tier.tiers_nom || tier.nom;
  const address = tier.tiers_adresse || tier.adresse;
  const city = [tier.tiers_cp || tier.cp, tier.tiers_ville || tier.ville].filter(Boolean).join(" ");
  const country = tier.tiers_pays || tier.pays;
  const siret = tier.tiers_siret || tier.siret;
  const vat = tier.tiers_tva_intra || tier.tva_intra;

  return (
    <div className="invoice-tier-summary">
      <div className="invoice-tier-summary-head">
        <strong>{value(name)}</strong>
        <span>{code}</span>
      </div>
      <div className="invoice-tier-summary-grid">
        <span><small>{t("invoice_address")}</small>{value(address)}</span>
        <span><small>{t("invoice_city")}</small>{value(city)}</span>
        <span><small>{t("invoice_country")}</small>{value(country)}</span>
        <span><small>SIRET</small>{value(siret)}</span>
        <span><small>{t("invoice_vat_number")}</small>{value(vat)}</span>
      </div>
    </div>
  );
}
