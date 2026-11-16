import { describe, expect, it } from "vitest";
import {
  buildWalletFlowGraph,
  findObservedPaths,
  type FlowEdge,
} from "./model";
import type { WalletHistory } from "../investigations/api";

const trackedAddress = {
  id: "wallet-1",
  investigation_id: "case-1",
  network: "base",
  address: "0x1111111111111111111111111111111111111111",
  label: "Treasury",
  created_at: "2026-11-16T10:00:00Z",
} as const;

function transfer(overrides: Record<string, unknown> = {}) {
  return {
    id: "event-1",
    tracked_address_id: trackedAddress.id,
    network: "base",
    transaction_hash: "0xabc",
    direction: "outbound",
    received_amount: "0",
    spent_amount: "1250000",
    asset: "USDC",
    token_decimals: 6,
    token_contract_address: "0xToken",
    from_address: trackedAddress.address,
    to_address: "0x2222222222222222222222222222222222222222",
    block_height: 20,
    block_time: "2026-11-16T10:00:00Z",
    confirmed: true,
    provider: "Blockscout",
    source_host: "api.blockscout.com",
    observed_at: "2026-11-16T10:00:00Z",
    transfer_kind: "erc20",
    event_index: 4,
    ...overrides,
  };
}

describe("wallet flow graph", () => {
  it("builds a directional edge and keeps raw token decimals exact", () => {
    const history = {
      address: trackedAddress,
      transfers: [transfer()],
    } as unknown as WalletHistory;

    const graph = buildWalletFlowGraph([history]);

    expect(graph.nodes).toHaveLength(2);
    expect(graph.nodes.find((node) => node.tracked)?.label).toBe("Treasury");
    expect(graph.edges).toMatchObject([
      {
        from: `base:${trackedAddress.address}`,
        to: "base:0x2222222222222222222222222222222222222222",
        asset: "USDC",
        amount: "1.25",
        latestAt: "2026-11-16T10:00:00Z",
        latestTimeKind: "block",
        providers: ["Blockscout"],
        sourceHosts: ["api.blockscout.com"],
      },
    ]);
  });

  it("deduplicates the same event observed from both tracked endpoints", () => {
    const reverseHistory = {
      address: {
        ...trackedAddress,
        id: "wallet-2",
        address: "0x2222222222222222222222222222222222222222",
      },
      transfers: [
        transfer({
          id: "event-duplicate",
          tracked_address_id: "wallet-2",
          direction: "inbound",
          received_amount: "1250000",
          spent_amount: "0",
        }),
      ],
    } as unknown as WalletHistory;
    const originalHistory = {
      address: trackedAddress,
      transfers: [transfer()],
    } as unknown as WalletHistory;

    expect(
      buildWalletFlowGraph([originalHistory, reverseHistory]).edges[0]
        ?.transfers,
    ).toBe(1);
  });

  it("does not infer edges for Bitcoin observations without endpoint addresses", () => {
    const history = {
      address: { ...trackedAddress, network: "bitcoin" },
      transfers: [
        transfer({
          network: "bitcoin",
          from_address: null,
          to_address: null,
          transfer_kind: "bitcoin",
        }),
      ],
    } as unknown as WalletHistory;

    expect(buildWalletFlowGraph([history])).toMatchObject({
      nodes: [],
      edges: [],
      truncated: false,
    });
  });
});

function graphEdge(id: string, from: string, to: string): FlowEdge {
  return {
    id,
    from,
    to,
    asset: "ETH",
    transfers: 1,
    amount: "1",
    decimals: 18,
    latestAt: "2026-11-16T10:00:00Z",
    latestTimeKind: "block",
    providers: ["Blockscout"],
    sourceHosts: ["api.blockscout.com"],
  };
}

describe("observed path traversal", () => {
  const edges = [
    graphEdge("ab", "a", "b"),
    graphEdge("bc", "b", "c"),
    graphEdge("ca", "c", "a"),
    graphEdge("de", "d", "e"),
  ];

  it("follows sender-to-recipient direction only", () => {
    const fromRecipient = findObservedPaths(edges.slice(0, 1), "b", 3);

    expect(fromRecipient.depthByNode.has("a")).toBe(false);
    expect(fromRecipient.depthByNode.has("c")).toBe(false);
    expect(fromRecipient.pathEdgeIds.size).toBe(0);
  });

  it("stops revisiting nodes in cycles", () => {
    const result = findObservedPaths(edges, "a", 3);

    expect(result.depthByNode.get("a")).toBe(0);
    expect(result.depthByNode.get("b")).toBe(1);
    expect(result.depthByNode.get("c")).toBe(2);
    expect(result.paths.map((path) => path.targetId)).toEqual(["b", "c"]);
  });

  it("respects the selected maximum depth", () => {
    expect(findObservedPaths(edges, "a", 1).depthByNode.has("c")).toBe(false);
    expect(findObservedPaths(edges, "a", 2).depthByNode.get("c")).toBe(2);
  });

  it("does not include disconnected nodes", () => {
    const result = findObservedPaths(edges, "a", 3);

    expect(result.depthByNode.has("d")).toBe(false);
    expect(result.depthByNode.has("e")).toBe(false);
    expect(result.pathEdgeIds.has("de")).toBe(false);
  });
});
