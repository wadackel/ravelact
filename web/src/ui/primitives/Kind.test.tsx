import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render } from "@testing-library/react";
import { Kind } from "./Kind.tsx";
import { NODE_KINDS, kindBadge, kindLabel } from "../../lib/kind-format.ts";

afterEach(cleanup);

describe("Kind", () => {
  it("renders all 5 kinds as a badge with the 2-letter code", () => {
    for (const kind of NODE_KINDS) {
      const { container, unmount } = render(<Kind kind={kind} variant="badge" />);
      const node = container.querySelector("span");
      expect(node?.getAttribute("data-kind")).toBe(kind);
      expect(node?.getAttribute("data-variant")).toBe("badge");
      expect(node?.textContent).toBe(kindBadge(kind));
      unmount();
    }
  });

  it("renders all 5 kinds as a pill with the human-readable label", () => {
    for (const kind of NODE_KINDS) {
      const { container, unmount } = render(<Kind kind={kind} variant="pill" />);
      const node = container.querySelector("span");
      expect(node?.getAttribute("data-kind")).toBe(kind);
      expect(node?.getAttribute("data-variant")).toBe("pill");
      expect(node?.textContent).toBe(kindLabel(kind));
      unmount();
    }
  });

  it("forwards aria-hidden when passed", () => {
    const { container } = render(<Kind kind="workflow" variant="badge" aria-hidden="true" />);
    const node = container.querySelector("span");
    expect(node?.getAttribute("aria-hidden")).toBe("true");
  });
});
