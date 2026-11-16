export type RiskSignal = {
  id: string;
  title: string;
  detail: string;
  category: "sanctions" | "exposure" | "behavior";
  severity: "critical" | "high" | "medium" | "low";
  hops: number;
  source: string;
  observedAt: string;
};

export type RiskAssessment = {
  score: number;
  level: "low" | "guarded" | "elevated" | "high";
  signals: RiskSignal[];
  disclaimer: string;
};

/**
 * Converts one observation into its contribution to the aggregate index.
 * Values are provisional and should be calibrated against reviewed cases.
 */
function weightSignal(signal: RiskSignal): number {
  // TODO: Calibrate these multipliers for the sources and review policy chosen by the project.
  const distanceFactor = signal.hops === 0 ? 1 : 1 / (signal.hops + 1);
  const categoryFactor =
    signal.category === "sanctions"
      ? 1
      : signal.category === "behavior"
        ? 0.7
        : 0.55;
  return SIGNAL_WEIGHTS[signal.severity] * distanceFactor * categoryFactor;
}

const SIGNAL_WEIGHTS: Record<RiskSignal["severity"], number> = {
  critical: 80,
  high: 48,
  medium: 24,
  low: 8,
};

/**
 * Produces an explainable exposure index from attributed evidence.
 * This is a prioritization aid, not a probability or a compliance decision.
 */
export function scoreWallet(signals: RiskSignal[]): RiskAssessment {
  const score = Math.min(
    100,
    signals.reduce((total, signal) => total + weightSignal(signal), 0),
  );

  const level =
    score >= 75
      ? "high"
      : score >= 45
        ? "elevated"
        : score >= 20
          ? "guarded"
          : "low";
  return {
    score: Math.round(score),
    level,
    signals,
    disclaimer:
      "Risk exposure index. Signals require analyst review and do not establish wrongdoing.",
  };
}

export const DEMO_RISK_SIGNALS: RiskSignal[] = [
  {
    id: "rapid-pass",
    title: "Rapid pass-through",
    detail: "82% of received value moved again within 14 minutes.",
    category: "behavior",
    severity: "medium",
    hops: 0,
    source: "Observed transaction history",
    observedAt: "2026-11-16T09:32:00Z",
  },
  {
    id: "indirect-service",
    title: "Indirect service exposure",
    detail:
      "One counterparty is labeled as a high-risk service by a demo source.",
    category: "exposure",
    severity: "high",
    hops: 1,
    source: "Demo attribution dataset",
    observedAt: "2026-11-15T18:12:00Z",
  },
  {
    id: "fan-out",
    title: "Unusual fan-out",
    detail: "Funds split across 11 new addresses in a short window.",
    category: "behavior",
    severity: "medium",
    hops: 0,
    source: "Observed transaction history",
    observedAt: "2026-11-14T14:06:00Z",
  },
];

export const DEMO_WALLET_ASSESSMENT = scoreWallet(DEMO_RISK_SIGNALS);
