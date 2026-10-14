import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { initToken } from "./api/http";
import { applyTheme, readTheme } from "./features/settings/ThemeToggle";
import "./styles/tokens.css";
import "./styles/base.css";

// Before the first render: take the token out of the URL and apply the saved theme.
initToken();
applyTheme(readTheme());

const root = document.getElementById("root");
if (!root) throw new Error("index.html is missing <div id=root>");
createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
