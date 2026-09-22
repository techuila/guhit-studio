import { describe, expect, it } from "vitest";
import { decideResize } from "./resizePolicy";

describe("decideResize", () => {
  it("shrink burst: every intermediate tick recenters, never fits, even as the pane shrinks toward tiny", () => {
    const sizes = [1400, 900, 500, 120, 48];
    for (const width of sizes) {
      expect(decideResize({ autoFit: true, settled: false, width, height: 543 })).toBe("recenter");
    }
    // Only the trailing edge, after the burst, may fit.
    expect(decideResize({ autoFit: true, settled: true, width: 48, height: 543 })).toBe("fit");
  });

  it("grow burst: every intermediate tick recenters, the settled trailing edge fits to the final size", () => {
    const sizes = [48, 300, 900, 1440];
    for (const width of sizes) {
      expect(decideResize({ autoFit: true, settled: false, width, height: 900 })).toBe("recenter");
    }
    expect(decideResize({ autoFit: true, settled: true, width: 1440, height: 900 })).toBe("fit");
  });

  it("user zoomed: auto fit is off, so even the settled trailing edge only recenters, scale never changes", () => {
    expect(decideResize({ autoFit: false, settled: false, width: 700, height: 500 })).toBe("recenter");
    expect(decideResize({ autoFit: false, settled: true, width: 1440, height: 900 })).toBe("recenter");
  });

  it("tiny intermediate size: never fits mid-burst no matter how small, so a small intermediate frame cannot become the final scale", () => {
    expect(decideResize({ autoFit: true, settled: false, width: 1, height: 1 })).toBe("recenter");
    expect(decideResize({ autoFit: true, settled: false, width: 6, height: 543 })).toBe("recenter");
  });

  it("zero size: ignored regardless of auto fit or settled state", () => {
    expect(decideResize({ autoFit: true, settled: true, width: 0, height: 900 })).toBe("ignore");
    expect(decideResize({ autoFit: true, settled: true, width: 1440, height: 0 })).toBe("ignore");
    expect(decideResize({ autoFit: false, settled: false, width: 0, height: 0 })).toBe("ignore");
  });
});
