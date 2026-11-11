// The compare slider names the AI image once (V19): the corner badge every AI
// image carries (DECISIONS D8), not a second label beside it.
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { CompareSlider } from "./CompareSlider";

const count = (html: string, text: string) => html.split(text).length - 1;
const noop = () => {};

describe("compare slider labels", () => {
  it("says AI visualization once on an AI image, and still names the model side", () => {
    const html = renderToStaticMarkup(<CompareSlider beforeSrc="a.png" afterSrc="b.png" value={50} onChange={noop} />);
    expect(html).toContain('data-testid="ai-badge"');
    // The badge, plus the image's alt text for screen readers.
    expect(count(html, ">AI visualization<")).toBe(1);
    expect(html).toContain(">Model view<");
  });

  it("keeps a right label that says more than the badge", () => {
    const html = renderToStaticMarkup(
      <CompareSlider beforeSrc="a.png" afterSrc="b.png" afterLabel="Not saved yet" value={50} onChange={noop} />,
    );
    expect(html).toContain(">Not saved yet<");
    expect(count(html, ">AI visualization<")).toBe(1);
  });

  it("shows a capture alone with no AI badge", () => {
    const html = renderToStaticMarkup(<CompareSlider beforeSrc="a.png" afterSrc="" value={50} onChange={noop} />);
    expect(html).not.toContain("AI visualization");
    expect(html).not.toContain('data-testid="compare-handle"');
  });
});
