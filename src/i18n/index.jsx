import { createContext, useContext, useState } from "react";
import { translations } from "./translations";

// ─── Context ──────────────────────────────────────────────────────────────────

export const LangContext = createContext({
  lang: "en",
  setLang: () => {},
  t: (key, ...args) => key,
});

// ─── Provider ─────────────────────────────────────────────────────────────────

export function LangProvider({ children }) {
  const stored = localStorage.getItem("sdb_lang") || "en";
  const [lang, setLangState] = useState(stored === "fr" ? "fr" : "en");

  const setLang = (l) => {
    const chosen = l === "fr" ? "fr" : "en";
    localStorage.setItem("sdb_lang", chosen);
    setLangState(chosen);
  };

  // t(key)           → string
  // t(key, ...args)  → string (if the translation is a function, call it with args)
  const t = (key, ...args) => {
    const dict = translations[lang];
    const val = dict[key];
    if (val === undefined) {
      // Fallback to English
      const en = translations["en"][key];
      if (en === undefined) return key;
      return typeof en === "function" ? en(...args) : en;
    }
    return typeof val === "function" ? val(...args) : val;
  };

  return (
    <LangContext.Provider value={{ lang, setLang, t }}>
      {children}
    </LangContext.Provider>
  );
}

// ─── Hook ─────────────────────────────────────────────────────────────────────

export function useT() {
  return useContext(LangContext);
}
