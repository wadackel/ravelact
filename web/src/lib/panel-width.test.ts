import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  clampPanelWidth,
  DEFAULT_PANEL_WIDTH,
  MAX_PANEL_WIDTH_CAP,
  MIN_PANEL_WIDTH,
  panelMaxWidth,
  readPersistedWidth,
  STORAGE_KEY,
  writePersistedWidth,
} from "./panel-width.ts";

describe("panelMaxWidth", () => {
  it("returns MAX_CAP on wide viewports", () => {
    expect(panelMaxWidth(1200)).toBe(MAX_PANEL_WIDTH_CAP);
    expect(panelMaxWidth(2000)).toBe(MAX_PANEL_WIDTH_CAP);
  });

  it("returns floor(viewport*0.6) on mid viewports (MIN < 60vw < MAX_CAP)", () => {
    expect(panelMaxWidth(800)).toBe(480);
    expect(panelMaxWidth(801)).toBe(480); // floors 480.6
    expect(panelMaxWidth(600)).toBe(360);
  });

  it("returns MIN when 60vw is below MIN (degenerate narrow viewport)", () => {
    expect(panelMaxWidth(400)).toBe(MIN_PANEL_WIDTH);
    expect(panelMaxWidth(0)).toBe(MIN_PANEL_WIDTH);
  });

  it("always returns an integer", () => {
    expect(Number.isInteger(panelMaxWidth(801))).toBe(true);
    expect(Number.isInteger(panelMaxWidth(1333))).toBe(true);
  });
});

describe("clampPanelWidth", () => {
  it("returns the value unchanged when inside [MIN, panelMaxWidth(vw)]", () => {
    expect(clampPanelWidth(360, 1200)).toBe(360);
    expect(clampPanelWidth(500, 1200)).toBe(500);
  });

  it("clamps to MIN when below MIN", () => {
    expect(clampPanelWidth(50, 1200)).toBe(MIN_PANEL_WIDTH);
    expect(clampPanelWidth(0, 1200)).toBe(MIN_PANEL_WIDTH);
    expect(clampPanelWidth(-100, 1200)).toBe(MIN_PANEL_WIDTH);
  });

  it("clamps to MAX_CAP on wide viewports (60vw >= MAX_CAP)", () => {
    expect(clampPanelWidth(9999, 1200)).toBe(MAX_PANEL_WIDTH_CAP);
    expect(clampPanelWidth(9999, 2000)).toBe(MAX_PANEL_WIDTH_CAP);
  });

  it("clamps to viewport*0.6 on narrow viewports (MIN < 60vw < MAX_CAP)", () => {
    expect(clampPanelWidth(9999, 600)).toBe(360);
    expect(clampPanelWidth(9999, 800)).toBe(480);
  });

  it("never returns a value below MIN even on a degenerate narrow viewport", () => {
    // 60vw of 400 is 240 (< MIN). The pane is pinned to MIN.
    expect(clampPanelWidth(9999, 400)).toBe(MIN_PANEL_WIDTH);
  });

  it("returns an integer for cap even when viewportWidth*0.6 is fractional", () => {
    // 60vw of 801 is 480.6 → floors to 480.
    expect(clampPanelWidth(9999, 801)).toBe(480);
  });
});

describe("readPersistedWidth", () => {
  beforeEach(() => {
    localStorage.clear();
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("returns DEFAULT_PANEL_WIDTH when storage is empty", () => {
    expect(readPersistedWidth()).toBe(DEFAULT_PANEL_WIDTH);
  });

  it("returns the parsed integer when in range", () => {
    localStorage.setItem(STORAGE_KEY, "500");
    expect(readPersistedWidth()).toBe(500);
  });

  it("falls back to DEFAULT_PANEL_WIDTH when value is non-numeric", () => {
    localStorage.setItem(STORAGE_KEY, "abc");
    expect(readPersistedWidth()).toBe(DEFAULT_PANEL_WIDTH);
  });

  it("falls back when value is empty string", () => {
    localStorage.setItem(STORAGE_KEY, "");
    expect(readPersistedWidth()).toBe(DEFAULT_PANEL_WIDTH);
  });

  it("rejects trailing garbage (parseInt-style accept-prefix is intentionally disallowed)", () => {
    localStorage.setItem(STORAGE_KEY, "300abc");
    expect(readPersistedWidth()).toBe(DEFAULT_PANEL_WIDTH);
  });

  it("rejects fractional values (no implicit truncation)", () => {
    localStorage.setItem(STORAGE_KEY, "300.5");
    expect(readPersistedWidth()).toBe(DEFAULT_PANEL_WIDTH);
  });

  it("rejects values with leading/trailing whitespace", () => {
    localStorage.setItem(STORAGE_KEY, " 400");
    expect(readPersistedWidth()).toBe(DEFAULT_PANEL_WIDTH);
    localStorage.setItem(STORAGE_KEY, "400 ");
    expect(readPersistedWidth()).toBe(DEFAULT_PANEL_WIDTH);
  });

  it("falls back when value is below MIN", () => {
    localStorage.setItem(STORAGE_KEY, "-10");
    expect(readPersistedWidth()).toBe(DEFAULT_PANEL_WIDTH);
    localStorage.setItem(STORAGE_KEY, "100");
    expect(readPersistedWidth()).toBe(DEFAULT_PANEL_WIDTH);
  });

  it("falls back when value is above MAX_CAP", () => {
    localStorage.setItem(STORAGE_KEY, "99999");
    expect(readPersistedWidth()).toBe(DEFAULT_PANEL_WIDTH);
    localStorage.setItem(STORAGE_KEY, "721");
    expect(readPersistedWidth()).toBe(DEFAULT_PANEL_WIDTH);
  });

  it("accepts the inclusive boundary values MIN and MAX_CAP", () => {
    localStorage.setItem(STORAGE_KEY, String(MIN_PANEL_WIDTH));
    expect(readPersistedWidth()).toBe(MIN_PANEL_WIDTH);
    localStorage.setItem(STORAGE_KEY, String(MAX_PANEL_WIDTH_CAP));
    expect(readPersistedWidth()).toBe(MAX_PANEL_WIDTH_CAP);
  });

  it("falls back when localStorage.getItem throws", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    expect(readPersistedWidth()).toBe(DEFAULT_PANEL_WIDTH);
  });
});

describe("writePersistedWidth", () => {
  beforeEach(() => {
    localStorage.clear();
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("writes the integer value as a string", () => {
    writePersistedWidth(420);
    expect(localStorage.getItem(STORAGE_KEY)).toBe("420");
  });

  it("warns and does not re-throw when setItem fails", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceeded");
    });
    expect(() => writePersistedWidth(500)).not.toThrow();
    expect(warn).toHaveBeenCalledOnce();
  });
});
