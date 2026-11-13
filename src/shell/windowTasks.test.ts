import { describe, expect, it } from "vitest";
import { useApp } from "../state/store";
import { PREVIEW_MAX_PX, exportStatus, fitSize, queueStatus, runWindowTask } from "./windowTasks";

// G4 problem 8: after a render came back "done", the window kept rendering
// and refused the next with no word of what it was doing. It now reports its
// queue to the backend, which MCP's `render` status and refusals read.
describe("queueStatus", () => {
  it("says which view of how many, how far and how long is left", () => {
    const status = queueStatus(
      {
        running: true,
        current: { index: 1, total: 2, name: "Front" },
        progress: { samples: 150, targetSamples: 200, elapsedMs: 40_000, budgetMs: 80_000, etaMs: 29_240 },
      },
      "job-1",
    );
    expect(status).toEqual({
      busy: true,
      what: "Rendering for your agent",
      view: "Front",
      index: 1,
      total: 2,
      progress_pct: 75,
      eta_s: 29.2,
      job_id: "job-1",
    });
  });

  it("says how far a tour video export is, and how long is left at its rate", () => {
    const at = exportStatus({ done: 1110, total: 4440, phase: "Rendering" }, 0, 44_400, "job-2");
    expect([at.what, at.index, at.total, at.progress_pct, at.eta_s, at.job_id]).toEqual(["Exporting a tour video (rendering)", 1110, 4440, 25, 133, "job-2"]);
    expect(exportStatus(null, 0, 1000, "job-2").progress_pct).toBeNull();
  });

  it("is idle when nothing runs, and a render the user started has no job", () => {
    expect(queueStatus({ running: false, current: null, progress: null }, "job-1").busy).toBe(false);
    const own = queueStatus({ running: true, current: { index: 1, total: 1, name: "View" }, progress: null }, null);
    expect([own.what, own.progress_pct, own.eta_s, own.job_id]).toEqual(["Rendering", null, null, null]);
  });
});

describe("fitSize", () => {
  it("fits the long side and keeps the aspect", () => {
    expect(fitSize(3840, 2160, PREVIEW_MAX_PX)).toEqual({ width: 1568, height: 882 });
    expect(fitSize(2048, 2048, PREVIEW_MAX_PX)).toEqual({ width: 1568, height: 1568 });
    expect(fitSize(1400, 2000, PREVIEW_MAX_PX)).toEqual({ width: 1098, height: 1568 });
  });

  it("never enlarges and never reaches zero", () => {
    expect(fitSize(800, 600, PREVIEW_MAX_PX)).toEqual({ width: 800, height: 600 });
    expect(fitSize(0, 0, PREVIEW_MAX_PX)).toEqual({ width: 1, height: 1 });
  });
});

describe("runWindowTask", () => {
  it("answers no_document while the window shows the hub", async () => {
    useApp.setState({ screen: "hub", doc: null });
    await expect(runWindowTask({ id: "r1", task: { type: "capture_plan", level_id: null } })).rejects.toMatchObject({
      code: "no_document",
    });
    await expect(
      runWindowTask({ id: "r2", task: { type: "render", views: [], quality: "quick", size: "hd", visibility: null } }),
    ).rejects.toMatchObject({ code: "no_document" });
  });
});
