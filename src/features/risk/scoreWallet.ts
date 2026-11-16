import type {
  AddressScreeningResult,
  ObservedTransfer,
  StoredRiskSignal,
} from "../investigations/api";
import { deriveBehaviorSignals } from "./behaviorSignals";

export const RISK_MODEL_VERSION = "0.3.0";

export type RiskSignal = {
  id: string;
  title: string;
  detail: string;
  category: "sanctions" | "exposure" | "behavior" | "other";
  severity: "critical" | "high" | "medium" | "low" | "informational";
  hops: number;
  source: string;
  sourceReference: string | null;
  observedAt: string;
};

export type ScoredRiskSignal = {
  signal: RiskSignal;
  baseWeight: number;
  distanceFactor: number;
  categoryFactor: number;
  contribution: number;
};

export type RiskAssessment = {
  score: number;
  level: "low" | "guarded" | "elevated" | "high";
  modelVersion: string;
  contributions: ScoredRiskSignal[];
  disclaimer: string;
};

const severityWeights: Record<RiskSignal["severity"], number> = {
  critical: 80,
  high: 48,
  medium: 24,
  low: 8,
  informational: 0,
};

function categoryWeight(category: RiskSignal["category"]): number {
  switch (category) {
    case "sanctions":
      return 1;
    case "behavior":
      return 0.7;
    case "exposure":
      return 0.55;
    case "other":
      return 0;
  }
}

function parseHopCount(details: Record<string, unknown>): number {
  const raw = details.hops ?? details.hop_count;
  if (typeof raw !== "number" || !Number.isInteger(raw)) return 0;
  return Math.max(0, Math.min(raw, 10));
}

function describeDetails(details: Record<string, unknown>): string {
  for (const key of ["summary", "description", "note"]) {
    const value = details[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return "Analyst-provided evidence. Review the source before relying on it.";
}

function humanizeSignalKey(signalKey: string): string {
  return signalKey
    .split(/[_\s-]+/)
    .filter(Boolean)
    .map((part) => part[0]?.toUpperCase() + part.slice(1))
    .join(" ");
}

export function mapStoredRiskSignal(signal: StoredRiskSignal): RiskSignal {
  return {
    id: signal.id,
    title: humanizeSignalKey(signal.signal_key),
    detail: describeDetails(signal.details),
    category: signal.category,
    severity: signal.severity,
    hops: parseHopCount(signal.details),
    source: signal.source_name,
    sourceReference: signal.source_reference,
    observedAt: signal.observed_at,
  };
}

export function mapOfacMatches(
  screening: AddressScreeningResult | null,
): RiskSignal[] {
  if (!screening || screening.status !== "exact_match" || !screening.source) {
    return [];
  }
  const source = screening.source;
  return screening.matches.map((match) => ({
    id: `ofac:${match.entity_uid}:${match.currency_code}:${match.address.toLowerCase()}`,
    title: match.entity_name,
    detail: [
      `Exact ${match.currency_code} address match in OFAC SDN.`,
      match.programs.length > 0
        ? `Programs: ${match.programs.join(", ")}.`
        : "",
    ]
      .filter(Boolean)
      .join(" "),
    category: "sanctions",
    severity: "critical",
    hops: 0,
    source: `${source.source_name} · published ${source.published_at}`,
    sourceReference: null,
    observedAt: source.imported_at,
  }));
}

function scoreSignal(signal: RiskSignal): ScoredRiskSignal {
  const hops = Number.isInteger(signal.hops)
    ? Math.min(10, Math.max(0, signal.hops))
    : 0;
  const distanceFactor = hops === 0 ? 1 : 1 / (hops + 1);
  const categoryFactor = categoryWeight(signal.category);
  const baseWeight = severityWeights[signal.severity];
  return {
    signal,
    baseWeight,
    distanceFactor,
    categoryFactor,
    contribution:
      Math.round(baseWeight * distanceFactor * categoryFactor * 100) / 100,
  };
}

/** Рассчитывает приоритет проверки, а не вероятность нарушения. */
export function scoreWallet(signals: RiskSignal[]): RiskAssessment {
  const contributions = signals
    .map(scoreSignal)
    .sort(
      (left, right) =>
        right.contribution - left.contribution ||
        left.signal.id.localeCompare(right.signal.id),
    );
  const rawScore = contributions.reduce(
    (total, contribution) => total + contribution.contribution,
    0,
  );
  const score = Math.round(Math.min(100, rawScore));
  const level =
    score >= 75
      ? "high"
      : score >= 45
        ? "elevated"
        : score >= 20
          ? "guarded"
          : "low";

  return {
    score,
    level,
    modelVersion: RISK_MODEL_VERSION,
    contributions,
    disclaimer:
      "Review-priority index only. Evidence is not independently verified and does not establish wrongdoing or legal status.",
  };
}

export function buildWalletAssessment(
  storedSignals: StoredRiskSignal[],
  screening: AddressScreeningResult | null,
  observedTransfers: ObservedTransfer[] = [],
): RiskAssessment {
  return scoreWallet([
    ...storedSignals.map(mapStoredRiskSignal),
    ...mapOfacMatches(screening),
    ...deriveBehaviorSignals(observedTransfers),
  ]);
}
