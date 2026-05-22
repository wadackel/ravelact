import { describe, expect, it } from "vitest";
import { githubUrlFor } from "./github-url.ts";
import type { RepoInfo } from "./types.ts";

const REPO: RepoInfo = {
  host: "github.com",
  owner: "wadackel",
  repo: "ravelact",
  ref: "main",
};

describe("githubUrlFor — workflow", () => {
  it("builds /blob/<ref>/<path> when repo info is present", () => {
    expect(githubUrlFor({ id: "wf:.github/workflows/ci.yaml", kind: "workflow" }, REPO)).toBe(
      "https://github.com/wadackel/ravelact/blob/main/.github/workflows/ci.yaml",
    );
  });

  it("supports branch names with slashes", () => {
    expect(
      githubUrlFor(
        { id: "wf:.github/workflows/ci.yaml", kind: "workflow" },
        { ...REPO, ref: "feat/foo" },
      ),
    ).toBe("https://github.com/wadackel/ravelact/blob/feat/foo/.github/workflows/ci.yaml");
  });

  it("returns null when repo info is missing", () => {
    expect(githubUrlFor({ id: "wf:.github/workflows/ci.yaml", kind: "workflow" }, null)).toBeNull();
  });

  it("builds URL using repo.host (GitHub Enterprise)", () => {
    expect(
      githubUrlFor(
        { id: "wf:.github/workflows/ci.yaml", kind: "workflow" },
        { ...REPO, host: "ghe.example.com" },
      ),
    ).toBe("https://ghe.example.com/wadackel/ravelact/blob/main/.github/workflows/ci.yaml");
  });
});

describe("githubUrlFor — local-action", () => {
  it("builds /tree/<ref>/<dir>", () => {
    expect(githubUrlFor({ id: "la:.github/actions/setup", kind: "local-action" }, REPO)).toBe(
      "https://github.com/wadackel/ravelact/tree/main/.github/actions/setup",
    );
  });

  it("returns null when repo info is missing", () => {
    expect(githubUrlFor({ id: "la:.github/actions/setup", kind: "local-action" }, null)).toBeNull();
  });

  it("builds URL using repo.host (GitHub Enterprise)", () => {
    expect(
      githubUrlFor(
        { id: "la:.github/actions/setup", kind: "local-action" },
        { ...REPO, host: "ghe.example.com" },
      ),
    ).toBe("https://ghe.example.com/wadackel/ravelact/tree/main/.github/actions/setup");
  });
});

describe("githubUrlFor — external-action", () => {
  it("builds /tree/<gitref> when no subpath", () => {
    expect(githubUrlFor({ id: "ea:actions/checkout@v4", kind: "external-action" }, null)).toBe(
      "https://github.com/actions/checkout/tree/v4",
    );
  });

  it("builds /tree/<gitref>/<subpath> with subpath", () => {
    expect(
      githubUrlFor(
        {
          id: "ea:owner/repo/path/to/action@main",
          kind: "external-action",
        },
        null,
      ),
    ).toBe("https://github.com/owner/repo/tree/main/path/to/action");
  });

  it("does not require repo info (external is always github.com)", () => {
    // Even with a non-github host stub in `repo`, external nodes are GA
    // spec-locked to github.com and should still produce a URL.
    expect(
      githubUrlFor(
        { id: "ea:actions/checkout@v4", kind: "external-action" },
        { ...REPO, host: "gitlab.com" },
      ),
    ).toBe("https://github.com/actions/checkout/tree/v4");
  });

  it("stays on github.com even when local repo is GHE", () => {
    // Regression: when local host gate was dropped, external refs must
    // still resolve to github.com per GA spec — GHE hosts internal actions
    // separately and ravelact does not model that yet.
    expect(
      githubUrlFor(
        { id: "ea:actions/checkout@v4", kind: "external-action" },
        { ...REPO, host: "ghe.example.com" },
      ),
    ).toBe("https://github.com/actions/checkout/tree/v4");
  });

  it("returns null for malformed id", () => {
    expect(githubUrlFor({ id: "ea:bogus", kind: "external-action" }, null)).toBeNull();
    expect(githubUrlFor({ id: "ea:owner/repo@", kind: "external-action" }, null)).toBeNull();
  });
});

describe("githubUrlFor — external-workflow", () => {
  it("builds /blob/<gitref>/<path>", () => {
    expect(
      githubUrlFor(
        {
          id: "ew:owner/repo/.github/workflows/ci.yaml@main",
          kind: "external-workflow",
        },
        null,
      ),
    ).toBe("https://github.com/owner/repo/blob/main/.github/workflows/ci.yaml");
  });

  it("stays on github.com even when local repo is GHE", () => {
    expect(
      githubUrlFor(
        {
          id: "ew:owner/repo/.github/workflows/ci.yaml@main",
          kind: "external-workflow",
        },
        { ...REPO, host: "ghe.example.com" },
      ),
    ).toBe("https://github.com/owner/repo/blob/main/.github/workflows/ci.yaml");
  });

  it("returns null for malformed id (missing path segment)", () => {
    expect(githubUrlFor({ id: "ew:owner/repo@main", kind: "external-workflow" }, null)).toBeNull();
  });
});

describe("githubUrlFor — docker", () => {
  it("returns null regardless of repo info", () => {
    expect(githubUrlFor({ id: "dk:alpine:3.20", kind: "docker" }, REPO)).toBeNull();
    expect(githubUrlFor({ id: "dk:alpine:3.20", kind: "docker" }, null)).toBeNull();
  });
});
