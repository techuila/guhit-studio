// Reads the legs of a lot out of its title's technical description (TCT), or
// out of a plain list of legs, one per line. Pure, tested in tct.test.ts.
//
// The bearing text is kept as written ("S. 44 deg. 30' E."): the engine
// parses it and refuses a bad one with a hint (`SetLotFromSurvey`). The tie
// line to a monument ("... 1234.56 m. from BLLM No. 1") is not a side.
// Distances are meters unless "mm" is written.

import type { SurveyLeg } from "../../contract/bindings";

export type TctResult = { ok: true; legs: SurveyLeg[] } | { ok: false; error: string };

export const TCT_EMPTY =
  "No legs found. Paste the technical description from the title, or one leg per line like N 45-30 E 15.00.";

/**
 * A quadrant bearing: N or S, degrees, optional minutes and seconds, E or W.
 * Separators as titles and surveyors write them: "deg.", "°", "-", "'", '"',
 * "min.", "sec." or plain spaces.
 */
const BEARING = new RegExp(
  [
    String.raw`(?<![A-Za-z])([NS])\.?\s*`,
    String.raw`(\d{1,3}(?:\.\d+)?)\s*(?:°|º|deg(?:rees?)?\.?|-|\s)\s*`,
    String.raw`(?:(\d{1,2}(?:\.\d+)?)\s*(?:'|’|′|min(?:utes?)?\.?|-|\s)?\s*)?`,
    String.raw`(?:(\d{1,2}(?:\.\d+)?)\s*(?:"|”|″|''|sec(?:onds?)?\.?)\s*)?`,
    String.raw`([EW])(?![A-Za-z])\.?`,
  ].join(""),
  "i",
);

/** A distance right after the bearing: "12.00 m.", "15.00", "8500 mm", "1,234.56 meters". */
const DISTANCE = /^\s*[,;:]?\s*(\d[\d,]*(?:\.\d+)?|\.\d+)\s*(mm\b|millimet(?:er|re)s?\b|m\b\.?|mts?\b\.?|met(?:er|re)s?\b)?/i;

/** Text that follows a tie line's distance. */
const TIE = /^\W*from\b/i;

function legIn(text: string): { leg: SurveyLeg; tie: boolean } | null {
  const b = BEARING.exec(text);
  if (!b) return null;
  const rest = text.slice(b.index + b[0].length);
  const d = DISTANCE.exec(rest);
  if (!d) return null;
  const value = Number(d[1].replace(/,/g, ""));
  if (!Number.isFinite(value) || value <= 0) return null;
  const mm = /^mm|^millimet/i.test(d[2] ?? "");
  const distance_mm = Math.round((mm ? value : value * 1000) * 1000) / 1000;
  const bearing = b[0].trim().replace(/[,;]+$/, "");
  return { leg: { bearing, distance_mm }, tie: TIE.test(rest.slice(d[0].length)) };
}

/** The legs of a technical description, or of a list of legs one per line. */
export function parseTct(input: string): TctResult {
  const text = input.replace(/\r\n?/g, "\n").replace(/[‘’`]/g, "'").replace(/[“”]/g, '"');
  const legs: SurveyLeg[] = [];
  if (/\bthence\b/i.test(text)) {
    // Each leg follows a "thence"; what comes before the first one is the
    // point of beginning and its tie to a monument.
    const parts = text.split(/\bthence\b/i).slice(1);
    for (const part of parts) {
      const found = legIn(part.replace(/\s+/g, " "));
      if (found && !found.tie) legs.push(found.leg);
    }
  } else {
    for (const line of text.split(/[\n;]+/)) {
      const found = legIn(line.replace(/\s+/g, " "));
      if (found && !found.tie) legs.push(found.leg);
    }
  }
  return legs.length > 0 ? { ok: true, legs } : { ok: false, error: TCT_EMPTY };
}
