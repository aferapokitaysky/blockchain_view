import type {
  AddressScreeningResult,
  ObservedTransfer,
  StoredRiskSignal,
} from "../investigations/api";
import { isEvmNetworkId } from "../networks/networkCatalog";

export type TransferRiskEvidence = {
  id: string;
  title: string;
  severity: "critical" | "high" | "medium" | "low" | "informational";
  scope: "direct" | "indirect" | "behavioral";
  hops: number;
  source: string;
  reference: string | null;
};

export type TransferRiskAssessment = {
  score: number;
  level: "unknown" | "low" | "guarded" | "elevated" | "high";
  modelVersion: string;
  contributions: Array<{
    evidence: TransferRiskEvidence;
    basePoints: number;
    factor: number;
    points: number;
  }>;
  disclaimer: string;
};

export const TRANSFER_RISK_MODEL_VERSION = "0.1.0";

const severityPoints: Record<TransferRiskEvidence["severity"], number> = {
  critical: 80,
  high: 48,
  medium: 24,
  low: 8,
  informational: 0,
};

function normalizeCounterpartyAddress(
  network: string,
  address: string,
): string {
  if (isEvmNetworkId(network)) return address.toLowerCase();
  if (
    network.toLowerCase() === "bitcoin" &&
    /^(bc1|tb1|bcrt1)/i.test(address)
  ) {
    return address.toLowerCase();
  }
  return address;
}

function normalizeTransactionHash(network: string, hash: string): string {
  if (isEvmNetworkId(network) || network.toLowerCase() === "bitcoin") {
    return hash.toLowerCase();
  }
  return hash;
}

export function collectTransferRiskEvidence(
  transfer: ObservedTransfer,
  analystSignals: StoredRiskSignal[],
  screening: AddressScreeningResult | null,
): TransferRiskEvidence[] {
  const transactionHash = normalizeTransactionHash(
    transfer.network,
    transfer.transaction_hash,
  );
  const analystEvidence = analystSignals.flatMap((signal) => {
    const linkedHash = signal.details.transaction_hash;
    if (
      typeof linkedHash !== "string" ||
      normalizeTransactionHash(transfer.network, linkedHash) !== transactionHash
    ) {
      return [];
    }

    const hops = signal.details.hops;
    const isIndirect = typeof hops === "number" && hops > 0;
    return [
      {
        id: signal.id,
        title: signal.signal_key,
        severity: signal.severity,
        scope:
          signal.category === "behavior"
            ? "behavioral"
            : isIndirect
              ? "indirect"
              : "direct",
        hops: isIndirect ? hops : 0,
        source: signal.source_name,
        reference: signal.source_reference,
      } satisfies TransferRiskEvidence,
    ];
  });

  if (
    !screening ||
    screening.status !== "exact_match" ||
    !screening.source ||
    screening.network.toLowerCase() !== transfer.network.toLowerCase()
  ) {
    return analystEvidence;
  }

  const participatingAddresses = new Set(
    [transfer.from_address, transfer.to_address]
      .filter((address): address is string => Boolean(address))
      .map((address) =>
        normalizeCounterpartyAddress(transfer.network, address),
      ),
  );
  const screeningEvidence = screening.matches
    .filter((match) =>
      participatingAddresses.has(
        normalizeCounterpartyAddress(transfer.network, match.address),
      ),
    )
    .map((match) => ({
      id: `ofac:${match.entity_uid}:${normalizeTransactionHash(transfer.network, transfer.transaction_hash)}`,
      title: match.entity_name,
      severity: "critical" as const,
      scope: "direct" as const,
      hops: 0,
      source: `${screening.source?.source_name} · published ${screening.source?.published_at}`,
      reference: null,
    }));

  return [...analystEvidence, ...screeningEvidence];
}

function evidenceFactor(evidence: TransferRiskEvidence): number {
  switch (evidence.scope) {
    case "direct":
      return 1;
    case "indirect":
      return 1 / (boundedHopCount(evidence.hops) + 1);
    case "behavioral":
      return 0.9;
  }
}

function boundedHopCount(hops: number): number {
  if (!Number.isFinite(hops)) return 10;
  return Math.max(0, Math.min(10, Math.floor(hops)));
}

/** Рассчитывает приоритет ручной проверки одного перевода, а не вероятность нарушения. */
export function scoreTransfer(
  evidence: TransferRiskEvidence[],
): TransferRiskAssessment {
  const contributions = evidence
    .map((item) => {
      const basePoints = severityPoints[item.severity];
      const factor = evidenceFactor(item);
      return {
        evidence: item,
        basePoints,
        factor,
        points: Math.round(basePoints * factor * 100) / 100,
      };
    })
    .sort(
      (left, right) =>
        right.points - left.points ||
        left.evidence.id.localeCompare(right.evidence.id),
    );
  const score = Math.round(
    Math.min(
      100,
      contributions.reduce((total, item) => total + item.points, 0),
    ),
  );

  return {
    score,
    level:
      contributions.length === 0
        ? "unknown"
        : score >= 75
          ? "high"
          : score >= 45
            ? "elevated"
            : score >= 20
              ? "guarded"
              : "low",
    modelVersion: TRANSFER_RISK_MODEL_VERSION,
    contributions,
    disclaimer:
      "Review-priority index only. Evidence does not establish wrongdoing or legal status.",
  };
}
