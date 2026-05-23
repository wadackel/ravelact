// Shared width state for the `ravelact browse` right pane (Panel + OverviewPane).
// Effective bounds: [MIN, max(MIN, floor(min(MAX_CAP, viewportWidth * 0.6)))].
// On viewports narrow enough that `viewportWidth * 0.6 < MIN`, the floor is
// pinned to MIN — the pane cannot be smaller than MIN even if it ends up
// wider than 60% of the viewport. The cap is always an integer so written
// values round-trip cleanly through localStorage.
// readPersistedWidth only enforces the absolute [MIN, MAX_CAP] range; the
// viewport-relative trim is applied at render time by ResizableRightPane.

export const DEFAULT_PANEL_WIDTH = 360;
export const MIN_PANEL_WIDTH = 280;
export const MAX_PANEL_WIDTH_CAP = 720;
export const STORAGE_KEY = "ravelact:panel-width";

export function panelMaxWidth(viewportWidth: number): number {
  return Math.max(MIN_PANEL_WIDTH, Math.floor(Math.min(MAX_PANEL_WIDTH_CAP, viewportWidth * 0.6)));
}

export function clampPanelWidth(width: number, viewportWidth: number): number {
  const cap = panelMaxWidth(viewportWidth);
  if (width < MIN_PANEL_WIDTH) return MIN_PANEL_WIDTH;
  if (width > cap) return cap;
  return width;
}

// Strict integer regex — `Number.parseInt` would silently accept "300abc",
// "280.5", " 500" etc. `localStorage` is a trust boundary (user can edit
// values via DevTools, stale values from prior versions may persist), so
// reject anything that is not a bare optional-minus + digits.
const INTEGER_RE = /^-?\d+$/;

export function readPersistedWidth(): number {
  let raw: string | null;
  try {
    raw = localStorage.getItem(STORAGE_KEY);
  } catch {
    return DEFAULT_PANEL_WIDTH;
  }
  if (raw === null || !INTEGER_RE.test(raw)) return DEFAULT_PANEL_WIDTH;
  const n = Number(raw);
  if (n < MIN_PANEL_WIDTH || n > MAX_PANEL_WIDTH_CAP) return DEFAULT_PANEL_WIDTH;
  return n;
}

export function writePersistedWidth(width: number): void {
  try {
    localStorage.setItem(STORAGE_KEY, String(width));
  } catch (err) {
    console.warn("ravelact: failed to persist panel width", err);
  }
}
