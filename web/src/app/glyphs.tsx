// The icons repeated in long lists (every table row, every sidebar item),
// as constant elements: one <svg> and one <path> each, no component to run
// per row (Lucide's icon component costs a render and a few objects every
// time; with 150 rows that showed in the load profile). Paths are Lucide's
// (ISC), merged into one path per icon.
const svg = { viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 2, strokeLinecap: "round", strokeLinejoin: "round", "aria-hidden": true } as const;

export const LOGS_ICON = (
  <svg {...svg} className="lucide lucide-scroll-text" width={24} height={24}>
    <path d="M15 12h-5M15 8h-5M19 17V5a2 2 0 0 0-2-2H4M8 21h12a2 2 0 0 0 2-2v-1a1 1 0 0 0-1-1H11a1 1 0 0 0-1 1v1a2 2 0 1 1-4 0V5a2 2 0 1 0-4 0v2a1 1 0 0 0 1 1h3" />
  </svg>
);

export const EXTERNAL_ICON = (
  <svg {...svg} className="lucide lucide-external-link" width={24} height={24}>
    <path d="M15 3h6v6M10 14 21 3M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
  </svg>
);

export const CHEVRON_RIGHT_ICON = (
  <svg {...svg} className="lucide lucide-chevron-right i chev" width={24} height={24}>
    <path d="m9 18 6-6-6-6" />
  </svg>
);

export const CHEVRON_DOWN_ICON = (
  <svg {...svg} className="lucide lucide-chevron-down chev" width={24} height={24}>
    <path d="m6 9 6 6 6-6" />
  </svg>
);
