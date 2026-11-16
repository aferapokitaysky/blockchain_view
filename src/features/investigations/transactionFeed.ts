import type { ObservedTransfer, WalletHistory } from "./api";
import { formatObservedAmount } from "./formatAssetAmount";
import { isEvmNetworkId } from "../networks/networkCatalog";
import type { AddressScreeningResult, StoredRiskSignal } from "./api";
import {
  collectTransferRiskEvidence,
  scoreTransfer,
} from "../risk/scoreTransfer";
import type { TransferRiskAssessment } from "../risk/scoreTransfer";

export type TransactionFeedItem = {
  id: string;
  hash: string;
  networkId: string;
  network: string;
  direction: ObservedTransfer["direction"];
  from: string;
  to: string;
  amount: string;
  asset: string;
  occurredAt: string;
  confirmed: boolean;
  provider: string;
  trackedWallet: string;
  trackedAddressId: string;
  riskAssessment: TransferRiskAssessment | null;
};

export type TransactionRiskContext = {
  trackedAddressId: string;
  signals: StoredRiskSignal[];
  screening: AddressScreeningResult | null;
};

const NETWORK_NAMES: Record<string, TransactionFeedItem["network"]> = {
  ethereum: "Ethereum",
  bitcoin: "Bitcoin",
  base: "Base",
  arbitrum: "Arbitrum",
};

function shortAddress(address: string): string {
  return address.length > 18
    ? `${address.slice(0, 8)}…${address.slice(-6)}`
    : address;
}

function formatAmount(transfer: ObservedTransfer, rawAmount: string): string {
  return formatObservedAmount(transfer, rawAmount);
}

function getAmount(transfer: ObservedTransfer): string {
  if (transfer.direction === "inbound") {
    return `+${formatAmount(transfer, transfer.received_amount)}`;
  }
  if (transfer.direction === "outbound") {
    return `−${formatAmount(transfer, transfer.spent_amount)}`;
  }
  return `+${formatAmount(transfer, transfer.received_amount)} in · −${formatAmount(transfer, transfer.spent_amount)} out`;
}

function getFlow(transfer: ObservedTransfer, trackedWallet: string) {
  if (transfer.network === "bitcoin") {
    if (transfer.direction === "inbound") {
      return { from: "Unattributed Bitcoin inputs", to: trackedWallet };
    }
    if (transfer.direction === "outbound") {
      return { from: trackedWallet, to: "Unattributed Bitcoin outputs" };
    }
    return { from: "Bitcoin transaction", to: "Tracked wallet activity" };
  }

  return {
    from: transfer.from_address
      ? shortAddress(transfer.from_address)
      : transfer.direction === "inbound"
        ? "Unknown sender"
        : trackedWallet,
    to: transfer.to_address
      ? shortAddress(transfer.to_address)
      : transfer.direction === "outbound"
        ? "Unknown recipient"
        : trackedWallet,
  };
}

function getNetworkName(
  network: string,
  namesById: Record<string, string>,
): string | null {
  const normalizedId = network.toLowerCase();
  return (
    NETWORK_NAMES[normalizedId] ??
    namesById[normalizedId] ??
    (isEvmNetworkId(normalizedId)
      ? `EVM chain ${normalizedId.slice("eip155:".length)}`
      : null)
  );
}

export function buildTransactionFeed(
  histories: WalletHistory[],
  namesById: Record<string, string> = {},
  riskContext: TransactionRiskContext | null = null,
): TransactionFeedItem[] {
  return histories
    .flatMap(({ address, transfers }) => {
      const trackedWallet =
        address.label?.trim() || shortAddress(address.address);
      return transfers.flatMap((transfer) => {
        const network = getNetworkName(transfer.network, namesById);
        if (!network) return [];

        const flow = getFlow(transfer, trackedWallet);
        const riskEvidence =
          address.id === riskContext?.trackedAddressId
            ? collectTransferRiskEvidence(
                transfer,
                riskContext.signals,
                riskContext.screening,
              )
            : [];
        return [
          {
            id: `${transfer.id}:${transfer.event_index ?? "transaction"}`,
            hash: transfer.transaction_hash,
            networkId: transfer.network.toLowerCase(),
            network,
            direction: transfer.direction,
            from: flow.from,
            to: flow.to,
            amount: getAmount(transfer),
            asset: transfer.asset,
            occurredAt: transfer.block_time ?? transfer.observed_at,
            confirmed: transfer.confirmed,
            provider: transfer.provider,
            trackedWallet,
            trackedAddressId: address.id,
            riskAssessment:
              riskEvidence.length > 0 ? scoreTransfer(riskEvidence) : null,
          },
        ];
      });
    })
    .sort((left, right) => {
      const leftTime = Date.parse(left.occurredAt);
      const rightTime = Date.parse(right.occurredAt);
      return (
        (Number.isFinite(rightTime) ? rightTime : 0) -
        (Number.isFinite(leftTime) ? leftTime : 0)
      );
    });
}

export function formatFeedTime(occurredAt: string): string {
  const date = new Date(occurredAt);
  if (!Number.isFinite(date.getTime())) return "Time unavailable";
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}
