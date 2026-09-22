import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/tokens.css";
import "./ui/global.css";

async function boot() {
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
