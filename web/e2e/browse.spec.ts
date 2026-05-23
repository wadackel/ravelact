import { expect, test } from "@playwright/test";

type RavelactRf = {
  getNodes(): Array<{ id: string; data: { kind: string; faded: boolean } }>;
  getEdges(): Array<{ id: string; source: string; target: string; className?: string }>;
  tapNode(id: string): string | null;
  tapFirstWorkflow(): string | null;
  tapFirstWorkflowExcept(excludeId: string): string | null;
  backgroundTap(): void;
  fadedIds(): string[];
  panBy(dx: number, dy: number): void;
};

declare global {
  // eslint-disable-next-line no-var
  var __ravelactRf: RavelactRf | undefined;
}

async function waitForGraph(page: import("@playwright/test").Page) {
  await page.goto("/");
  await page.waitForFunction(
    () => typeof (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf !== "undefined",
    { timeout: 15_000 },
  );
}

async function tapFirstWorkflow(page: import("@playwright/test").Page): Promise<string | null> {
  return await page.evaluate<string | null>(() => {
    const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf;
    return rf ? rf.tapFirstWorkflow() : null;
  });
}

test("graph loads and panel opens on node click", async ({ page }) => {
  await waitForGraph(page);
  const id = await tapFirstWorkflow(page);
  expect(id).toBeTruthy();

  const panel = page.getByRole("complementary", { name: "Node detail panel" });
  await expect(panel).toBeVisible();
});

test("panel exposes Open in GitHub link for a workflow node", async ({ page }) => {
  // Playwright config runs `ravelact --root ..` which points at the
  // wadackel/ravelact checkout, so /api/repo returns
  // `{ host: "github.com", owner: "wadackel", repo: "ravelact", ref: <branch|sha> }`.
  // The link is therefore expected to exist; if /api/repo unexpectedly
  // 404s the test will fail loudly, which is the desired signal.
  await waitForGraph(page);
  const id = await tapFirstWorkflow(page);
  expect(id).toBeTruthy();

  const link = page
    .getByRole("complementary", { name: "Node detail panel" })
    .getByRole("link", { name: "Open in GitHub" });
  await expect(link).toBeVisible();
  // ref is either a branch name (with possible slashes) or a 40-char SHA —
  // both legal in GitHub URLs, both accepted by `.+`.
  await expect(link).toHaveAttribute(
    "href",
    /^https:\/\/github\.com\/wadackel\/ravelact\/blob\/.+\/\.github\/workflows\/.+\.yaml$/,
  );
  await expect(link).toHaveAttribute("target", "_blank");
  await expect(link).toHaveAttribute("rel", "noopener noreferrer");
});

test("4 tabs are accessible and switch on click", async ({ page }) => {
  await waitForGraph(page);
  await tapFirstWorkflow(page);

  for (const tab of ["Details", "Triggers", "Impact", "Trace"] as const) {
    const button = page.getByRole("tab", { name: tab });
    await button.click();
    await expect(button).toHaveAttribute("aria-selected", "true");
  }
});

test("keyboard navigation: Arrow / Home / End / Escape", async ({ page }) => {
  await waitForGraph(page);
  await tapFirstWorkflow(page);

  await page.getByRole("tab", { name: "Details" }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("tab", { name: "Triggers" })).toHaveAttribute(
    "aria-selected",
    "true",
  );

  await page.keyboard.press("End");
  await expect(page.getByRole("tab", { name: "Trace" })).toHaveAttribute("aria-selected", "true");

  await page.keyboard.press("Home");
  await expect(page.getByRole("tab", { name: "Details" })).toHaveAttribute("aria-selected", "true");

  await page.keyboard.press("Escape");
  await expect(page.getByRole("complementary", { name: "Node detail panel" })).toBeHidden();
});

test("stats strip eventually shows event count", async ({ page }) => {
  await page.goto("/");
  const stats = page.locator("#stats");
  await expect(stats).not.toHaveText("", { timeout: 10_000 });
  await expect(stats).toContainText("events");
});

test("stats strip includes nodes count (redesign)", async ({ page }) => {
  await waitForGraph(page);
  const stats = page.locator("#stats");
  await expect(stats).toContainText("nodes", { timeout: 10_000 });
  await expect(stats).toContainText("entry workflows");
});

test("header search input is enabled and accepts text", async ({ page }) => {
  await page.goto("/");
  const search = page.getByLabel("Search nodes, files, and triggers");
  await expect(search).toBeVisible();
  await expect(search).toBeEnabled();
  await search.fill("hello");
  await expect(search).toHaveValue("hello");

  const fs = page.getByRole("button", { name: "Toggle fullscreen" });
  await expect(fs).toBeVisible();
});

test("typing in search fades non-matching nodes", async ({ page }) => {
  await waitForGraph(page);
  const search = page.getByLabel("Search nodes, files, and triggers");
  await search.fill("ci");

  // After debounce + fetch, at least one match has landed and at least
  // one non-matching node is faded.
  await page.waitForFunction(
    () => {
      const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf;
      if (!rf) return false;
      const nodes = rf.getNodes();
      const fadedCount = nodes.filter((n) => n.data.faded).length;
      const unfadedCount = nodes.filter((n) => !n.data.faded).length;
      return fadedCount > 0 && unfadedCount > 0;
    },
    { timeout: 5_000 },
  );
});

test("⌘K focuses the search input; Escape clears and blurs", async ({ page, browserName }) => {
  await page.goto("/");
  const search = page.getByLabel("Search nodes, files, and triggers");
  await search.fill("noise");
  // Move focus away first to make the focus assertion meaningful.
  await page.getByRole("button", { name: "Toggle fullscreen" }).focus();
  await expect(search).not.toBeFocused();

  // Webkit on macOS routes Meta+K to OS keyboard; use Control+K on
  // webkit which the handler also accepts via `e.metaKey || e.ctrlKey`.
  const mod = browserName === "webkit" ? "Control" : "ControlOrMeta";
  await page.keyboard.press(`${mod}+KeyK`);
  await expect(search).toBeFocused();

  await page.keyboard.press("Escape");
  await expect(search).toHaveValue("");
  await expect(search).not.toBeFocused();
});

test("highlight: tap fades non-reachable; second tap recomputes set", async ({ page }) => {
  await waitForGraph(page);

  const firstId = await tapFirstWorkflow(page);
  expect(firstId).toBeTruthy();

  await page.waitForFunction(
    () => {
      const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf;
      return !!rf && rf.fadedIds().length > 0;
    },
    { timeout: 5_000 },
  );

  // The selected node itself must stay unfaded across the highlight pass.
  const firstUnfaded = await page.evaluate<boolean, string>((id) => {
    const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf!;
    const node = rf.getNodes().find((n) => n.id === id);
    return !!node && !node.data.faded;
  }, firstId!);
  expect(firstUnfaded).toBe(true);

  const before = await page.evaluate<string[]>(() => {
    const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf!;
    return rf.fadedIds();
  });

  const secondId = await page.evaluate<string | null, string>((excludeId) => {
    const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf!;
    return rf.tapFirstWorkflowExcept(excludeId);
  }, firstId!);
  expect(secondId).toBeTruthy();
  expect(secondId).not.toBe(firstId);

  await page.waitForFunction(
    (id) => {
      const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf;
      if (!rf) return false;
      const n = rf.getNodes().find((x) => x.id === id);
      return !!n && !n.data.faded;
    },
    secondId!,
    { timeout: 5_000 },
  );

  const after = await page.evaluate<string[]>(() => {
    const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf!;
    return rf.fadedIds();
  });
  expect(after).not.toEqual(before);
});

test("overview pane lists events and clicking one fades unrelated nodes", async ({ page }) => {
  await waitForGraph(page);

  // OverviewPane is mounted by default (no node selected).
  const overview = page.getByRole("complementary", { name: "Graph overview" });
  await expect(overview).toBeVisible();

  // The dogfood estate has at least `push` as an event.
  // pull_request triggers only `ci.yaml` in the dogfood estate so
  // `release.yaml` and `release-plz.yaml` reliably fade. Using `push`
  // would reach every node and break the fade assertion.
  const pushOption = overview.getByRole("button", { name: /^pull_request/ });
  await expect(pushOption).toBeVisible();
  await pushOption.click();

  await page.waitForFunction(
    () => {
      const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf;
      if (!rf) return false;
      const nodes = rf.getNodes();
      return (
        nodes.filter((n) => n.data.faded).length > 0 &&
        nodes.filter((n) => !n.data.faded).length > 0
      );
    },
    { timeout: 5_000 },
  );

  await expect(pushOption).toHaveAttribute("aria-pressed", "true");
});

test("clicking the same event again clears the analysis fade", async ({ page }) => {
  await waitForGraph(page);
  const overview = page.getByRole("complementary", { name: "Graph overview" });
  // pull_request triggers only `ci.yaml` in the dogfood estate so
  // `release.yaml` and `release-plz.yaml` reliably fade. Using `push`
  // would reach every node and break the fade assertion.
  const pushOption = overview.getByRole("button", { name: /^pull_request/ });

  // First click → fade applies.
  await pushOption.click();
  await page.waitForFunction(
    () => {
      const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf;
      return !!rf && rf.getNodes().some((n) => n.data.faded);
    },
    { timeout: 5_000 },
  );

  // Second click on the same row → toggle off, fade clears.
  await pushOption.click();
  await page.waitForFunction(
    () => {
      const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf;
      return !!rf && rf.getNodes().every((n) => !n.data.faded);
    },
    { timeout: 5_000 },
  );
  await expect(pushOption).toHaveAttribute("aria-pressed", "false");
});

test("search with zero matches fades every node; clearing restores", async ({ page }) => {
  await waitForGraph(page);
  const search = page.getByLabel("Search nodes, files, and triggers");
  await search.fill("zzqx-no-such-token-anywhere");
  await page.waitForFunction(
    () => {
      const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf;
      if (!rf) return false;
      const nodes = rf.getNodes();
      return nodes.length > 0 && nodes.every((n) => n.data.faded);
    },
    { timeout: 5_000 },
  );

  await search.fill("");
  await page.waitForFunction(
    () => {
      const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf;
      return !!rf && rf.getNodes().every((n) => !n.data.faded);
    },
    { timeout: 5_000 },
  );
});

test("search + event compose with OR: union is visible", async ({ page }) => {
  await waitForGraph(page);
  // The CI workflow matches "ci" (label CI + file ci.yaml).
  const search = page.getByLabel("Search nodes, files, and triggers");
  await search.fill("ci");
  await page.waitForFunction(
    () => {
      const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf;
      if (!rf) return false;
      const nodes = rf.getNodes();
      return nodes.some((n) => n.data.faded) && nodes.some((n) => !n.data.faded);
    },
    { timeout: 5_000 },
  );

  // Click pull_request event in the overview. Note that the panel
  // does NOT open just by overview interaction, so OverviewPane is
  // still mounted alongside the active search.
  const overview = page.getByRole("complementary", { name: "Graph overview" });
  await overview.getByRole("button", { name: /^pull_request/ }).click();

  // After OR composition: union of (search-matched) ∪ (event-reachable)
  // must still include the CI workflow id (it satisfies both filters).
  await page.waitForFunction(
    () => {
      const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf;
      if (!rf) return false;
      const ci = rf.getNodes().find((n) => n.id === "wf:.github/workflows/ci.yaml");
      return !!ci && !ci.data.faded;
    },
    { timeout: 5_000 },
  );
});

test("failed GetGraph fetch surfaces ErrorBanner", async ({ page }) => {
  const endpoint = "**/ravelact.browse.v1.BrowseService/GetGraph";
  await page.route(endpoint, (route) => route.fulfill({ status: 500, body: "" }));
  await page.goto("/");
  const alert = page.getByRole("alert");
  await expect(alert).toBeVisible();
  // Connect-Web normalizes an HTTP 500 with no Connect body to a
  // ConnectError whose stringified message contains the HTTP status.
  await expect(alert).toContainText("500");
  await page.unroute(endpoint);
});

test("background tap clears highlight and closes panel", async ({ page }) => {
  await waitForGraph(page);
  const firstId = await tapFirstWorkflow(page);
  expect(firstId).toBeTruthy();

  await page.waitForFunction(
    () => {
      const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf;
      return !!rf && rf.fadedIds().length > 0;
    },
    { timeout: 5_000 },
  );

  await page.evaluate(() => {
    (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf!.backgroundTap();
  });

  await page.waitForFunction(
    () => {
      const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf;
      return !!rf && rf.fadedIds().length === 0;
    },
    { timeout: 5_000 },
  );

  await expect(page.getByRole("complementary", { name: "Node detail panel" })).toBeHidden();
});

test("viewport culling: store stays complete while DOM only renders visible nodes", async ({
  page,
}) => {
  await waitForGraph(page);

  // Wait for the initial fitView to settle. With `onlyRenderVisibleElements`
  // ON, the DOM should at this point still hold every node because fitView
  // scaled everything into the viewport.
  const beforeCount = await page.evaluate(
    () => (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf!.getNodes().length,
  );
  expect(beforeCount).toBeGreaterThan(0);

  // Pan far enough that some nodes leave the viewport. The dogfood graph
  // is laid out LR; a horizontal pan of 4000px guarantees at least one
  // node falls off-screen at default zoom.
  await page.evaluate(() => {
    (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf!.panBy(4000, 0);
  });

  // ReactFlow re-runs the visible-node selector on the next render after
  // viewport mutation. Give it a moment to settle.
  await page.waitForTimeout(150);

  // Store count is invariant — `useReactFlow().getNodes()` reads the
  // backing zustand store regardless of viewport culling.
  const afterCount = await page.evaluate(
    () => (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf!.getNodes().length,
  );
  expect(afterCount).toBe(beforeCount);

  // DOM count should drop because culling now omits off-screen nodes.
  // We assert strictly less; if dogfood's layout coincidentally keeps
  // every node in view after a 4000px pan the assertion will catch the
  // regression.
  const domCount = await page.locator(".react-flow__node").count();
  expect(domCount).toBeLessThan(beforeCount);
});

test("Powered by ravelact credit pill links to the project repo", async ({ page }) => {
  await waitForGraph(page);

  const link = page.getByRole("link", { name: /Powered by ravelact/i });
  await expect(link).toBeVisible();

  expect(await link.getAttribute("href")).toBe("https://github.com/wadackel/ravelact");
  expect(await link.getAttribute("target")).toBe("_blank");
  const rel = (await link.getAttribute("rel")) ?? "";
  expect(rel).toMatch(/noopener/);
  expect(rel).toMatch(/noreferrer/);
});
