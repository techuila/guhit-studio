import { describe, expect, it } from "vitest";
import { useApp } from "../state/store";
import { PREVIEW_MAX_PX, fitSize, runWindowTask } from "./windowTasks";

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
      runWindowTask({ id: "r2", task: { type: "render", views: [], quality: "quick", size: "hd" } }),
    ).rejects.toMatchObject({ code: "no_document" });
  });
});
