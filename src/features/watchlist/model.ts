import type { TrackedAddress, WalletHistory } from "../investigations/api";

export type WatchlistEntry = {
  address: TrackedAddress;
  networkName: string;
  transferCount: number | null;
  latestObservedAt: string | null;
};

export type WatchlistFilters = {
  query: string;
  network: string;
};

export function buildWatchlistEntries(
  addresses: TrackedAddress[],
  histories: WalletHistory[],
  networkNames: Readonly<Record<string, string>>,
  historiesReady: boolean,
): WatchlistEntry[] {
  const historyByAddress = new Map(
    histories.map((history) => [history.address.id, history]),
  );

  return addresses.map((address) => {
    const history = historyByAddress.get(address.id);
    const times = history?.transfers
      .map((transfer) => transfer.block_time ?? transfer.observed_at)
      .filter(Boolean)
      .sort((left, right) => right.localeCompare(left));

    return {
      address,
      networkName: networkNames[address.network] ?? address.network,
      transferCount: historiesReady ? (history?.transfers.length ?? 0) : null,
      latestObservedAt: times?.[0] ?? null,
    };
  });
}

export function filterWatchlistEntries(
  entries: WatchlistEntry[],
  filters: WatchlistFilters,
): WatchlistEntry[] {
  const query = filters.query.trim().toLocaleLowerCase();
  const network = filters.network.toLocaleLowerCase();

  return entries.filter(({ address, networkName }) => {
    const matchesNetwork =
      network === "all" || address.network.toLocaleLowerCase() === network;
    if (!matchesNetwork) return false;
    if (!query) return true;

    return [address.label, address.address, networkName, address.network].some(
      (value) => value?.toLocaleLowerCase().includes(query),
    );
  });
}

export function formatWatchlistDate(value: string | null): string {
  if (!value) return "No observations loaded";
  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? "Observation time unavailable"
    : new Intl.DateTimeFormat(undefined, {
        dateStyle: "medium",
        timeStyle: "short",
      }).format(date);
}
