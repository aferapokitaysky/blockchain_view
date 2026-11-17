import { describe, expect, it } from "vitest";
import type { GraphSnapshot } from "../investigations/api";
import { getSnapshotViewState } from "./snapshotView";

function snapshot(overrides: Partial<GraphSnapshot> = {}): GraphSnapshot {
  return {
    id: "snapshot-1",
    investigation_id: "case-1",
    title: "Snapshot",
    focus_network: "base",
    focus_address: "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    max_depth: 3,
    source_transfer_ids: ["transfer-1"],
    source_relationship_ids: [],
    graph: {
      nodes: [
        {
          id: "base:0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
          network: "base",
          address: "0xAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
          label: "Treasury",
          tracked: true,
          x: 0,
          y: 0,
        },
      ],
      edges: [],
      sourceTransferIds: ["transfer-1"],
      sourceRelationshipIds: [],
      truncated: false,
    },
    created_at: "2026-11-17T12:00:00Z",
    ...overrides,
  };
}

describe("saved graph snapshot view state", () => {
  it("restores the snapshot focus case-insensitively on EVM networks", () => {
    expect(getSnapshotViewState(snapshot())).toEqual({
      selectedNodeId: "base:0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      maxDepth: 3,
    });
  });

  it("clears focus when a snapshot has no matching focus node", () => {
    expect(
      getSnapshotViewState(
        snapshot({
          focus_address: "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        }),
      ),
    ).toEqual({ selectedNodeId: "", maxDepth: 3 });
  });

  it("returns current graph defaults when leaving a snapshot", () => {
    expect(getSnapshotViewState(null)).toEqual({
      selectedNodeId: "",
      maxDepth: 2,
    });
  });
});
