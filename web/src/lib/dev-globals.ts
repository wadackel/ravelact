// Test/perf surface that Graph.tsx installs on `globalThis.__ravelactRf`
// during mount and that App.tsx, the e2e harness, and the perf harness
// read. Keep the producer (Graph) and the consumers in sync by importing
// this type instead of re-declaring it.
export type RavelactRf = {
  getNodes: () => { id: string; data: unknown }[];
  getEdges: () => { id: string; className?: string }[];
  tapNode: (id: string) => string | null;
  tapFirstWorkflow: () => string | null;
  tapFirstWorkflowExcept: (excludeId: string) => string | null;
  backgroundTap: () => void;
  panBy: (dx: number, dy: number) => void;
  fitNodes: (ids: string[]) => void;
  fadedIds: () => string[];
};

const RF_KEY = "__ravelactRf";
const PERF_KEY = "__perf";

type GlobalWithDevHooks = Record<string, unknown>;

export function setRavelactRf(handle: RavelactRf): () => void {
  (globalThis as GlobalWithDevHooks)[RF_KEY] = handle;
  return () => {
    delete (globalThis as GlobalWithDevHooks)[RF_KEY];
  };
}

export function getRavelactRf(): RavelactRf | undefined {
  return (globalThis as GlobalWithDevHooks)[RF_KEY] as RavelactRf | undefined;
}

export function isPerfHarnessEnabled(): boolean {
  return typeof (globalThis as GlobalWithDevHooks)[PERF_KEY] !== "undefined";
}
