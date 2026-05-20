import { describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useDebounced } from "./useDebounced.ts";

describe("useDebounced", () => {
  it("returns the initial value synchronously and updates after the delay", () => {
    vi.useFakeTimers();
    try {
      const { result, rerender } = renderHook(({ v }: { v: string }) => useDebounced(v, 100), {
        initialProps: { v: "a" },
      });
      expect(result.current).toBe("a");

      rerender({ v: "b" });
      // Still the previous value before the delay elapses.
      expect(result.current).toBe("a");

      act(() => {
        vi.advanceTimersByTime(99);
      });
      expect(result.current).toBe("a");

      act(() => {
        vi.advanceTimersByTime(1);
      });
      expect(result.current).toBe("b");
    } finally {
      vi.useRealTimers();
    }
  });

  it("coalesces rapid changes — only the final value lands", () => {
    vi.useFakeTimers();
    try {
      const { result, rerender } = renderHook(({ v }: { v: string }) => useDebounced(v, 50), {
        initialProps: { v: "x" },
      });
      rerender({ v: "y" });
      act(() => {
        vi.advanceTimersByTime(30);
      });
      rerender({ v: "z" });
      act(() => {
        vi.advanceTimersByTime(49);
      });
      // Within 50 ms of the latest change → still the original.
      expect(result.current).toBe("x");
      act(() => {
        vi.advanceTimersByTime(1);
      });
      expect(result.current).toBe("z");
    } finally {
      vi.useRealTimers();
    }
  });
});
