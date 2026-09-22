// In-app update notice.
//
// The desktop app checks GitHub releases through the Tauri updater plugin
// (endpoint and public key: src-tauri/tauri.conf.json), shows one calm card
// when a newer version is published, downloads it with a progress bar and
// restarts. Release process: docs/RELEASING.md.
//
// Outside the desktop app (the dev bridge in a plain browser, and tests) this
// renders nothing and never touches the plugin, so the browser build stays
// clean. Failures show one sentence and never block the app.

import { useEffect } from "react";
import { create } from "zustand";
import { isTauri } from "../contract/ipc";
import { useApp } from "../state/store";
import { Button, cx } from "../ui/controls";
import { Icon } from "../ui/icons";
import { usePresence } from "../ui/motion";
import s from "./UpdateNotice.module.css";

import type { Update } from "@tauri-apps/plugin-updater";

/** First check after launch: late enough that it never competes with startup. */
const FIRST_CHECK_MS = 3_000;
/** Then once every six hours while the app stays open. */
const EVERY_MS = 6 * 60 * 60 * 1000;

type Phase = "idle" | "checking" | "found" | "downloading" | "installing" | "failed";

interface UpdateState {
  phase: Phase;
  version: string;
  /** First line of the release notes, or "" when the release has none. */
  headline: string;
  /** 0..1 while downloading, null when the total size is unknown. */
  progress: number | null;
  error: string;
  dismissed: boolean;
}

const useUpdate = create<UpdateState>(() => ({
  phase: "idle",
  version: "",
  headline: "",
  progress: null,
  error: "",
  dismissed: false,
}));

/** The plugin's handle for the pending update. Not state: it is not renderable. */
let pending: Update | null = null;
/** One shared schedule, even though both the hub and the editor mount the notice. */
let scheduled = false;

function firstLine(body: string | undefined): string {
  if (!body) return "";
  for (const line of body.split("\n")) {
    const text = line.replace(/^[#>*\-\s]+/, "").trim();
    if (text !== "") return text.length > 160 ? `${text.slice(0, 157)}...` : text;
  }
  return "";
}

function message(e: unknown): string {
  const text = e instanceof Error ? e.message : String(e);
  return text.trim() === "" ? "The update could not be completed." : text;
}

/**
 * Asks GitHub whether a newer release exists. `manual` is the palette action:
 * it toasts the answer either way. The automatic check stays silent unless it
 * finds something.
 */
export async function checkForUpdates(manual = false): Promise<void> {
  const app = useApp.getState();
  if (!isTauri) {
    if (manual) app.toast("info", "Updates are only available in the desktop app");
    return;
  }
  const phase = useUpdate.getState().phase;
  if (phase === "checking" || phase === "downloading" || phase === "installing") return;
  // An update already waiting: just bring the card back.
  if (phase === "found" && pending) {
    useUpdate.setState({ dismissed: false });
    return;
  }

  useUpdate.setState({ phase: "checking", error: "" });
  try {
    const { check } = await import("@tauri-apps/plugin-updater");
    const update = await check();
    if (!update) {
      pending = null;
      useUpdate.setState({ phase: "idle" });
      if (manual) app.toast("success", "You are up to date");
      return;
    }
    pending = update;
    useUpdate.setState({
      phase: "found",
      version: update.version,
      headline: firstLine(update.body),
      progress: null,
      dismissed: false,
    });
  } catch (e) {
    pending = null;
    useUpdate.setState({ phase: "idle" });
    // A silent check that fails (offline, GitHub down) must stay silent.
    if (manual) app.toast("error", `Could not check for updates. ${message(e)}`);
  }
}

/** Downloads the pending update, then restarts into it. */
async function installAndRestart(): Promise<void> {
  const update = pending;
  if (!update) return;
  useUpdate.setState({ phase: "downloading", progress: null, error: "" });
  try {
    let total = 0;
    let got = 0;
    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case "Started":
          total = event.data.contentLength ?? 0;
          useUpdate.setState({ progress: total > 0 ? 0 : null });
          break;
        case "Progress":
          got += event.data.chunkLength;
          if (total > 0) useUpdate.setState({ progress: Math.min(1, got / total) });
          break;
        case "Finished":
          useUpdate.setState({ phase: "installing", progress: 1 });
          break;
      }
    });
    const { relaunch } = await import("@tauri-apps/plugin-process");
    await relaunch();
  } catch (e) {
    useUpdate.setState({ phase: "failed", error: message(e) });
  }
}

/**
 * The notice card. Mounted by both the hub and the editor; the first one to
 * mount owns the schedule, so switching screens does not re-check.
 */
export function UpdateNotice() {
  const st = useUpdate();

  useEffect(() => {
    if (!isTauri || scheduled) return;
    scheduled = true;
    const first = window.setTimeout(() => void checkForUpdates(), FIRST_CHECK_MS);
    const repeat = window.setInterval(() => void checkForUpdates(), EVERY_MS);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(repeat);
      scheduled = false;
    };
  }, []);

  const busy = st.phase === "downloading" || st.phase === "installing";
  const open = !st.dismissed && (st.phase === "found" || busy || st.phase === "failed");
  const presence = usePresence(open, "panel");
  if (!presence.mounted) return null;

  const later = () => {
    if (st.phase === "failed") {
      useUpdate.setState({ phase: "idle", error: "" });
      pending = null;
    }
    // Put it away. The next check, six-hourly or from the palette, brings the
    // same update back.
    useUpdate.setState({ dismissed: true });
  };

  const pct = st.progress === null ? null : Math.round(st.progress * 100);

  return (
    <aside
      className={s.card}
      data-stage={presence.stage}
      role="status"
      aria-live="polite"
      aria-label="Software update"
    >
      <div className={s.head}>
        <Icon name={st.phase === "failed" ? "warning" : "import"} size={16} className={s.icon} />
        <div className={s.text}>
          <strong className={s.title}>Guhit Studio {st.version} is available</strong>
          {st.phase === "failed" ? (
            <span className={s.detail}>{st.error}</span>
          ) : st.headline !== "" ? (
            <span className={s.detail}>{st.headline}</span>
          ) : null}
        </div>
      </div>

      {busy ? (
        <div className={s.progressRow}>
          <div
            className={s.track}
            role="progressbar"
            aria-label="Downloading the update"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={pct ?? undefined}
          >
            <span className={cx(s.bar, pct === null && s.barUnknown)} style={pct === null ? undefined : { width: `${pct}%` }} />
          </div>
          <span className={s.pct}>
            {st.phase === "installing" ? "Installing" : pct === null ? "Downloading" : `${pct}%`}
          </span>
        </div>
      ) : (
        <div className={s.actions}>
          <Button size="sm" onClick={later}>
            Later
          </Button>
          <Button size="sm" variant="primary" onClick={() => void installAndRestart()}>
            {st.phase === "failed" ? "Try again" : "Update and restart"}
          </Button>
        </div>
      )}
    </aside>
  );
}
