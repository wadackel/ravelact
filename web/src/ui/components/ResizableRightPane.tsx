import { type ReactNode, useCallback, useEffect, useState } from "react";
import { Resizable, type ResizeCallback } from "re-resizable";
import { clampPanelWidth, MIN_PANEL_WIDTH, panelMaxWidth } from "../../lib/panel-width.ts";

export type ResizableRightPaneProps = {
  width: number;
  onWidthChange: (next: number) => void;
  children: ReactNode;
};

// Wrapper around `re-resizable`'s <Resizable> that exposes only a
// left-edge drag handle and enforces a viewport-relative max width.
// `re-resizable`'s `maxWidth` accepts only `string | number`, so we
// track `window.innerWidth` in state and compute the numeric cap on
// every render. When a smaller viewport drops the cap below the
// current width (persisted layout from a wider session), the
// clamp-down effect feeds the new cap back to the App-owned setter.
export function ResizableRightPane({ width, onWidthChange, children }: ResizableRightPaneProps) {
  const [viewportWidth, setViewportWidth] = useState<number>(() =>
    typeof window === "undefined" ? 1024 : window.innerWidth,
  );

  useEffect(() => {
    const onResize = () => setViewportWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  const cap = panelMaxWidth(viewportWidth);

  useEffect(() => {
    if (width > cap) onWidthChange(cap);
  }, [cap, width, onWidthChange]);

  const handleResizeStop = useCallback<ResizeCallback>(
    (_e, _dir, _ref, delta) => {
      onWidthChange(clampPanelWidth(width + delta.width, viewportWidth));
    },
    [onWidthChange, viewportWidth, width],
  );

  return (
    <Resizable
      size={{ width, height: "100%" }}
      minWidth={MIN_PANEL_WIDTH}
      maxWidth={cap}
      enable={{
        top: false,
        right: false,
        bottom: false,
        left: true,
        topRight: false,
        bottomRight: false,
        bottomLeft: false,
        topLeft: false,
      }}
      handleStyles={{ left: { cursor: "ew-resize", width: 6, left: -3 } }}
      handleClasses={{ left: "ravelact-resize-handle-left" }}
      className="ravelact-right-pane h-full"
      onResizeStop={handleResizeStop}
    >
      {children}
    </Resizable>
  );
}
