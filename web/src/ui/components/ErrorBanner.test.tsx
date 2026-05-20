import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { ErrorBanner } from "./ErrorBanner.tsx";

afterEach(cleanup);

describe("ErrorBanner", () => {
  it("renders nothing when message is null", () => {
    const { container } = render(<ErrorBanner message={null} />);
    expect(container.firstChild).toBeNull();
  });

  it("renders an alert with the message when message is non-null", () => {
    render(<ErrorBanner message="boom" />);
    const alert = screen.getByRole("alert");
    expect(alert.textContent).toBe("boom");
  });
});
