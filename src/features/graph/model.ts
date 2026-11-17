import type {
  ObservedTransfer,
  ObservedRelationship,
  TrackedAddress,
  WalletHistory,
} from "../investigations/api";
import { isEvmNetworkId } from "../networks/networkCatalog";

const MAX_RECORDS = 10_000;
const MAX_EDGES = 400;
const MAX_NODES = 180;

export type FlowNode = {
  id: string;
  network: string;
  address: string;
  label: string;
  tracked: boolean;
  x: number;
  y: number;
};

export type CounterpartyPrefill = {
  address: string;
  network: string;
};

export function getCounterpartyPrefill(
  node: FlowNode,
): CounterpartyPrefill | null {
  if (node.tracked) return null;
  return { address: node.address, network: node.network };
}

export type FlowEdge = {
  id: string;
  from: string;
  to: string;
  asset: string;
  transfers: number;
  amount: string;
  decimals: number;
  latestAt: string;
  latestTimeKind: "block" | "observed";
  providers: string[];
  sourceHosts: string[];
  kind: "transfer" | "same_transaction";
  relationships: number;
};

export type WalletFlowGraphModel = {
  nodes: FlowNode[];
  edges: FlowEdge[];
  truncated: boolean;
  sourceTransferIds: string[];
  sourceRelationshipIds: string[];
};

export type ObservedPathAnalysis = {
  depthByNode: Map<string, number>;
  paths: Array<{ targetId: string; depth: number; edgeIds: string[] }>;
  pathEdgeIds: Set<string>;
};

export function hasGraphEvidence(
  graph: Pick<WalletFlowGraphModel, "sourceTransferIds" | "sourceRelationshipIds">,
): boolean {
  return graph.sourceTransferIds.length > 0 || graph.sourceRelationshipIds.length > 0;
}

function addressKey(network: string, address: string): string {
  const normalizedNetwork = network.toLowerCase();
  const normalizedAddress = isEvmNetworkId(normalizedNetwork)
    ? address.toLowerCase()
    : address;
  return `${normalizedNetwork}:${normalizedAddress}`;
}

function eventKey(transfer: ObservedTransfer): string {
  const network = transfer.network.toLowerCase();
  const transactionHash =
    isEvmNetworkId(network) ||
    network === "tron" ||
    network === "xrpl" ||
    network === "ton"
      ? transfer.transaction_hash.toLowerCase()
      : transfer.transaction_hash;
  return [
    network,
    transactionHash,
    transfer.transfer_kind ?? "evm_native",
    transfer.event_index ?? -1,
  ].join(":");
}

function rawAmount(transfer: ObservedTransfer): bigint {
  const received = BigInt(transfer.received_amount || "0");
  const spent = BigInt(transfer.spent_amount || "0");
  return received > spent ? received : spent;
}

function formatRawAmount(amount: bigint, decimals: number): string {
  if (decimals <= 0) return amount.toString();
  const digits = amount.toString().padStart(decimals + 1, "0");
  const whole = digits.slice(0, -decimals);
  const fraction = digits.slice(-decimals).replace(/0+$/, "");
  return fraction ? `${whole}.${fraction}` : whole;
}

function nodeLabel(address: TrackedAddress | undefined, value: string): string {
  return address?.label?.trim() || `${value.slice(0, 7)}…${value.slice(-5)}`;
}

