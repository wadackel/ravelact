// Type façade over the generated ConnectRPC stubs in
// `web/src/proto/ravelact/browse/v1/browse_pb.ts`.
//
// Two roles:
//   1. Re-export every wire message type the SPA consumes, so call
//      sites can keep importing from `./lib/types.ts` without learning
//      the generated path.
//   2. Carry the narrower TS string-literal unions (`NodeKind`,
//      `EdgeKind`, `NodeResponseKind`) that protobuf enums cannot
//      express — these unions back the SPA's exhaustive switches over
//      node/edge categories.

export type {
  CyEdge,
  CyEdgeData,
  CyNode,
  CyNodeData,
  Finding,
  FindingCounts,
  FindingWithNode,
  GetEventImpactResponse,
  GetFindingsResponse,
  GetGraphResponse,
  GetImpactResponse,
  GetNodeResponse,
  GetRepoResponse,
  IfCondition,
  ImpactAction,
  JobIfCondition,
  ListFindingsResponse,
  ListTriggersResponse,
  SearchMatch,
  SearchResponse,
  StepIfCondition,
  TraceActionNode,
  TraceAnnotatedNode,
  TraceCycleNode,
  TraceDockerNode,
  TraceExternalActionNode,
  TraceExternalWorkflowNode,
  TraceGuardedNode,
  TraceJsonNode,
  TraceResponse,
  TraceWorkflowNode,
  TriggerSummary,
} from "../proto/ravelact/browse/v1/browse_pb.ts";

// Aliases preserving the names the SPA imported under the
// hand-written-JSON era. They keep call sites
// (`fetchGraph(): Promise<GraphPayload>`, `RepoInfo`, etc.) compiling
// against the renamed proto messages without churning every consumer.
import type {
  GetFindingsResponse as _GetFindingsResponse,
  GetGraphResponse as _GetGraphResponse,
  GetImpactResponse as _GetImpactResponse,
  GetNodeResponse as _GetNodeResponse,
  GetRepoResponse as _GetRepoResponse,
  ListTriggersResponse as _ListTriggersResponse,
} from "../proto/ravelact/browse/v1/browse_pb.ts";
export type GraphPayload = _GetGraphResponse;
export type ImpactResponse = _GetImpactResponse;
export type NodeResponse = _GetNodeResponse;
export type RepoInfo = _GetRepoResponse;
export type TriggersResponse = _ListTriggersResponse;
export type FindingsResponse = _GetFindingsResponse;

// The Rust producer emits one of five `kind` literals on `CyNodeData`;
// the proto's `string kind` field stays open-shape, so we narrow here.
// `add_node` call sites in `src/cli/render/browse/mod.rs` (the only
// producer) write exactly these five values.
export type NodeKind =
  | "workflow"
  | "local-action"
  | "external-action"
  | "external-workflow"
  | "docker";

// Subset of `NodeKind` that the `GetNode` RPC can return. The Rust
// handler returns `Err(NotFound)` for `external-workflow` and
// `docker`, so the response can only carry one of these three. The
// SPA narrows to this type via the runtime check in `api.ts`'s
// `fetchNode`.
export type NodeResponseKind = "workflow" | "local-action" | "external-action";

// Edge categories produced by `GraphBuilder::add_edge` in
// `src/cli/render/browse/mod.rs`.
export type EdgeKind =
  | "annotation"
  | "calls-workflow"
  | "uses-local-workflow"
  | "uses-local-action"
  | "uses-external-action"
  | "uses-docker";
