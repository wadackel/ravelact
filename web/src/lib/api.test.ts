import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Code, ConnectError } from "@connectrpc/connect";

// `api.ts` builds its `BrowseService` client at module import. Mock the
// Connect factory before importing so each test can swap in a fresh
// stub. The mock object captures every call so assertions can read
// argument shape + count back.
const stub: Record<string, ReturnType<typeof vi.fn>> = {
  getGraph: vi.fn(),
  getRepo: vi.fn(),
  listTriggers: vi.fn(),
  search: vi.fn(),
  getEventImpact: vi.fn(),
  getNode: vi.fn(),
  getImpact: vi.fn(),
  trace: vi.fn(),
};

vi.mock("@connectrpc/connect", async (importOriginal) => {
  const orig = await importOriginal<typeof import("@connectrpc/connect")>();
  return {
    ...orig,
    createClient: () => stub,
  };
});

vi.mock("@connectrpc/connect-web", () => ({
  createConnectTransport: () => ({ transport: "mock" }),
}));

const {
  fetchEventImpact,
  fetchGraph,
  fetchImpact,
  fetchNode,
  fetchSearch,
  fetchTrace,
  fetchTriggers,
  fetchRepo,
} = await import("./api.ts");

function notFound(): ConnectError {
  return new ConnectError("missing", Code.NotFound);
}

function canceled(): ConnectError {
  return new ConnectError("canceled", Code.Canceled);
}

describe("lib/api — Connect client wrappers", () => {
  beforeEach(() => {
    for (const key of Object.keys(stub)) {
      stub[key]!.mockReset();
    }
  });
  afterEach(() => {
    for (const key of Object.keys(stub)) {
      stub[key]!.mockReset();
    }
  });

  it("fetchNode passes kind + id and narrows the response kind", async () => {
    stub.getNode!.mockResolvedValueOnce({
      id: "wf:x",
      kind: "workflow",
      label: "x",
      file: "",
      summary: "",
      entryTriggers: [],
      refsIn: [],
      refsOut: [],
      ifConditions: [],
    });
    const resp = await fetchNode("workflow", ".github/workflows/x.yml");
    expect(stub.getNode).toHaveBeenCalledWith({
      kind: "workflow",
      id: ".github/workflows/x.yml",
    });
    expect(resp?.kind).toBe("workflow");
  });

  it("fetchNode throws when server returns a non-NodeResponseKind", async () => {
    stub.getNode!.mockResolvedValueOnce({
      id: "x",
      kind: "external-workflow",
      label: "",
      file: "",
      summary: "",
      entryTriggers: [],
      refsIn: [],
      refsOut: [],
      ifConditions: [],
    });
    await expect(fetchNode("workflow", "x")).rejects.toThrow(
      "unexpected GetNode kind: external-workflow",
    );
  });

  it("fetchImpact passes id through verbatim", async () => {
    stub.getImpact!.mockResolvedValueOnce({ workflows: [], actions: [], unknowns: [] });
    await fetchImpact(".github/workflows/x.yml");
    expect(stub.getImpact).toHaveBeenCalledWith({ id: ".github/workflows/x.yml" });
  });

  it("fetchTrace passes id through verbatim", async () => {
    stub.trace!.mockResolvedValueOnce({ tree: undefined, eventUsed: "push" });
    await fetchTrace(".github/workflows/wf.yml");
    expect(stub.trace).toHaveBeenCalledWith({ id: ".github/workflows/wf.yml" });
  });

  it("returns null when the Connect server signals NotFound (5 nullable helpers)", async () => {
    stub.getNode!.mockRejectedValueOnce(notFound());
    expect(await fetchNode("workflow", "missing")).toBeNull();
    stub.getImpact!.mockRejectedValueOnce(notFound());
    expect(await fetchImpact("missing")).toBeNull();
    stub.trace!.mockRejectedValueOnce(notFound());
    expect(await fetchTrace("missing")).toBeNull();
    stub.listTriggers!.mockRejectedValueOnce(notFound());
    expect(await fetchTriggers()).toBeNull();
    stub.getRepo!.mockRejectedValueOnce(notFound());
    expect(await fetchRepo()).toBeNull();
  });

  it("re-throws non-NotFound errors from the nullable helpers", async () => {
    stub.getNode!.mockRejectedValueOnce(new ConnectError("boom", Code.Internal));
    await expect(fetchNode("workflow", "x")).rejects.toThrow("boom");
  });

  it("fetchGraph throws on any failure (never returns null)", async () => {
    stub.getGraph!.mockRejectedValueOnce(new ConnectError("server-down", Code.Internal));
    await expect(fetchGraph()).rejects.toThrow("server-down");
  });

  it("fetchSearch forwards AbortSignal and Canceled becomes AbortError", async () => {
    stub.search!.mockResolvedValueOnce({ matches: [], truncated: false, total: 0 });
    const controller = new AbortController();
    await fetchSearch("a b", controller.signal);
    expect(stub.search).toHaveBeenCalledWith({ q: "a b" }, { signal: controller.signal });

    stub.search!.mockRejectedValueOnce(canceled());
    await expect(fetchSearch("a b", controller.signal)).rejects.toMatchObject({
      name: "AbortError",
    });
  });

  it("fetchSearch throws on non-Canceled, non-NotFound failures", async () => {
    stub.search!.mockRejectedValueOnce(new ConnectError("boom", Code.Internal));
    await expect(fetchSearch("x")).rejects.toThrow("boom");
  });

  it("fetchEventImpact forwards AbortSignal", async () => {
    stub.getEventImpact!.mockResolvedValueOnce({
      event: "push",
      entryWorkflows: [],
      nodeIds: [],
    });
    const controller = new AbortController();
    await fetchEventImpact("push", controller.signal);
    expect(stub.getEventImpact).toHaveBeenCalledWith(
      { event: "push" },
      { signal: controller.signal },
    );
  });

  it("fetchEventImpact throws on transport failure", async () => {
    stub.getEventImpact!.mockRejectedValueOnce(new ConnectError("nope", Code.Internal));
    await expect(fetchEventImpact("push")).rejects.toThrow("nope");
  });

  it("fetchTriggers returns the parsed list on success", async () => {
    stub.listTriggers!.mockResolvedValueOnce({
      rows: [
        {
          event: "push",
          entryWorkflows: 3,
          declarations: 3,
          typed: 0,
          filtered: 0,
          examples: [],
        },
      ],
    });
    const r = await fetchTriggers();
    expect(r?.rows[0]?.event).toBe("push");
    expect(r?.rows[0]?.entryWorkflows).toBe(3);
  });
});
