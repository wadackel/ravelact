import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { Chip } from "./Chip.tsx";

afterEach(cleanup);

describe("Chip", () => {
  it("renders its children inside a span", () => {
    render(<Chip>push</Chip>);
    const node = screen.getByText("push");
    expect(node.tagName).toBe("SPAN");
  });
});
