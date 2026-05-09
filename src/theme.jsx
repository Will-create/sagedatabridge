import { createContext, useContext, useEffect, useState } from "react";

const STORAGE_KEY = "sdb_theme";

export const ThemeContext = createContext({
  theme: "light",
  setTheme: () => {},
});

function normalizeTheme(value) {
  return value === "dark" ? "dark" : "light";
}

export function ThemeProvider({ children }) {
  const [theme, setThemeState] = useState(() => normalizeTheme(localStorage.getItem(STORAGE_KEY)));

  useEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = theme;
    root.style.colorScheme = theme;
    localStorage.setItem(STORAGE_KEY, theme);
  }, [theme]);

  const setTheme = (value) => setThemeState(normalizeTheme(value));

  return (
    <ThemeContext.Provider value={{ theme, setTheme }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme() {
  return useContext(ThemeContext);
}
