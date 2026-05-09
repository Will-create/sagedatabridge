import React from "react";
import ReactDOM from "react-dom/client";
import { LangProvider } from "./i18n";
import App from "./App";
import { ThemeProvider } from "./theme";
import "./App.css";

ReactDOM.createRoot(document.getElementById("root")).render(
  <React.StrictMode>
    <ThemeProvider>
      <LangProvider>
        <App />
      </LangProvider>
    </ThemeProvider>
  </React.StrictMode>
);