export function buildWalletFlowGraph(
  histories: WalletHistory[],
): WalletFlowGraphModel {
  const knownAddresses = new Map<string, TrackedAddress>();
  histories.forEach(({ address }) => {
    knownAddresses.set(addressKey(address.network, address.address), address);
  });

  const uniqueEvents = new Map<string, ObservedTransfer>();
  const sourceTransferIds = new Set<string>();
  const sourceRelationshipIds = new Set<string>();
  let truncated = false;
  for (const { transfers } of histories) {
    for (const transfer of transfers) {
      if (
        !isEvmNetworkId(transfer.network) &&
        !["tron", "solana", "xrpl", "ton"].includes(
          transfer.network.toLowerCase(),
        )
      ) {
        continue;
      }
      if (!transfer.from_address || !transfer.to_address) continue;
      const key = eventKey(transfer);
      if (!uniqueEvents.has(key)) {
        if (uniqueEvents.size >= MAX_RECORDS) {
          truncated = true;
          break;
        }
        uniqueEvents.set(key, transfer);
      }
      sourceTransferIds.add(transfer.id);
    }
    if (truncated) break;
  }

  const edgeRows = new Map<
    string,
    {
      from: string;
      to: string;
      asset: string;
      amount: bigint;
      decimals: number;
      transfers: number;
      latestAt: string;
      latestTimeKind: "block" | "observed";
      providers: Set<string>;
      sourceHosts: Set<string>;
      kind: "transfer" | "same_transaction";
      relationships: number;
    }
  >();
  const nodeKeys = new Set<string>();
  for (const transfer of uniqueEvents.values()) {
    const from = addressKey(transfer.network, transfer.from_address!);
    const to = addressKey(transfer.network, transfer.to_address!);
    nodeKeys.add(from);
    nodeKeys.add(to);
    const assetKey = transfer.token_contract_address
      ? isEvmNetworkId(transfer.network)
        ? transfer.token_contract_address.toLowerCase()
        : transfer.token_contract_address
      : transfer.asset;
    const id = `${from}>${to}:${assetKey}`;
    const observedAt = transfer.block_time ?? transfer.observed_at;
    const timeKind = transfer.block_time ? "block" : "observed";
    const existing = edgeRows.get(id);
    if (existing) {
      existing.amount += rawAmount(transfer);
      existing.transfers += 1;
      if (observedAt > existing.latestAt) {
        existing.latestAt = observedAt;
        existing.latestTimeKind = timeKind;
      }
      existing.providers.add(transfer.provider);
      if (transfer.source_host) existing.sourceHosts.add(transfer.source_host);
    } else {
      edgeRows.set(id, {
        from,
        to,
        asset: transfer.asset,
        amount: rawAmount(transfer),
        decimals: transfer.token_decimals ?? 0,
        transfers: 1,
        latestAt: observedAt,
        latestTimeKind: timeKind,
        providers: new Set([transfer.provider]),
        sourceHosts: new Set(
          transfer.source_host ? [transfer.source_host] : [],
        ),
        kind: "transfer",
        relationships: 0,
      });
    }
  }

  const relationships = new Map<string, ObservedRelationship>();
  for (const history of histories) {
    for (const relationship of history.relationships ?? []) {
      if (relationship.relationship_type !== "same_transaction") continue;
      const tracked = addressKey(history.address.network, history.address.address);
      const counterparty = addressKey(history.address.network, relationship.counterparty_address);
      if (tracked === counterparty) continue;
      if (
        !sourceRelationshipIds.has(relationship.id) &&
        sourceTransferIds.size + sourceRelationshipIds.size >= MAX_RECORDS
      ) {
        truncated = true;
        break;
      }
      const [first, second] = [tracked, counterparty].sort();
      const id = `${first}~${second}:same_transaction`;
      relationships.set(`${id}:${relationship.id}`, relationship);
      sourceRelationshipIds.add(relationship.id);
      nodeKeys.add(tracked);
      nodeKeys.add(counterparty);
    }
    if (truncated) break;
  }
  for (const [relationshipKey, relationship] of relationships) {
    const id = relationshipKey.slice(0, relationshipKey.indexOf(":same_transaction:"));
    const [from, to] = id.split("~");
    const existing = edgeRows.get(id);
    if (existing) {
      existing.relationships += 1;
      existing.providers.add(relationship.provider);
      if (relationship.source_host) existing.sourceHosts.add(relationship.source_host);
      continue;
    }
    edgeRows.set(id, {
      from,
      to,
      asset: "Cardano transaction",
      amount: 0n,
      decimals: 0,
      transfers: 0,
      latestAt: relationship.block_time ?? relationship.observed_at,
      latestTimeKind: relationship.block_time ? "block" : "observed",
      providers: new Set([relationship.provider]),
      sourceHosts: new Set(relationship.source_host ? [relationship.source_host] : []),
      kind: "same_transaction",
      relationships: 1,
    });
  }

  const nodeWeights = new Map<string, number>();
  for (const edge of edgeRows.values()) {
    nodeWeights.set(
      edge.from,
      (nodeWeights.get(edge.from) ?? 0) + edge.transfers,
    );
    nodeWeights.set(edge.to, (nodeWeights.get(edge.to) ?? 0) + edge.transfers);
  }
  const rankedNodeKeys = [...nodeKeys].sort(
    (left, right) =>
      (nodeWeights.get(right) ?? 0) - (nodeWeights.get(left) ?? 0) ||
      left.localeCompare(right),
  );
  if (rankedNodeKeys.length > MAX_NODES) {
    truncated = true;
  }
  const orderedNodeKeys = rankedNodeKeys.slice(0, MAX_NODES).sort();
  const visibleNodeKeys = new Set(orderedNodeKeys);
  const edges: FlowEdge[] = [...edgeRows.entries()]
    .filter(
      ([, edge]) =>
        visibleNodeKeys.has(edge.from) && visibleNodeKeys.has(edge.to),
    )
    .sort(([, left], [, right]) => right.latestAt.localeCompare(left.latestAt))
    .slice(0, MAX_EDGES)
    .map(([id, edge]) => ({
      id,
      from: edge.from,
      to: edge.to,
      asset: edge.asset,
      transfers: edge.transfers,
      amount: formatRawAmount(edge.amount, edge.decimals),
      decimals: edge.decimals,
      latestAt: edge.latestAt,
      latestTimeKind: edge.latestTimeKind,
      providers: [...edge.providers].sort(),
      sourceHosts: [...edge.sourceHosts].sort(),
      kind: edge.kind,
      relationships: edge.relationships,
    }));
  if (edgeRows.size > MAX_EDGES) truncated = true;

  const radius = orderedNodeKeys.length > 1 ? 180 : 0;
  const nodes = orderedNodeKeys.map((id, index) => {
    const separator = id.startsWith("eip155:")
      ? id.indexOf(":", 7)
      : id.indexOf(":");
    const network = id.slice(0, separator);
    const address = id.slice(separator + 1);
    const tracked = knownAddresses.get(id);
    const angle = (2 * Math.PI * index) / Math.max(orderedNodeKeys.length, 1);
    return {
      id,
      network,
      address,
      label: nodeLabel(tracked, address),
      tracked: Boolean(tracked),
      x: 500 + Math.cos(angle) * radius,
      y: 250 + Math.sin(angle) * radius * 0.72,
    };
  });

  return {
    nodes,
    edges,
    truncated,
    sourceTransferIds: [...sourceTransferIds],
    sourceRelationshipIds: [...sourceRelationshipIds],
  };
}

