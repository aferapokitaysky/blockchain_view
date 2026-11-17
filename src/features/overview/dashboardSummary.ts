import type { TrackedAddress, WalletHistory } from "../investigations/api";
import { buildWalletFlowGraph } from "../graph/model";

function transactionKey(network: string, hash: string): string {
  const normalizedNetwork = network.toLowerCase();
  const caseSensitiveHash = ["solana", "sui"].includes(normalizedNetwork);
  return `${normalizedNetwork}:${caseSensitiveHash ? hash : hash.toLowerCase()}`;
}

export type DashboardSummary = {
  trackedAddresses: number;
  networks: number;
  transactions: number;
  connections: number;
};

export type ActiveInvestigationData = {
  investigationId: string | null;
  addresses: TrackedAddress[];
  histories: WalletHistory[];
};

export function getActiveInvestigationData(
  addresses: TrackedAddress[],
  histories: WalletHistory[],
  investigationId: string | null,
): ActiveInvestigationData {
  if (!investigationId) {
    return { investigationId: null, addresses: [], histories: [] };
  }

  return {
    investigationId,
    addresses: addresses.filter(
      ({ investigation_id }) => investigation_id === investigationId,
    ),
    histories: histories.filter(
      ({ address }) => address.investigation_id === investigationId,
    ),
  };
}

export function getDashboardSummary(
  addresses: TrackedAddress[],
  histories: WalletHistory[],
): DashboardSummary {
  const transactionKeys = new Set<string>();
  for (const { transfers } of histories) {
    for (const transfer of transfers) {
      transactionKeys.add(
        transactionKey(transfer.network, transfer.transaction_hash),
      );
    }
  }
  for (const { relationships = [] } of histories) {
    for (const relationship of relationships) {
      transactionKeys.add(
        transactionKey(relationship.network, relationship.transaction_hash),
      );
    }
  }

  return {
    trackedAddresses: addresses.length,
    networks: new Set(addresses.map(({ network }) => network.toLowerCase()))
      .size,
    transactions: transactionKeys.size,
    connections: buildWalletFlowGraph(histories).edges.length,
  };
}
