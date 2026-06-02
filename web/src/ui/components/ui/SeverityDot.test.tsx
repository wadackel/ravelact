import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render } from "@testing-library/react";
import { SEVERITY_TIERS, SeverityDot } from "./SeverityDot.tsx";

afterEach(cleanup);

describe("SeverityDot", () => {
  it("renders a decorative dot carrying the severity as a data attribute", () => {
    for (const severity of SEVERITY_TIERS) {
      const { container, unmount } = render(<SeverityDot severity={severity} />);
      const node = container.querySelector("span");
      expect(node?.getAttribute("data-severity")).toBe(severity);
      expect(node?.getAttribute("aria-hidden")).toBe("true");
      unmount();
    }
  });

  it("forwards the title for a native tooltip", () => {
    const { container } = render(<SeverityDot severity="high" title="2 high" />);
    expect(container.querySelector("span")?.getAttribute("title")).toBe("2 high");
  });
});
