import { useEffect, useRef, useState } from "react";
import { hasPin, verifyPin } from "../hooks/useTauri";
import { useT } from "../i18n";

export default function LockScreen({ onUnlock }) {
  const { t } = useT();
  const [needsPin, setNeedsPin] = useState(null);
  const [pin, setPin] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);
  const inputRef = useRef(null);

  useEffect(() => {
    hasPin().then((hasStoredPin) => {
      if (hasStoredPin) {
        setNeedsPin(true);
      } else {
        setNeedsPin(false);
        onUnlock();
      }
    });
  }, [onUnlock]);

  useEffect(() => {
    if (needsPin && inputRef.current) inputRef.current.focus();
  }, [needsPin]);

  const handleUnlock = async (event) => {
    event.preventDefault();
    setLoading(true);
    setError("");

    try {
      const ok = await verifyPin(pin);
      if (ok) {
        onUnlock();
      } else {
        setError(t("lock_err_wrong"));
        setPin("");
        inputRef.current?.focus();
      }
    } catch (nextError) {
      setError(String(nextError));
    } finally {
      setLoading(false);
    }
  };

  if (needsPin === null) {
    return <div className="lock-screen"><div className="spinner" /></div>;
  }

  return (
    <div className="lock-screen">
      <div className="lock-card">
        <svg className="lock-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
          <rect x="3" y="11" width="18" height="11" rx="2" ry="2" />
          <path d="M7 11V7a5 5 0 0 1 10 0v4" />
        </svg>
        <div className="lock-title">{t("lock_title")}</div>
        <div className="lock-subtitle">{t("lock_subtitle_unlock")}</div>
        <form onSubmit={handleUnlock}>
          <div className="form-group">
            <input
              ref={inputRef}
              type="password"
              placeholder={t("lock_password")}
              value={pin}
              onChange={(event) => { setPin(event.target.value); setError(""); }}
              autoComplete="current-password"
              inputMode="numeric"
              pattern="[0-9]*"
            />
          </div>
          <div className="lock-error">{error}</div>
          <button
            type="submit"
            className="btn btn-accent"
            style={{ width: "100%", justifyContent: "center", marginTop: 12, padding: "10px" }}
            disabled={loading}
          >
            {loading ? <span className="spinner" style={{ width: 14, height: 14 }} /> : t("lock_unlock")}
          </button>
        </form>
      </div>
    </div>
  );
}
