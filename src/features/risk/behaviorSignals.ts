import type { ObservedTransfer } from "../investigations/api";
import { formatObservedAmount } from "../investigations/formatAssetAmount";
import type { RiskSignal } from "./scoreWallet";

const HOURS_24_MS = 24 * 60 * 60 * 1_000;
const MINIMUM_COUNTERPARTIES = 4;
const RAPID_TURNOVER_WINDOW_MS = 10 * 60 * 1_000;
const RAPID_TURNOVER_MINIMUM_PAIRS = 2;
const RAPID_TURNOVER_MAX_EVIDENCE_PAIRS = 3;

type TimedTransfer = {
  transfer: ObservedTransfer;
  timestamp: number;
  counterparty: string;
};

type AmountTransfer = {
  transfer: ObservedTransfer;
  timestamp: number;
  amount: bigint;
  eventKey: string;
};

type RapidTurnoverPair = {
  network: string;
  asset: string;
  assetKey: string;
  inbound: AmountTransfer;
  outbound: AmountTransfer;
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

function getRapidAmount(transfer: ObservedTransfer): bigint | null {
  const amount =
    transfer.direction === "inbound"
      ? transfer.received_amount
      : transfer.direction === "outbound"
        ? transfer.spent_amount
        : null;
  if (!amount || !/^\d+$/.test(amount)) return null;
  const parsedAmount = BigInt(amount);
  return parsedAmount > 0n ? parsedAmount : null;
}

function rapidEventKey(transfer: ObservedTransfer): string {
  return [
    transfer.network.toLowerCase(),
    transfer.transaction_hash.toLowerCase(),
    transfer.transfer_kind ?? "",
    transfer.event_index ?? -1,
    transfer.direction,
  ].join(":");
}

function getAmountTransfers(
  transfers: ObservedTransfer[],
  direction: "inbound" | "outbound",
): AmountTransfer[] {
  const events = new Map<string, AmountTransfer>();
  for (const transfer of transfers) {
    // Эти агрегированные изменения не являются доказанными входящими и исходящими переводами.
    if (transfer.network.toLowerCase() === "cardano") continue;
    if (transfer.transfer_kind === "sui_balance_change") continue;
    if (!transfer.confirmed || transfer.direction !== direction) continue;
    // Время записи не заменяет on-chain время транзакции.
    if (!transfer.block_time) continue;
    const timestamp = Date.parse(transfer.block_time);
    const amount = getRapidAmount(transfer);
    if (!Number.isFinite(timestamp) || amount === null) continue;

    const eventKey = rapidEventKey(transfer);
    if (!events.has(eventKey)) {
      events.set(eventKey, { transfer, timestamp, amount, eventKey });
    }
  }
  return [...events.values()];
}

function sameAssetGroupKey(transfer: ObservedTransfer): string {
  const network = transfer.network.toLowerCase();
  const asset =
    transfer.token_contract_address?.toLowerCase() ??
    transfer.asset.trim().toLowerCase();
  return `${network}:${asset}`;
}

function amountsAreSimilar(inbound: bigint, outbound: bigint): boolean {
  return outbound * 10n >= inbound * 7n && outbound * 10n <= inbound * 13n;
}

function pairRapidTurnoverEvents(
  transfers: ObservedTransfer[],
): RapidTurnoverPair[] {
  const inbound = getAmountTransfers(transfers, "inbound");
  const outbound = getAmountTransfers(transfers, "outbound");
  const inboundByGroup = new Map<string, AmountTransfer[]>();
  const outboundByGroup = new Map<string, AmountTransfer[]>();

  for (const event of inbound) {
    const groupKey = sameAssetGroupKey(event.transfer);
    const group = inboundByGroup.get(groupKey) ?? [];
    group.push(event);
    inboundByGroup.set(groupKey, group);
  }
  for (const event of outbound) {
    const groupKey = sameAssetGroupKey(event.transfer);
    const group = outboundByGroup.get(groupKey) ?? [];
    group.push(event);
    outboundByGroup.set(groupKey, group);
  }

  const pairs: RapidTurnoverPair[] = [];
  for (const [groupKey, incomingEvents] of inboundByGroup) {
    const outgoingEvents = outboundByGroup.get(groupKey);
    if (!outgoingEvents) continue;

    const latestTimestamp = Math.max(
      ...incomingEvents.map(({ timestamp }) => timestamp),
      ...outgoingEvents.map(({ timestamp }) => timestamp),
    );
    const windowStart = latestTimestamp - HOURS_24_MS;
    const eligibleIncoming = incomingEvents.filter(
      ({ timestamp }) => timestamp >= windowStart,
    );
    const usedIncoming = new Set<string>();

    for (const outgoing of [...outgoingEvents]
      .filter(({ timestamp }) => timestamp >= windowStart)
      .sort(
        (left, right) =>
          left.timestamp - right.timestamp ||
          left.eventKey.localeCompare(right.eventKey),
      )) {
      const candidates = eligibleIncoming
        .filter((incoming) => {
          const elapsed = outgoing.timestamp - incoming.timestamp;
          return (
            !usedIncoming.has(incoming.eventKey) &&
            elapsed >= 0 &&
            elapsed <= RAPID_TURNOVER_WINDOW_MS &&
            incoming.eventKey !== outgoing.eventKey &&
            incoming.transfer.transaction_hash.toLowerCase() !==
              outgoing.transfer.transaction_hash.toLowerCase() &&
            amountsAreSimilar(incoming.amount, outgoing.amount)
          );
        })
        .sort(
          (left, right) =>
            right.timestamp - left.timestamp ||
            left.eventKey.localeCompare(right.eventKey),
        );
      const incoming = candidates[0];
      if (!incoming) continue;

      usedIncoming.add(incoming.eventKey);
      pairs.push({
        network: outgoing.transfer.network,
        asset: outgoing.transfer.asset,
        assetKey: groupKey,
        inbound: incoming,
        outbound: outgoing,
      });
    }
  }
  return pairs;
}

function formatRapidPair(pair: RapidTurnoverPair): string {
  const elapsedMinutes =
    (pair.outbound.timestamp - pair.inbound.timestamp) / 60_000;
  const inboundAmount = formatRapidAmount(pair.inbound, pair.asset);
  const outboundAmount = formatRapidAmount(pair.outbound, pair.asset);
  return `${pair.network} · ${pair.asset}: inbound ${inboundAmount} at ${new Date(pair.inbound.timestamp).toISOString()} (tx ${pair.inbound.transfer.transaction_hash}), then outbound ${outboundAmount} at ${new Date(pair.outbound.timestamp).toISOString()} (tx ${pair.outbound.transfer.transaction_hash}); ${elapsedMinutes.toFixed(2)} minutes apart.`;
}

function formatRapidAmount(event: AmountTransfer, asset: string): string {
  const formatted = formatObservedAmount(
    event.transfer,
    event.amount.toString(),
  );
  const assetSuffix = event.transfer.network === "bitcoin" ? "" : ` ${asset}`;
  return `${formatted}${assetSuffix} (raw ${event.amount.toString()})`;
}

function buildRapidTurnoverSignal(
  transfers: ObservedTransfer[],
): RiskSignal | null {
  const pairs = pairRapidTurnoverEvents(transfers);
  const pairsByGroup = new Map<string, RapidTurnoverPair[]>();
  for (const pair of pairs) {
    const group = pairsByGroup.get(pair.assetKey) ?? [];
    group.push(pair);
    pairsByGroup.set(pair.assetKey, group);
  }

  const repeatedPairs = [...pairsByGroup.values()]
    .filter((groupPairs) => groupPairs.length >= RAPID_TURNOVER_MINIMUM_PAIRS)
    .flat();
  if (repeatedPairs.length < RAPID_TURNOVER_MINIMUM_PAIRS) return null;

  repeatedPairs.sort(
    (left, right) => right.outbound.timestamp - left.outbound.timestamp,
  );
  const latest = repeatedPairs[0].outbound;
  const evidence = repeatedPairs
    .slice(0, RAPID_TURNOVER_MAX_EVIDENCE_PAIRS)
    .map(formatRapidPair);
  const providers = [
    ...new Set(
      repeatedPairs.flatMap(({ inbound, outbound }) => [
        inbound.transfer.provider,
        outbound.transfer.provider,
      ]),
    ),
  ].sort();
  const omittedPairs = repeatedPairs.length - evidence.length;
  const providerSummary = providers.join(", ");

  return {
    id: `behavior:rapid-turnover:${latest.eventKey}`,
    title: "Repeated rapid inbound/outbound sequence",
    detail: `${repeatedPairs.length} non-overlapping same-network, same-asset inbound→outbound pairs were observed in a rolling 24-hour window from ${providerSummary}. Each pair was within 10 minutes and the outbound raw amount was 70–130% of the inbound amount. Evidence${omittedPairs > 0 ? ` (showing ${evidence.length} of ${repeatedPairs.length} pairs)` : ""}: ${evidence.join(" ")} Similar timing and amounts do not prove the same funds continued or establish intent.`,
    category: "behavior",
    severity: repeatedPairs.length >= 4 ? "medium" : "low",
    hops: 0,
    source: `Saved chain observations · ${providers.join(", ")}`,
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
  const rapidTurnover = buildRapidTurnoverSignal(transfers);
  return [outbound, inbound, rapidTurnover].filter(
    (signal): signal is RiskSignal => signal !== null,
  );
}
