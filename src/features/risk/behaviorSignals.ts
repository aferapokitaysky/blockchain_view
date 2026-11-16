import type { ObservedTransfer } from "../investigations/api";
import type { RiskSignal } from "./scoreWallet";

const HOURS_24_MS = 24 * 60 * 60 * 1_000;
const MINIMUM_COUNTERPARTIES = 4;

type TimedTransfer = {
  transfer: ObservedTransfer;
  timestamp: number;
  counterparty: string;
};

function getTimedTransfers(
  transfers: ObservedTransfer[],
  direction: "inbound" | "outbound",
): TimedTransfer[] {
  return transfers.flatMap((transfer) => {
    if (!transfer.confirmed || transfer.direction !== direction) return [];

    const counterparty =
      direction === "inbound" ? transfer.from_address : transfer.to_address;
    const timestamp = Date.parse(transfer.block_time ?? transfer.observed_at);
    if (!counterparty || !Number.isFinite(timestamp)) return [];

    return [{ transfer, timestamp, counterparty: counterparty.toLowerCase() }];
  });
}

function findPeakCounterpartyWindow(
  transfers: TimedTransfer[],
): TimedTransfer[] {
  const ordered = [...transfers].sort(
    (left, right) => left.timestamp - right.timestamp,
  );
  let start = 0;
  let bestStart = 0;
  let bestEnd = -1;
  let bestCount = 0;
  const counterparties = new Map<string, number>();

  for (let end = 0; end < ordered.length; end += 1) {
    const counterparty = ordered[end].counterparty;
    counterparties.set(
      counterparty,
      (counterparties.get(counterparty) ?? 0) + 1,
    );
    while (ordered[end].timestamp - ordered[start].timestamp > HOURS_24_MS) {
      const expired = ordered[start].counterparty;
      const remaining = (counterparties.get(expired) ?? 0) - 1;
      if (remaining === 0) counterparties.delete(expired);
      else counterparties.set(expired, remaining);
      start += 1;
    }
    if (counterparties.size > bestCount) {
      bestStart = start;
      bestEnd = end;
      bestCount = counterparties.size;
    }
  }

  return bestEnd < 0 ? [] : ordered.slice(bestStart, bestEnd + 1);
}

function buildFanSignal(
  direction: "inbound" | "outbound",
  transfers: TimedTransfer[],
): RiskSignal | null {
  const window = findPeakCounterpartyWindow(transfers);
  const counterparties = new Set(
    window.map(({ counterparty }) => counterparty),
  );
  if (counterparties.size < MINIMUM_COUNTERPARTIES) return null;

  const latest = window.reduce((newest, current) =>
    current.timestamp > newest.timestamp ? current : newest,
  );
  const earliest = window.reduce((oldest, current) =>
    current.timestamp < oldest.timestamp ? current : oldest,
  );
  const networks = [
    ...new Set(window.map(({ transfer }) => transfer.network)),
  ].sort();
  const directionLabel = direction;

  return {
    id: `behavior:fan-${directionLabel}:${networks.join(",")}:${latest.transfer.id}`,
    title:
      direction === "outbound"
        ? "Observed fan-out pattern"
        : "Observed fan-in pattern",
    detail: `${counterparties.size} distinct ${directionLabel} counterparties from ${new Date(earliest.timestamp).toISOString()} to ${new Date(latest.timestamp).toISOString()} (UTC), within a rolling 24-hour window on ${networks.join(", ")}. This describes saved observations only; it does not establish common ownership or intent.`,
    category: "behavior",
    severity: "medium",
    hops: 0,
    source: `Saved chain observations · ${[...new Set(window.map(({ transfer }) => transfer.provider))].join(", ")}`,
    sourceReference: null,
    observedAt: new Date(latest.timestamp).toISOString(),
  };
}

/** Derives conservative fan-in/fan-out signals from confirmed saved observations. */
export function deriveBehaviorSignals(
  transfers: ObservedTransfer[],
): RiskSignal[] {
  const outbound = buildFanSignal(
    "outbound",
    getTimedTransfers(transfers, "outbound"),
  );
  const inbound = buildFanSignal(
    "inbound",
    getTimedTransfers(transfers, "inbound"),
  );
  return [outbound, inbound].filter(
    (signal): signal is RiskSignal => signal !== null,
  );
}
