import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { PoweredBy } from "./PoweredBy.tsx";

afterEach(cleanup);

describe("PoweredBy", () => {
  it("links to the ravelact repository and opens in a new tab", () => {
    render(<PoweredBy />);
    const link = screen.getByRole("link", { name: /Powered by ravelact/i });
    expect(link).toHaveAttribute("href", "https://github.com/wadackel/ravelact");
    expect(link).toHaveAttribute("target", "_blank");
    const rel = link.getAttribute("rel") ?? "";
    expect(rel).toMatch(/noopener/);
    expect(rel).toMatch(/noreferrer/);
  });

  it("renders the build-time version both visibly and in the accessible name", () => {
    render(<PoweredBy />);
    const link = screen.getByRole("link", { name: /Powered by ravelact/i });
    // The visible label collapses inner-span whitespace; assert against the
    // normalised text so the split <span> (version chip) does not trip the test.
    // The visible label collapses inner-span whitespace; the `↗` glyph sits
    // in its own flex item so there is no literal whitespace between version
    // and arrow.
    expect(link.textContent?.replace(/\s+/g, " ").trim()).toMatch(
      /^Powered by ravelact v0\.0\.0-test\s*↗$/,
    );
    expect(link).toHaveAccessibleName(
      "Powered by ravelact v0.0.0-test — opens GitHub repository in a new tab",
    );
  });
});
