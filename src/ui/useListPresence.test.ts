import { describe, expect, it } from "vitest";
import { mergeRows, type PresentRow } from "./useListPresence";

const key = (s: string) => s;
const rows = (list: Array<PresentRow<string>>) => list.map((r) => `${r.key}${r.entering ? "+" : ""}${r.leaving ? "-" : ""}`);

describe("mergeRows", () => {
  it("does not mark the first rows as entering", () => {
    expect(rows(mergeRows([], ["a", "b"], key, false))).toEqual(["a", "b"]);
  });

  it("marks rows that arrive later as entering", () => {
    const first = mergeRows([], ["a", "b"], key, false);
    expect(rows(mergeRows(first, ["a", "c", "b"], key, true))).toEqual(["a", "c+", "b"]);
  });

  it("keeps a removed row in its place, leaving", () => {
    const first = mergeRows([], ["a", "b", "c"], key, false);
    expect(rows(mergeRows(first, ["a", "c"], key, true))).toEqual(["a", "b-", "c"]);
    expect(rows(mergeRows(first, ["b", "c"], key, true))).toEqual(["a-", "b", "c"]);
  });

  it("brings a leaving row back when it returns", () => {
    const first = mergeRows([], ["a", "b"], key, false);
    const gone = mergeRows(first, ["a"], key, true);
    expect(rows(mergeRows(gone, ["a", "b"], key, true))).toEqual(["a", "b"]);
  });
});
