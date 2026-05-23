import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render } from "@testing-library/react";
import { ResizableRightPane } from "./ResizableRightPane.tsx";

describe("ResizableRightPane", () => {
  beforeEach(() => {
    vi.stubGlobal("innerWidth", 1200);
  });
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("renders its children", () => {
    const { getByText } = render(
      <ResizableRightPane width={360} onWidthChange={() => {}}>
        <div>child-content</div>
      </ResizableRightPane>,
    );
    expect(getByText("child-content")).toBeInTheDocument();
  });

  it("clamps down via onWidthChange when viewport shrinks below 60vw cap", () => {
    // Viewport: 800px → cap = min(720, 800*0.6) = 480.
    vi.stubGlobal("innerWidth", 800);
    const onWidthChange = vi.fn();
    render(
      <ResizableRightPane width={700} onWidthChange={onWidthChange}>
        <div />
      </ResizableRightPane>,
    );
    expect(onWidthChange).toHaveBeenCalledWith(480);
  });

  it("does not clamp when width is within the cap", () => {
    // Viewport: 1200 → cap = min(720, 720) = 720. width=400 < cap.
    const onWidthChange = vi.fn();
    render(
      <ResizableRightPane width={400} onWidthChange={onWidthChange}>
        <div />
      </ResizableRightPane>,
    );
    expect(onWidthChange).not.toHaveBeenCalled();
  });

  it("fires onWidthChange on viewport resize when new cap drops below width", () => {
    const onWidthChange = vi.fn();
    const { rerender } = render(
      <ResizableRightPane width={700} onWidthChange={onWidthChange}>
        <div />
      </ResizableRightPane>,
    );
    expect(onWidthChange).not.toHaveBeenCalled();
    act(() => {
      vi.stubGlobal("innerWidth", 800);
      window.dispatchEvent(new Event("resize"));
    });
    // After resize: cap = 480, current width still 700 → effect fires.
    expect(onWidthChange).toHaveBeenLastCalledWith(480);
    onWidthChange.mockClear();
    // Caller would normally update `width`; re-render with the new
    // value and confirm the effect quiesces.
    rerender(
      <ResizableRightPane width={480} onWidthChange={onWidthChange}>
        <div />
      </ResizableRightPane>,
    );
    expect(onWidthChange).not.toHaveBeenCalled();
  });
});