export function findObservedPaths(
  edges: FlowEdge[],
  originId: string,
  maxDepth: 1 | 2 | 3,
): ObservedPathAnalysis {
  const outgoing = new Map<string, FlowEdge[]>();
  for (const edge of edges) {
    if (edge.kind !== "transfer") continue;
    const fromEdges = outgoing.get(edge.from) ?? [];
    fromEdges.push(edge);
    outgoing.set(edge.from, fromEdges);
  }

  const depthByNode = new Map([[originId, 0]]);
  const paths: ObservedPathAnalysis["paths"] = [];
  const pathByNode = new Map<string, string[]>([[originId, []]]);
  const pathEdgeIds = new Set<string>();
  const queue = [originId];
  for (let cursor = 0; cursor < queue.length; cursor += 1) {
    const from = queue[cursor];
    const depth = depthByNode.get(from) ?? 0;
    if (depth >= maxDepth) continue;

    for (const edge of outgoing.get(from) ?? []) {
      if (depthByNode.has(edge.to)) continue;
      const nextDepth = depth + 1;
      depthByNode.set(edge.to, nextDepth);
      queue.push(edge.to);
      const edgeIds = [...(pathByNode.get(from) ?? []), edge.id];
      pathByNode.set(edge.to, edgeIds);
      paths.push({ targetId: edge.to, depth: nextDepth, edgeIds });
      edgeIds.forEach((edgeId) => pathEdgeIds.add(edgeId));
    }
  }

  return { depthByNode, paths, pathEdgeIds };
}
