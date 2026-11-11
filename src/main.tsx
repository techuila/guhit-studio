import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
// Inter ships with the app (SIL OFL 1.1), so every platform measures the same
// letter widths. The woff2 files load by unicode-range and work offline.
import "@fontsource-variable/inter";
import "./styles/tokens.css";
import "./ui/global.css";
import { installTipWarmup } from "./ui/tips";

async function boot() {
  installTipWarmup();
  if (import.meta.env.DEV) {
    const { installDevHarness } = await import("./shell/devHarness");
    await installDevHarness();
  }
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}

void boot();
