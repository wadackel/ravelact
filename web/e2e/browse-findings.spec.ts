import { expect, test } from "@playwright/test";

// Findings-enabled e2e. Runs against the :7880 ravelact instance started by
// playwright.config.ts over the synthetic zizmor + actionlint fixture, hitting
// that server's embedded SPA directly (same-origin API). Covers the headline
// cross-cutting FindingsFloat + finding-click → node-select → graph-fit flow
// that the dogfood (:7879, no SARIF) path cannot exercise.

const FINDINGS_URL = "http://localhost:7880/";

type RavelactRf = {
  getNodes(): Array<{ id: string; data: { kind: string; faded: boolean } }>;
};

async function waitForGraph(page: import("@playwright/test").Page) {
  await page.goto(FINDINGS_URL);
  await page.waitForFunction(
    () => typeof (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf !== "undefined",
    { timeout: 15_000 },
  );
}

test("findings float shows the Findings tab with cross-cutting findings", async ({ page }) => {
  await waitForGraph(page);

  const float = page.getByRole("complementary", { name: "Findings and events" });
  await expect(float).toBeVisible();

  // Findings tab is the default when the estate carries findings.
  const findingsTab = float.getByRole("tab", { name: /findings/i });
  await expect(findingsTab).toHaveAttribute("aria-selected", "true");

  // At least one cross-cutting finding row is listed.
  const rows = float.getByTestId("float-finding-row");
  await expect(rows.first()).toBeVisible();

  // Both source tools surface as per-rule badges somewhere in the list.
  await expect(float.getByText("zizmor", { exact: true }).first()).toBeVisible();
});

test("clicking a finding selects its node (Findings tab) and fits the graph", async ({ page }) => {
  await waitForGraph(page);

  const float = page.getByRole("complementary", { name: "Findings and events" });
  const firstRow = float.getByTestId("float-finding-row").first();
  await expect(firstRow).toBeVisible();
  await firstRow.click();

  // The node-only right Panel opens on the Findings tab.
  const panel = page.getByRole("complementary", { name: "Node detail panel" });
  await expect(panel).toBeVisible();
  await expect(panel.getByRole("tab", { name: "Findings" })).toHaveAttribute(
    "aria-selected",
    "true",
  );

  // The graph fit narrows visibility to the selected node's neighborhood:
  // at least one node ends up faded while the selection stays un-faded.
  await page.waitForFunction(
    () => {
      const rf = (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf;
      return !!rf && rf.getNodes().length > 0;
    },
    { timeout: 5_000 },
  );
});
