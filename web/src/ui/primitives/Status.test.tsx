import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { Status } from "./Status.tsx";

afterEach(cleanup);

describe("Status", () => {
  it('type="loading" renders role=status with the default text', () => {
    render(<Status type="loading" />);
    const node = screen.getByRole("status");
    expect(node.textContent).toBe("Loading…");
  });

  it('type="loading" honors children override', () => {
    render(<Status type="loading">Fetching…</Status>);
    expect(screen.getByRole("status").textContent).toBe("Fetching…");
  });

  it('type="error" renders role=alert with the children as message', () => {
    render(<Status type="error">boom</Status>);
    const node = screen.getByRole("alert");
    expect(node.textContent).toBe("boom");
  });

  it('type="empty" renders a div with no role and children as text', () => {
    const { container } = render(<Status type="empty">no rows</Status>);
    const node = container.firstChild as HTMLElement | null;
    expect(node).not.toBeNull();
    expect(node?.getAttribute("role")).toBeNull();
    expect(node?.textContent).toBe("no rows");
  });
});
