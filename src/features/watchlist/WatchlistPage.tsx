import { useMemo, useState } from "react";
import {
  ArrowUpRight,
  Clock3,
  Plus,
  RefreshCw,
  Search,
  Wallet,
} from "lucide-react";
import type { TrackedAddress, WalletHistory } from "../investigations/api";
import {
  getNetworkSyncAvailability,
  type NetworkCatalog,
} from "../networks/networkCatalog";
import {
  buildWatchlistEntries,
  filterWatchlistEntries,
  formatWatchlistDate,
} from "./model";
import "./WatchlistPage.css";

type WatchlistPageProps = {
  addresses: TrackedAddress[];
  histories: WalletHistory[];
  networkNames: Readonly<Record<string, string>>;
  networkCatalog: NetworkCatalog;
  selectedAddressId: string;
  historyStatus: "loading" | "ready" | "error";
  loading: boolean;
  syncing: boolean;
  apiAvailable: boolean;
  error: string;
  investigationName: string;
  onSelectAddress: (addressId: string) => void;
  onOpenAddress: (addressId: string) => void;
  onAddAddress: () => void;
  onSyncAddress: (address: TrackedAddress) => void;
  onRetry: () => void;
  onOpenInvestigations: () => void;
};

export function WatchlistPage({
  addresses,
  histories,
  networkNames,
  networkCatalog,
  selectedAddressId,
  historyStatus,
  loading,
  syncing,
  apiAvailable,
  error,
  investigationName,
  onSelectAddress,
  onOpenAddress,
  onAddAddress,
  onSyncAddress,
  onRetry,
  onOpenInvestigations,
}: WatchlistPageProps) {
  const [query, setQuery] = useState("");
  const [networkFilter, setNetworkFilter] = useState("all");
  const entries = useMemo(
    () =>
      buildWatchlistEntries(
        addresses,
        histories,
        networkNames,
        historyStatus === "ready",
      ),
    [addresses, histories, historyStatus, networkNames],
  );
  const visibleEntries = useMemo(
    () => filterWatchlistEntries(entries, { query, network: networkFilter }),
    [entries, networkFilter, query],
  );
  const networks = useMemo(
    () =>
      [
        ...new Map(
          entries.map((entry) => [entry.address.network, entry.networkName]),
        ),
      ]
        .map(([id, name]) => ({ id, name }))
        .sort((left, right) => left.name.localeCompare(right.name)),
    [entries],
  );
  const selectedEntry = entries.find(
    ({ address }) => address.id === selectedAddressId,
  );
  const selectedSyncConfigured = selectedEntry
    ? getNetworkSyncAvailability(
        selectedEntry.address.network,
        networkCatalog,
      ) !== false
    : false;
  const canSync = apiAvailable && selectedSyncConfigured;

  return (
    <section className="watchlist-page" aria-labelledby="watchlist-title">
      <header className="watchlist-heading">
        <div>
          <div className="eyebrow">
            <span className="eyebrow-dot" />
            {investigationName || "Investigation"}
          </div>
          <h1 id="watchlist-title">Watchlist</h1>
          <p>
            Saved addresses and the latest history loaded for this
            investigation.
          </p>
        </div>
        <button
          className="button button-primary"
          type="button"
          onClick={onAddAddress}
          disabled={!investigationName}
        >
          <Plus size={16} /> Add address
        </button>
      </header>

      <div className="watchlist-toolbar">
        <label className="watchlist-search" htmlFor="watchlist-search">
          <Search size={16} aria-hidden="true" />
          <span className="visually-hidden">Search saved addresses</span>
          <input
            id="watchlist-search"
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search label or address"
          />
        </label>
        <label className="watchlist-filter" htmlFor="watchlist-network-filter">
          <span>Network</span>
          <select
            id="watchlist-network-filter"
            value={networkFilter}
            onChange={(event) => setNetworkFilter(event.target.value)}
          >
            <option value="all">All networks</option>
            {networks.map(({ id, name }) => (
              <option key={id} value={id}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <span className="watchlist-count" aria-live="polite">
          {loading
            ? "Loading addresses…"
            : `${visibleEntries.length} of ${entries.length} addresses`}
        </span>
      </div>

      {error && addresses.length > 0 && (
        <div className="watchlist-error" role="alert">
          <span>{error}</span>
          <button type="button" className="text-action" onClick={onRetry}>
            Retry
          </button>
        </div>
      )}

      {loading ? (
        <div className="watchlist-state" role="status">
          <span className="watchlist-state-mark">
            <Wallet size={18} />
          </span>
          <strong>Loading saved addresses</strong>
          <p>Addresses are read from the active investigation.</p>
        </div>
      ) : error && addresses.length === 0 ? (
        <div className="watchlist-state" role="alert">
          <span className="watchlist-state-mark">
            <Wallet size={18} />
          </span>
          <strong>Could not load saved addresses</strong>
          <p>{error}</p>
          <button
            className="button button-secondary"
            type="button"
            onClick={onRetry}
          >
            Retry
          </button>
        </div>
      ) : !investigationName ? (
        <div className="watchlist-state">
          <span className="watchlist-state-mark">
            <Wallet size={18} />
          </span>
          <strong>No investigation selected</strong>
          <p>Create or open an investigation before adding addresses.</p>
          <button
            className="button button-secondary"
            type="button"
            onClick={onOpenInvestigations}
          >
            Open investigations <ArrowUpRight size={15} />
          </button>
        </div>
      ) : addresses.length === 0 ? (
        <div className="watchlist-state">
          <span className="watchlist-state-mark">
            <Wallet size={18} />
          </span>
          <strong>No saved addresses</strong>
          <p>
            Add a public wallet address to load its history and map observed
            counterparties.
          </p>
          <button
            className="button button-primary"
            type="button"
            onClick={onAddAddress}
          >
            <Plus size={15} /> Add address
          </button>
        </div>
      ) : visibleEntries.length === 0 ? (
        <div className="watchlist-state">
          <strong>No addresses match these filters</strong>
          <p>Change the search or choose another network.</p>
          <button
            className="text-action"
            type="button"
            onClick={() => {
              setQuery("");
              setNetworkFilter("all");
            }}
          >
            Clear filters
          </button>
        </div>
      ) : (
        <div className="watchlist-layout">
          <ul className="watchlist-list" aria-label="Saved wallet addresses">
            {visibleEntries.map((entry) => {
              const isSelected = entry.address.id === selectedAddressId;
              return (
                <li key={entry.address.id}>
                  <button
                    className={`watchlist-row ${isSelected ? "is-selected" : ""}`}
                    type="button"
                    onClick={() => onSelectAddress(entry.address.id)}
                    aria-pressed={isSelected}
                  >
                    <span className="watchlist-row-network" aria-hidden="true">
                      {entry.networkName.slice(0, 1).toUpperCase()}
                    </span>
                    <span className="watchlist-row-copy">
                      <strong>
                        {entry.address.label || "Unlabeled address"}
                      </strong>
                      <code>{entry.address.address}</code>
                    </span>
                    <span className="watchlist-row-meta">
                      <span>{entry.networkName}</span>
                      <small>
                        {entry.transferCount === null
                          ? historyStatus === "error"
                            ? "History unavailable"
                            : "History not loaded"
                          : `${entry.transferCount} observations`}
                      </small>
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>

          <aside
            className="watchlist-detail"
            aria-labelledby="watchlist-detail-title"
          >
            {selectedEntry ? (
              <>
                <div className="watchlist-detail-network">
                  <span className="watchlist-row-network" aria-hidden="true">
                    {selectedEntry.networkName.slice(0, 1).toUpperCase()}
                  </span>
                  <span>{selectedEntry.networkName}</span>
                </div>
                <h2 id="watchlist-detail-title">
                  {selectedEntry.address.label || "Unlabeled address"}
                </h2>
                <code className="watchlist-detail-address">
                  {selectedEntry.address.address}
                </code>
                <div className="watchlist-detail-facts">
                  <div>
                    <span>Saved on</span>
                    <strong>
                      {formatWatchlistDate(selectedEntry.address.created_at)}
                    </strong>
                  </div>
                  <div>
                    <span>Latest observation</span>
                    <strong>
                      {historyStatus === "ready"
                        ? formatWatchlistDate(selectedEntry.latestObservedAt)
                        : historyStatus === "error"
                          ? "History unavailable"
                          : "History is still loading"}
                    </strong>
                  </div>
                  <div>
                    <span>Observed transfers</span>
                    <strong>
                      {selectedEntry.transferCount === null
                        ? "—"
                        : selectedEntry.transferCount.toLocaleString()}
                    </strong>
                  </div>
                </div>
                <div className="watchlist-detail-actions">
                  <button
                    className="button button-primary"
                    type="button"
                    onClick={() => onSyncAddress(selectedEntry.address)}
                    disabled={syncing || !canSync}
                  >
                    <RefreshCw
                      size={15}
                      className={syncing ? "is-spinning" : ""}
                    />
                    {syncing ? "Syncing history…" : "Sync history"}
                  </button>
                  <button
                    className="button button-secondary"
                    type="button"
                    onClick={() => onOpenAddress(selectedEntry.address.id)}
                  >
                    Open address <ArrowUpRight size={15} />
                  </button>
                </div>
                <p className="watchlist-detail-note">
                  {!apiAvailable
                    ? "The API is unavailable. Reconnect before syncing history."
                    : !selectedSyncConfigured
                      ? "History sync is not configured for this network. Saved observations remain available."
                      : "Sync reads public provider history. It does not monitor mempool activity."}
                </p>
              </>
            ) : (
              <div className="watchlist-detail-empty">
                <Clock3 size={18} aria-hidden="true" />
                <p>Select an address to view its saved history status.</p>
              </div>
            )}
          </aside>
        </div>
      )}
    </section>
  );
}
