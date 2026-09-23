// Linework plan drawings used where no real thumbnail exists yet, and as the
// hub's brand drawing. Pure SVG, drawn with the same thin strokes as the icons.

const VARIANTS = [
  // L-shaped bungalow
  {
    walls: "M30 30h150v50h60v90H30zM30 100h80M110 30v70M110 130v40M180 80v90",
    swings: "M110 100v22a22 22 0 0 0 22-22M180 124h-20a20 20 0 0 1 20-20",
    windows: "M60 30h30M135 30h30M240 110v30M60 170h30",
  },
  // compact rectangle, three rooms
  {
    walls: "M40 40h190v120H40zM130 40v120M130 100h100",
    swings: "M130 122h-22a22 22 0 0 1 22-22M176 100v-20a20 20 0 0 1 20 20",
    windows: "M70 40h36M160 40h40M230 118v26M70 160h36",
  },
  // long house with porch
  {
    walls: "M26 50h218v100H26zM96 50v100M170 50v60M170 110h74M26 150v26h70v-26",
    swings: "M96 84h20a20 20 0 0 1-20 20M170 132v-22a22 22 0 0 0-22 22",
    windows: "M46 50h30M120 50h30M196 50h30M244 70v24M200 150h28",
  },
];

function hash(text: string): number {
  let h = 0;
  for (let i = 0; i < text.length; i++) h = (h * 31 + text.charCodeAt(i)) >>> 0;
  return h;
}

function PlanLines({ v }: { v: (typeof VARIANTS)[number] }) {
  return (
    <>
      <path d={v.walls} fill="none" stroke="var(--ink-3)" strokeWidth="2.5" strokeLinejoin="miter" opacity="0.75" />
      <path d={v.windows} fill="none" stroke="var(--paper)" strokeWidth="3.5" />
      <path d={v.windows} fill="none" stroke="var(--blueprint)" strokeWidth="1" opacity="0.7" />
      <path d={v.swings} fill="none" stroke="var(--accent)" strokeWidth="1" opacity="0.8" />
    </>
  );
}

/** Placeholder thumbnail. `seed` keeps the same drawing for the same project. */
export function PlanSketch({ seed, className }: { seed: string; className?: string }) {
  const v = VARIANTS[hash(seed) % VARIANTS.length];
  return (
    <svg viewBox="0 0 270 200" className={className} preserveAspectRatio="xMidYMid meet" aria-hidden>
      <defs>
        <pattern id={`g-${hash(seed) % 997}`} width="10" height="10" patternUnits="userSpaceOnUse">
          <path d="M10 0H0v10" fill="none" stroke="var(--draw-grid)" strokeWidth="1" />
        </pattern>
      </defs>
      <rect width="270" height="200" fill={`url(#g-${hash(seed) % 997})`} />
      <PlanLines v={v} />
    </svg>
  );
}

/**
 * The plumbing template's preview: the sample bungalow's sketch (the same
 * drawing as seed "b") with a run in each pipe system color.
 */
export function PlumbingSketch({ className }: { className?: string }) {
  const v = VARIANTS[hash("b") % VARIANTS.length];
  return (
    <svg viewBox="0 0 270 200" className={className} preserveAspectRatio="xMidYMid meet" aria-hidden>
      <defs>
        <pattern id="g-plumbing" width="10" height="10" patternUnits="userSpaceOnUse">
          <path d="M10 0H0v10" fill="none" stroke="var(--draw-grid)" strokeWidth="1" />
        </pattern>
      </defs>
      <rect width="270" height="200" fill="url(#g-plumbing)" />
      <PlanLines v={v} />
      <g fill="none" strokeLinecap="round" strokeLinejoin="round">
        <path d="M262 130H206V74H186" stroke="var(--pipe-cold)" strokeWidth="1.7" />
        <path d="M226 130V82H194" stroke="var(--pipe-hot)" strokeWidth="1.7" />
        <path d="M186 62H236V164H262" stroke="var(--pipe-drain)" strokeWidth="2.3" />
        <circle cx="236" cy="62" r="3.4" stroke="var(--pipe-vent)" strokeWidth="1.7" fill="var(--paper)" />
      </g>
    </svg>
  );
}

/** Blank template preview: grid and an origin cross. */
export function BlankSketch({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 270 200" className={className} preserveAspectRatio="xMidYMid meet" aria-hidden>
      <defs>
        <pattern id="g-blank" width="10" height="10" patternUnits="userSpaceOnUse">
          <path d="M10 0H0v10" fill="none" stroke="var(--draw-grid)" strokeWidth="1" />
        </pattern>
      </defs>
      <rect width="270" height="200" fill="url(#g-blank)" />
      <path d="M135 86v28M121 100h28" stroke="var(--ink-3)" strokeWidth="1.2" />
      <circle cx="135" cy="100" r="5" fill="none" stroke="var(--accent)" strokeWidth="1.2" />
    </svg>
  );
}

/** The large drawing on the hub's brand panel: plan below, elevation above, tied by projection lines. */
export function BrandDrawing({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 300 420" className={className} fill="none" preserveAspectRatio="xMidYMax meet" aria-hidden>
      {/* projection lines from plan to elevation */}
      <g stroke="rgba(223,231,240,0.16)" strokeWidth="1" strokeDasharray="2 5">
        <path d="M40 150v90M260 150v90M150 78v162" />
      </g>
      {/* elevation */}
      <g stroke="rgba(223,231,240,0.72)" strokeWidth="1.25" strokeLinejoin="miter">
        <path d="M22 110L150 58l128 52" />
        <path d="M40 103v47h220v-47" />
        <path d="M16 150h268" />
        <path d="M128 150v-34h24v34M70 116h34v20H70zM196 116h42v20h-42z" />
        <path d="M87 116v20M217 116v20" strokeOpacity="0.5" />
      </g>
      {/* plan */}
      <g stroke="rgba(223,231,240,0.78)" strokeWidth="1.25" strokeLinejoin="miter">
        <path d="M40 240h220v140H40zM46 246h208v128H46z" />
        <path d="M150 246v70M150 344v30M46 316h70M144 316h12" />
        <path d="M156 300h98" />
      </g>
      <g stroke="var(--accent)" strokeWidth="1.25">
        <path d="M116 316a28 28 0 0 1 28-28v28" strokeOpacity="0.95" />
        <path d="M150 344a28 28 0 0 1 28-28" strokeOpacity="0.95" />
        <path d="M150 316h28" strokeOpacity="0.95" />
      </g>
      {/* dimension string */}
      <g stroke="rgba(223,231,240,0.42)" strokeWidth="1">
        <path d="M40 400h220M40 394v12M260 394v12M150 394v12M36 404l8-8M146 404l8-8M256 404l8-8" />
      </g>
      <g fill="rgba(223,231,240,0.5)" fontSize="9" fontFamily="var(--font-mono)" textAnchor="middle">
        <text x="95" y="416">5 500</text>
        <text x="205" y="416">5 500</text>
      </g>
    </svg>
  );
}
