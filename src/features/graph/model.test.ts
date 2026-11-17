import { describe, expect, it } from "vitest";
import {
  buildWalletFlowGraph,
  findObservedPaths,
  getCounterpartyPrefill,
  hasGraphEvidence,
  type FlowNode,
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
  it("creates Aptos graph edges from attributed fungible asset activities", () => {
    const address = `0x${"1".repeat(64)}`;
    const history = {
      address: { ...trackedAddress, network: "aptos", address },
      transfers: [
        transfer({
          network: "aptos",
          transaction_hash: "123456",
          transfer_kind: "aptos_fungible",
          direction: "outbound",
          received_amount: "0",
          spent_amount: "123456789",
          asset: "0x1::aptos_coin::AptosCoin",
          token_decimals: null,
          token_contract_address: "0x1::aptos_coin::AptosCoin",
          from_address: address,
          to_address: `0x${"2".repeat(64)}`,
        }),
      ],
    } as unknown as WalletHistory;

    const graph = buildWalletFlowGraph([history]);
    expect(graph.nodes).toHaveLength(2);
    expect(graph.edges).toMatchObject([
      {
        from: `aptos:${address}`,
        to: `aptos:0x${"2".repeat(64)}`,
        asset: "0x1::aptos_coin::AptosCoin",
        amount: "123456789",
      },
    ]);
  });

  it("allows snapshots when either evidence source exists", () => {
    expect(
      hasGraphEvidence({
        sourceTransferIds: ["transfer"],
        sourceRelationshipIds: [],
      }),
    ).toBe(true);
    expect(
      hasGraphEvidence({
        sourceTransferIds: [],
        sourceRelationshipIds: ["relationship"],
      }),
    ).toBe(true);
    expect(
      hasGraphEvidence({ sourceTransferIds: [], sourceRelationshipIds: [] }),
    ).toBe(false);
  });

  it("shows Cardano same-transaction relations without adding path edges", () => {
    const history = {
      address: {
        ...trackedAddress,
        network: "cardano",
        address:
          "addr1qx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3n0d3vllmyqwsx5wktcd8cc3sq835lu7drv2xwl2wywfgse35a3x",
      },
      transfers: [],
      relationships: [
        {
          id: "relationship-1",
          tracked_address_id: "wallet-1",
          network: "cardano",
          transaction_hash: "a".repeat(64),
          counterparty_address:
            "Ae2tdPwUPEZLs4HtbuNey7tK4hTKrwNwYtGqp7bDfCy2WdR3P6735W5Yfpe",
          relationship_type: "same_transaction",
          participant_role: "input",
          block_height: 42,
          block_time: "2026-11-16T10:00:00Z",
          confirmed: true,
          provider: "Blockfrost",
          source_host: "cardano-mainnet.blockfrost.io",
          observed_at: "2026-11-16T10:00:00Z",
        },
      ],
    } as unknown as WalletHistory;

    const graph = buildWalletFlowGraph([history]);
    expect(graph.edges).toHaveLength(1);
    expect(graph.edges[0]).toMatchObject({
      kind: "same_transaction",
      transfers: 0,
      amount: "0",
    });
    expect(graph.sourceRelationshipIds).toEqual(["relationship-1"]);
    expect(
      findObservedPaths(graph.edges, graph.edges[0].from, 3).paths,
    ).toEqual([]);
  });

  it("prefills only untracked counterparty addresses", () => {
    const counterparty: FlowNode = {
      id: "base:0x2222",
      network: "base",
      address: "0x2222222222222222222222222222222222222222",
      label: "0x2222…222222",
      tracked: false,
      x: 0,
      y: 0,
    };

    expect(getCounterpartyPrefill(counterparty)).toEqual({
      address: counterparty.address,
      network: "base",
    });
    expect(
      getCounterpartyPrefill({ ...counterparty, tracked: true }),
    ).toBeNull();
  });

  it("builds a directional edge and keeps raw token decimals exact", () => {
    const history = {
      address: trackedAddress,
      transfers: [transfer()],
    } as unknown as WalletHistory;

    const graph = buildWalletFlowGraph([history]);

    expect(graph.nodes).toHaveLength(2);
    expect(graph.sourceTransferIds).toEqual(["event-1"]);
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

  it("includes catalogued EVM chain ids and keeps edges isolated by chain", () => {
    const chainTen = {
      address: { ...trackedAddress, network: "eip155:10" },
      transfers: [transfer({ network: "eip155:10" })],
    } as unknown as WalletHistory;
    const chainTwenty = {
      address: { ...trackedAddress, network: "eip155:20" },
      transfers: [
        transfer({
          id: "event-chain-20",
          network: "eip155:20",
        }),
      ],
    } as unknown as WalletHistory;

    const graph = buildWalletFlowGraph([chainTen, chainTwenty]);

    expect(graph.edges).toHaveLength(2);
    expect(graph.nodes.map(({ id }) => id)).toContain(
      `eip155:10:${trackedAddress.address}`,
    );
    expect(graph.nodes.map(({ id }) => id)).toContain(
      `eip155:20:${trackedAddress.address}`,
    );
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

  it("keeps TRON addresses case-sensitive and includes token counterparties", () => {
    const tronAddress = "TUoHaVjx7n5xz8LwPRDckgFrDWhMhuSuJM";
    const counterparty = "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t";
    const history = {
      address: { ...trackedAddress, network: "tron", address: tronAddress },
      transfers: [
        transfer({
          id: "tron-event",
          network: "tron",
          transaction_hash: "aBcD",
          transfer_kind: "trc20",
          token_contract_address: counterparty,
          from_address: tronAddress,
          to_address: counterparty,
        }),
      ],
    } as unknown as WalletHistory;

    const graph = buildWalletFlowGraph([history]);

    expect(graph.nodes.map(({ address }) => address)).toContain(tronAddress);
    expect(graph.nodes.map(({ address }) => address)).toContain(counterparty);
    expect(graph.nodes.find(({ tracked }) => tracked)?.address).toBe(
      tronAddress,
    );
    expect(graph.edges[0]?.asset).toBe("USDC");
    expect(graph.sourceTransferIds).toEqual(["tron-event"]);
  });

  it("parses dynamic EVM network ids without dropping the chain id", () => {
    const history = {
      address: { ...trackedAddress, network: "eip155:10" },
      transfers: [transfer({ network: "eip155:10" })],
    } as unknown as WalletHistory;

    expect(
      buildWalletFlowGraph([history]).nodes.map(({ network }) => network),
    ).toEqual(["eip155:10", "eip155:10"]);
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
    kind: "transfer",
    relationships: 0,
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
