import type { CSSProperties } from "react";
import type { Accent } from "../../api/gen/Accent.ts";
import type { BoardAppearance } from "../../api/gen/BoardAppearance.ts";
import type { BoardBackground } from "../../api/gen/BoardBackground.ts";
import type { BoardDensity } from "../../api/gen/BoardDensity.ts";
import type { TileSurface } from "../../api/gen/TileSurface.ts";
import type { Rect } from "../../boards/grid.ts";

/** Accent presets: one colour each, readable on light and dark cards. */
export const ACCENTS: Record<Accent, { label: string; color: string }> = {
  blue: { label: "Blue", color: "#3b82f6" },
  violet: { label: "Violet", color: "#8b5cf6" },
  green: { label: "Green", color: "#22c55e" },
  amber: { label: "Amber", color: "#f59e0b" },
  rose: { label: "Rose", color: "#f43f5e" },
  teal: { label: "Teal", color: "#14b8a6" },
  slate: { label: "Slate", color: "#64748b" },
};
export const ACCENT_NAMES = Object.keys(ACCENTS) as Accent[];

/** Background presets (CSS in board.css); no custom images: the CSP only
 * allows same-origin images. */
export const BACKGROUNDS: { value: BoardBackground; label: string }[] = [
  { value: "none", label: "None" }, { value: "dawn", label: "Dawn" }, { value: "dusk", label: "Dusk" },
  { value: "aurora", label: "Aurora" }, { value: "ocean", label: "Ocean" }, { value: "forest", label: "Forest" },
  { value: "graphite", label: "Graphite" },
];
export const DENSITIES: { value: BoardDensity; label: string }[] = [{ value: "comfortable", label: "Comfortable" }, { value: "compact", label: "Compact" }];
export const SURFACES: { value: TileSurface; label: string }[] = [{ value: "solid", label: "Solid" }, { value: "glass", label: "Glass" }, { value: "outline", label: "Outline" }];

/** The attributes a board page carries for its appearance. */
export function boardLook(appearance: BoardAppearance | undefined) {
  const a = appearance ?? { background: "none", density: "comfortable", surface: "solid" };
  return {
    "data-bg": a.background === "none" ? undefined : a.background,
    "data-density": a.density,
    "data-surface": a.surface,
    style: a.accent ? { "--board-accent": ACCENTS[a.accent].color } as CSSProperties : undefined,
  };
}

/** A cell's place on the canvas, as CSS variables (board.css does the maths). */
export const cellVars = (r: Rect): CSSProperties => ({ "--x": r.x, "--y": r.y, "--w": r.w, "--h": r.h } as CSSProperties);
export const canvasVars = (rows: number): CSSProperties => ({ "--rows": rows } as CSSProperties);
