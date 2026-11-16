import { describe, expect, it } from "vitest";
import type { ObservedTransfer, TrackedAddress, WalletHistory } from "./api";
import { buildTransactionFeed, formatFeedTime } from "./transactionFeed";

const address: TrackedAddress = {
  id: "tracked-1",
  investigation_id: "investigation-1",
  network: "ethereum",
  address: "0x0123456789abcdef0123456789abcdef01234567",
  label: "Treasury",
  created_at: "2026-11-16T10:00:00Z",
};

function transfer(overrides: Partial<ObservedTransfer> = {}): ObservedTransfer {
  return {
    id: "observation-1",
    tracked_address_id: address.id,
    network: "ethereum",
    transaction_hash: "0xabcdef0123456789abcdef0123456789",
    direction: "outbound",
    received_amount: "0",
    spent_amount: "1250000000000000000",
    asset: "ETH",
    block_height: 100,
    block_time: "2026-11-16T11:00:00Z",
    confirmed: true,
    provider: "blockscout",
    observed_at: "2026-11-16T11:01:00Z",
    transfer_kind: "evm_native",
    from_address: address.address,
    to_address: "0xabcdef0123456789abcdef0123456789abcdef01",
    ...overrides,
  };
}

function history(
  transfers: ObservedTransfer[],
  trackedAddress = address,
): WalletHistory {
  return { address: trackedAddress, transfers };
}

describe("transaction feed", () => {
  it("preserves observed EVM endpoints, tracked wallet, and provider provenance", () => {
    const [item] = buildTransactionFeed([history([transfer()])]);

    expect(item).toMatchObject({
      network: "Ethereum",
      direction: "outbound",
      from: "0x012345…234567",
      to: "0xabcdef…cdef01",
      amount: "−1.25",
      asset: "ETH",
      provider: "blockscout",
      trackedWallet: "Treasury",
    });
  });

  it("does not invent Bitcoin counterparties and includes both input and output totals", () => {
    const bitcoinAddress: TrackedAddress = {
      ...address,
      network: "bitcoin",
      address: "bc1qexampleaddress",
      label: null,
    };
    const [item] = buildTransactionFeed([
      history(
        [
          transfer({
            network: "bitcoin",
            direction: "both",
            received_amount: "50000000",
            spent_amount: "80000000",
            asset: "BTC",
            transfer_kind: "bitcoin",
            from_address: null,
            to_address: null,
          }),
        ],
        bitcoinAddress,
      ),
    ]);

    expect(item.from).toBe("Bitcoin transaction");
    expect(item.to).toBe("Tracked wallet activity");
    expect(item.trackedWallet).toBe("bc1qexampleaddress");
    expect(item.amount).toBe("+0.5 BTC in · −0.8 BTC out");
  });

  it("sorts newest observations first and skips records from unknown networks", () => {
    const items = buildTransactionFeed([
      history([
        transfer({ id: "older", block_time: "2026-11-15T11:00:00Z" }),
        transfer({ id: "unsupported", network: "solana" }),
        transfer({ id: "newer", block_time: "2026-11-16T11:00:00Z" }),
      ]),
    ]);

    expect(items.map(({ id }) => id)).toEqual([
      "newer:transaction",
      "older:transaction",
    ]);
  });

  it("formats valid timestamps and handles missing provider time", () => {
    expect(formatFeedTime("invalid")).toBe("Time unavailable");
    expect(formatFeedTime("2026-11-16T11:00:00Z")).not.toBe("Time unavailable");
  });
});
