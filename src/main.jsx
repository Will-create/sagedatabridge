import React from "react";
import ReactDOM from "react-dom/client";
import { LangProvider } from "./i18n";
import App from "./App";
import { ThemeProvider } from "./theme";
import "./App.css";
import { ExportJobsProvider } from "./exportJobs";

ReactDOM.createRoot(document.getElementById("root")).render(
  <React.StrictMode>
    <ThemeProvider>
      <LangProvider>
        <ExportJobsProvider>
          <App />
        </ExportJobsProvider>
      </LangProvider>
    </ThemeProvider>
  </React.StrictMode>
);
