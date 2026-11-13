import { describe, expect, it } from "vitest";
import { TCT_EMPTY, parseTct } from "./tct";

const TITLE =
  'A parcel of land (Lot 5, Block 3 of the subdivision plan Psd-04-012345), situated in Barangay Pasong Tamo, Quezon City. Bounded on the NE., along line 1-2 by Lot 4; along line 2-3 by Road Lot 1. Beginning at a point marked "1" on plan, being N. 45 deg. 30\' E., 1234.56 m. from BLLM No. 1, Quezon City; thence S. 44 deg. 30\' E., 12.00 m. to point 2; thence S. 45 deg. 30\' W., 15.00 m. to point 3; thence N. 44 deg. 30\' W., 12.00 m. to point 4; thence N. 45 deg. 30\' E., 15.00 m. to the point of beginning; containing an area of ONE HUNDRED EIGHTY (180) SQUARE METERS, more or less.';

describe("technical description parser", () => {
  it("reads the legs after each thence and skips the tie line", () => {
    const r = parseTct(TITLE);
    expect(r).toEqual({
      ok: true,
      legs: [
        { bearing: "S. 44 deg. 30' E.", distance_mm: 12000 },
        { bearing: "S. 45 deg. 30' W.", distance_mm: 15000 },
        { bearing: "N. 44 deg. 30' W.", distance_mm: 12000 },
        { bearing: "N. 45 deg. 30' E.", distance_mm: 15000 },
      ],
    });
  });

  it("reads a title broken over lines, with curly quotes", () => {
    const text = "Beginning at point 1, being S. 10 deg. W., 300.5 m. from BBM No. 3;\nthence N. 80 deg. 15’ E.,\n20.25 m. to point 2;\nthence S. 9 deg. 45’ E., 10.00 m. to point 3;\nthence S. 80 deg. 15’ W., 20.25 m. to the point of beginning.";
    const r = parseTct(text);
    if (!r.ok) throw new Error(r.error);
    expect(r.legs.map((l) => l.bearing)).toEqual(["N. 80 deg. 15' E.", "S. 9 deg. 45' E.", "S. 80 deg. 15' W."]);
    expect(r.legs.map((l) => l.distance_mm)).toEqual([20250, 10000, 20250]);
  });

  it("reads a plain list, one leg per line", () => {
    const r = parseTct("N 45-30 E 15.00\nS 12°05' W, 8.5 m\nS 44 30 E 12\nN 1°02'03\" W 7500 mm");
    expect(r).toEqual({
      ok: true,
      legs: [
        { bearing: "N 45-30 E", distance_mm: 15000 },
        { bearing: "S 12°05' W", distance_mm: 8500 },
        { bearing: "S 44 30 E", distance_mm: 12000 },
        { bearing: 'N 1°02\'03" W', distance_mm: 7500 },
      ],
    });
  });

  it("reads thousands separators and leaves out a plain list's tie line", () => {
    const r = parseTct("N 45-30 E 1,234.56 m from BLLM No. 1\nS 44-30 E 12.00");
    expect(r).toEqual({ ok: true, legs: [{ bearing: "S 44-30 E", distance_mm: 12000 }] });
  });

  it("says so when there are no legs", () => {
    expect(parseTct("")).toEqual({ ok: false, error: TCT_EMPTY });
    expect(parseTct("Lot 5, Block 3, Quezon City. North by Lot 4.")).toEqual({ ok: false, error: TCT_EMPTY });
    expect(parseTct("thence along the creek to the point of beginning")).toEqual({ ok: false, error: TCT_EMPTY });
  });
});
