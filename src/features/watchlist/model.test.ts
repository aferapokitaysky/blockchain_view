import { describe, expect, it } from "vitest";
import type { TrackedAddress, WalletHistory } from "../investigations/api";
import {
  buildWatchlistEntries,
  filterWatchlistEntries,
  formatWatchlistDate,
} from "./model";

const address: TrackedAddress = {
  id: "address-1",
  investigation_id: "case-1",
  network: "eip155:137",
  address: "0xabcdef0123456789abcdef0123456789abcdef01",
  label: "Treasury",
  created_at: "2026-11-17T10:00:00Z",
};

describe("watchlist model", () => {
  it("builds history summaries without claiming history is loaded", () => {
    const histories: WalletHistory[] = [
      {
        address,
        transfers: [
          {
            id: "transfer-1",
            tracked_address_id: address.id,
            network: address.network,
            transaction_hash: "0x1",
            direction: "inbound",
            received_amount: "100",
            spent_amount: "0",
            asset: "USDC",
            block_height: 10,
            block_time: "2026-11-17T12:00:00Z",
            confirmed: true,
            provider: "Blockscout",
            observed_at: "2026-11-17T12:01:00Z",
          },
        ],
      },
    ];

    expect(
      buildWatchlistEntries(
        [address],
        histories,
        { [address.network]: "Polygon PoS" },
        false,
      ),
    ).toEqual([
      {
        address,
        networkName: "Polygon PoS",
        transferCount: null,
        latestObservedAt: "2026-11-17T12:00:00Z",
      },
    ]);
    expect(
      buildWatchlistEntries([address], histories, {}, true)[0].transferCount,
    ).toBe(1);
  });

  it("filters case-insensitively by label, address, and network", () => {
    const entries = buildWatchlistEntries(
      [address],
      [],
      { [address.network]: "Polygon PoS" },
      true,
    );

    expect(
      filterWatchlistEntries(entries, { query: "trea", network: "all" }),
    ).toHaveLength(1);
    expect(
      filterWatchlistEntries(entries, { query: "0xABCDEF", network: "all" }),
    ).toHaveLength(1);
    expect(
      filterWatchlistEntries(entries, { query: "polygon", network: "all" }),
    ).toHaveLength(1);
    expect(
      filterWatchlistEntries(entries, { query: "trea", network: "ethereum" }),
    ).toHaveLength(0);
  });

  it("formats missing and invalid timestamps honestly", () => {
    expect(formatWatchlistDate(null)).toBe("No observations loaded");
    expect(formatWatchlistDate("invalid-date")).toBe(
      "Observation time unavailable",
    );
  });
});
