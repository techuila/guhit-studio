// Toasts (lane L0): an optional action button, a timer that pauses while the
// toast is hovered or focused, and a longer life when there is a button.
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useApp, type Toast } from "../state/store";
import { NO_ANNOUNCEMENTS, ToastItem, ToastsView, createPausableTimer, nextAnnouncements, runToastAction, toastLifetime } from "./feedback";

describe("toast action", () => {
  beforeEach(() => useApp.setState({ toasts: [] }));

  it("is stored with the toast, and a toast without one has no action key", () => {
    const run = vi.fn();
    useApp.getState().toast("success", "Exported plan.pdf", { label: "Show in folder", run });
    useApp.getState().toast("info", "Saved");
    const [withAction, plain] = useApp.getState().toasts;
    expect(withAction.action?.label).toBe("Show in folder");
    expect("action" in plain).toBe(false);
  });

  it("renders a button with its label next to Dismiss", () => {
    const toast: Toast = { id: 1, kind: "success", message: "Exported plan.pdf", action: { label: "Show in folder", run: () => {} } };
    const html = renderToStaticMarkup(<ToastItem toast={toast} leaving={false} />);
    expect(html).toContain(">Show in folder</button>");
    expect(html).toContain('aria-label="Dismiss"');
  });

  it("renders no extra button without an action", () => {
    const html = renderToStaticMarkup(<ToastItem toast={{ id: 2, kind: "error", message: "Could not export" }} leaving={false} />);
    expect(html.match(/<button/g)?.length).toBe(1);
  });

  it("runs the action, then dismisses the toast", () => {
    const calls: string[] = [];
    const toast: Toast = { id: 7, kind: "info", message: "Moved to trash", action: { label: "Undo", run: () => calls.push("run") } };
    runToastAction(toast, (id) => calls.push(`dismiss ${id}`));
    expect(calls).toEqual(["run", "dismiss 7"]);
  });

  it("lives at least 8 s with an action, so the button can be reached", () => {
    expect(toastLifetime({ kind: "info" })).toBe(4000);
    expect(toastLifetime({ kind: "info", action: { label: "Undo", run: () => {} } })).toBe(8000);
    expect(toastLifetime({ kind: "error", action: { label: "Retry", run: () => {} } })).toBe(8000);
  });
});

describe("pausable toast timer", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("fires after its time when nothing pauses it", () => {
    const done = vi.fn();
    createPausableTimer(4000, done);
    vi.advanceTimersByTime(3999);
    expect(done).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(done).toHaveBeenCalledOnce();
  });

  it("stops while paused and resumes with the time it had left", () => {
    const done = vi.fn();
    const t = createPausableTimer(4000, done, 0);
    vi.advanceTimersByTime(1000);
    t.pause();
    vi.advanceTimersByTime(60_000);
    expect(done).not.toHaveBeenCalled();
    expect(t.remaining()).toBe(3000);
    t.resume();
    vi.advanceTimersByTime(2999);
    expect(done).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(done).toHaveBeenCalledOnce();
  });

  it("gives at least the minimum after a resume, so it does not vanish as the pointer leaves", () => {
    const done = vi.fn();
    const t = createPausableTimer(4000, done, 1500);
    vi.advanceTimersByTime(3900);
    t.pause();
    t.resume();
    vi.advanceTimersByTime(1499);
    expect(done).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(done).toHaveBeenCalledOnce();
  });

  it("never fires once cancelled", () => {
    const done = vi.fn();
    const t = createPausableTimer(100, done);
    t.cancel();
    t.resume();
    vi.advanceTimersByTime(10_000);
    expect(done).not.toHaveBeenCalled();
  });
});

describe("toast announcements", () => {
  const region = (html: string, role: string) => new RegExp(`<div[^>]*role="${role}"[^>]*>(.*?)</div>`).exec(html);
  const view = (live: Toast[], said = nextAnnouncements(NO_ANNOUNCEMENTS, live)) =>
    renderToStaticMarkup(<ToastsView queue={live.map((t) => ({ ...t, leaving: false }))} said={said} onHub={false} />);

  it("keeps a polite and an assertive live region in the DOM even with no toasts", () => {
    const html = view([]);
    const polite = region(html, "status");
    const alert = region(html, "alert");
    expect(polite?.[0]).toContain('aria-live="polite"');
    expect(alert?.[0]).toContain('aria-live="assertive"');
    expect(polite?.[1]).toBe("");
    expect(alert?.[1]).toBe("");
  });

  it("puts each new toast's text in the region for its kind, and the toasts themselves carry no role", () => {
    const html = view([
      { id: 1, kind: "success", message: "Saved version 3" },
      { id: 2, kind: "error", message: "Could not export" },
      { id: 3, kind: "info", message: "Draft discarded" },
    ]);
    expect(region(html, "status")?.[1]).toBe("<span>Draft discarded</span>");
    expect(region(html, "alert")?.[1]).toBe("<span>Could not export</span>");
    // Read once: only the two live regions have a role.
    expect(html.match(/role="(status|alert)"/g)).toHaveLength(2);
    expect(html).toContain("Saved version 3");
  });

  it("announces the newest of each kind, so a new toast replaces the last one said", () => {
    const said = nextAnnouncements(NO_ANNOUNCEMENTS, [
      { id: 1, kind: "info", message: "a" },
      { id: 2, kind: "error", message: "b" },
      { id: 3, kind: "success", message: "c" },
      { id: 4, kind: "error", message: "d" },
    ]);
    expect(said.polite?.id).toBe(3);
    expect(said.alert?.id).toBe(4);
    expect(nextAnnouncements(NO_ANNOUNCEMENTS, [])).toBe(NO_ANNOUNCEMENTS);
  });

  it("says each message once, when it arrives, and never again when a newer toast leaves first", () => {
    const success: Toast = { id: 1, kind: "success", message: "Saved version 3" };
    const info: Toast = { id: 2, kind: "info", message: "Draft discarded" };
    const error: Toast = { id: 3, kind: "error", message: "Could not export" };
    const politeText = (said: ReturnType<typeof nextAnnouncements>, live: Toast[]) => /role="status"[^>]*>(.*?)<\/div>/.exec(view(live, said))?.[1];

    // Each arrival changes what the region holds, so each is spoken once.
    const spoken: string[] = [];
    let said = NO_ANNOUNCEMENTS;
    const step = (live: Toast[]) => {
      const next = nextAnnouncements(said, live);
      if (next.polite !== said.polite && next.polite) spoken.push(next.polite.message);
      if (next.alert !== said.alert && next.alert) spoken.push(next.alert.message);
      said = next;
      return politeText(said, live);
    };

    expect(step([success])).toBe("<span>Saved version 3</span>");
    expect(step([success, info])).toBe("<span>Draft discarded</span>");
    // The 4 s info leaves before the 4.5 s success: the region keeps
    // "Draft discarded" and does not speak "Saved version 3" again.
    const before = said;
    expect(step([success])).toBe("<span>Draft discarded</span>");
    expect(said).toBe(before);
    // Closing a toast by hand, the error arriving and every toast leaving: no repeats either.
    step([success, error]);
    step([error]);
    step([]);
    expect(spoken).toEqual(["Saved version 3", "Draft discarded", "Could not export"]);
  });
});
