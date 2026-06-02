import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { SourceBadge } from "./SourceBadge.tsx";

afterEach(cleanup);

describe("SourceBadge", () => {
  it("maps zizmor / actionlint to their own color variant", () => {
    for (const source of ["zizmor", "actionlint"] as const) {
      const { container, unmount } = render(<SourceBadge source={source} />);
      const node = container.querySelector("span");
      expect(node?.getAttribute("data-source")).toBe(source);
      expect(node?.textContent).toBe(source);
      unmount();
    }
  });

  it("falls back to the default variant for other sources but shows the verbatim label", () => {
    render(<SourceBadge source="ravelact" />);
    const node = screen.getByText("ravelact");
    expect(node.getAttribute("data-source")).toBe("default");
  });
});
