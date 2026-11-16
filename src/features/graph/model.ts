import type {
  ObservedTransfer,
  TrackedAddress,
  WalletHistory,
} from "../investigations/api";

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

export type FlowEdge = {
  id: string;
  from: string;
  to: string;
  asset: string;
  transfers: number;
  amount: string;
  decimals: number;
  latestAt: string;
};

export type WalletFlowGraphModel = {
  nodes: FlowNode[];
  edges: FlowEdge[];
  truncated: boolean;
};

const evmNetworks = new Set(["ethereum", "base", "arbitrum"]);

function addressKey(network: string, address: string): string {
  return `${network.toLowerCase()}:${address.toLowerCase()}`;
}

function eventKey(transfer: ObservedTransfer): string {
  return [
    transfer.network.toLowerCase(),
    transfer.transaction_hash.toLowerCase(),
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
  let truncated = false;
  for (const { transfers } of histories) {
    for (const transfer of transfers) {
      if (!evmNetworks.has(transfer.network.toLowerCase())) continue;
      if (!transfer.from_address || !transfer.to_address) continue;
      const key = eventKey(transfer);
      if (!uniqueEvents.has(key)) {
        if (uniqueEvents.size >= MAX_RECORDS) {
          truncated = true;
          break;
        }
        uniqueEvents.set(key, transfer);
      }
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
    }
  >();
  const nodeKeys = new Set<string>();
  for (const transfer of uniqueEvents.values()) {
    const from = addressKey(transfer.network, transfer.from_address!);
    const to = addressKey(transfer.network, transfer.to_address!);
    nodeKeys.add(from);
    nodeKeys.add(to);
    const assetKey =
      transfer.token_contract_address?.toLowerCase() || transfer.asset;
    const id = `${from}>${to}:${assetKey}`;
    const existing = edgeRows.get(id);
    if (existing) {
      existing.amount += rawAmount(transfer);
      existing.transfers += 1;
      if (transfer.block_time && transfer.block_time > existing.latestAt) {
        existing.latestAt = transfer.block_time;
      }
    } else {
      edgeRows.set(id, {
        from,
        to,
        asset: transfer.asset,
        amount: rawAmount(transfer),
        decimals: transfer.token_decimals ?? 0,
        transfers: 1,
        latestAt: transfer.block_time ?? transfer.observed_at,
      });
    }
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
    }));
  if (edgeRows.size > MAX_EDGES) truncated = true;

  const radius = orderedNodeKeys.length > 1 ? 180 : 0;
  const nodes = orderedNodeKeys.map((id, index) => {
    const [, address] = id.split(":");
    const [network] = id.split(":");
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

  return { nodes, edges, truncated };
}
