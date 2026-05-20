import { describe, expect, it } from "vitest";
import { formatNodeLabel, kindBadge, kindLabel } from "./kind-format.ts";

describe("kindBadge", () => {
  it("returns 2-letter codes per kind", () => {
    expect(kindBadge("workflow")).toBe("WF");
    expect(kindBadge("local-action")).toBe("LA");
    expect(kindBadge("external-action")).toBe("EA");
    expect(kindBadge("external-workflow")).toBe("EW");
    expect(kindBadge("docker")).toBe("DK");
  });
});

describe("kindLabel", () => {
  it("returns human-readable kind names", () => {
    expect(kindLabel("workflow")).toBe("Workflow");
    expect(kindLabel("local-action")).toBe("Local action");
    expect(kindLabel("external-action")).toBe("External action");
    expect(kindLabel("external-workflow")).toBe("External workflow");
    expect(kindLabel("docker")).toBe("Docker");
  });
});

describe("formatNodeLabel", () => {
  it("workflow → bare label, subtitle 'workflow'", () => {
    expect(formatNodeLabel("workflow", "CI")).toEqual({
      name: "CI",
      subtitle: "workflow",
    });
  });

  it("local-action → bare label, subtitle 'local action'", () => {
    expect(formatNodeLabel("local-action", "Setup ravelact")).toEqual({
      name: "Setup ravelact",
      subtitle: "local action",
    });
  });

  it("external-action with 40-hex SHA → shortened to 7 chars", () => {
    const sha = "de0fac2e4500dabe0009e67214ff5f5447ce83dd";
    expect(formatNodeLabel("external-action", `actions/checkout@${sha}`)).toEqual({
      name: "actions/checkout",
      subtitle: "@de0fac2",
    });
  });

  it("external-action with non-SHA ref → kept verbatim", () => {
    expect(formatNodeLabel("external-action", "actions/checkout@v4")).toEqual({
      name: "actions/checkout",
      subtitle: "@v4",
    });
  });

  it("external-action without '@' → falls back to kind label", () => {
    expect(formatNodeLabel("external-action", "foo/bar")).toEqual({
      name: "foo/bar",
      subtitle: "external action",
    });
  });

  it("external-workflow with SHA → same shortening rule", () => {
    const sha = "de0fac2e4500dabe0009e67214ff5f5447ce83dd";
    expect(
      formatNodeLabel("external-workflow", `owner/repo/.github/workflows/deploy.yml@${sha}`),
    ).toEqual({
      name: "owner/repo/.github/workflows/deploy.yml",
      subtitle: "@de0fac2",
    });
  });

  it("docker with ':tag' → subtitle ':tag'", () => {
    expect(formatNodeLabel("docker", "node:18")).toEqual({
      name: "node",
      subtitle: ":18",
    });
  });

  it("docker without ':' → subtitle 'docker'", () => {
    expect(formatNodeLabel("docker", "node")).toEqual({
      name: "node",
      subtitle: "docker",
    });
  });

  it("docker with empty tag after ':' → subtitle 'docker' (fallback)", () => {
    // `name:` (trailing colon with no tag) lands on the last branch.
    expect(formatNodeLabel("docker", "node:")).toEqual({
      name: "node:",
      subtitle: "docker",
    });
  });
});
