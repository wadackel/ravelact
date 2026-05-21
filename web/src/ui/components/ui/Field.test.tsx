import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { Field } from "./Field.tsx";

afterEach(cleanup);

describe("Field", () => {
  it("renders the label and the value", () => {
    render(<Field label="ID">wf:ci.yaml</Field>);
    expect(screen.getByText("ID")).toBeDefined();
    expect(screen.getByText("wf:ci.yaml")).toBeDefined();
  });

  it("omits data-mono when the prop is not set", () => {
    const { container } = render(<Field label="Label">plain</Field>);
    const value = container.querySelector("[data-field-value]");
    expect(value).not.toBeNull();
    expect(value?.getAttribute("data-mono")).toBeNull();
  });

  it('sets data-mono="true" on the value when mono is passed', () => {
    const { container } = render(
      <Field label="ID" mono>
        wf:ci.yaml
      </Field>,
    );
    const value = container.querySelector("[data-field-value]");
    expect(value?.getAttribute("data-mono")).toBe("true");
  });

  it("nests value inside a data-field-rows parent without losing data-mono", () => {
    const { container } = render(
      <div data-field-rows>
        <Field label="row1" mono>
          a
        </Field>
        <Field label="row2" mono>
          b
        </Field>
      </div>,
    );
    const monoNodes = container.querySelectorAll(
      '[data-field-rows] [data-field-value][data-mono="true"]',
    );
    expect(monoNodes.length).toBe(2);
  });
});
