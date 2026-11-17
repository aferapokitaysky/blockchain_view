import { describe, expect, it } from "vitest";
import type { TrackedAddress, WalletHistory } from "../investigations/api";
import {
  getActiveInvestigationData,
  getDashboardSummary,
} from "./dashboardSummary";

const addresses: TrackedAddress[] = [
  {
    id: "tracked-1",
    investigation_id: "investigation-1",
    network: "ethereum",
    address: "0xaaa",
    label: null,
    created_at: "2026-01-01T00:00:00Z",
  },
  {
    id: "tracked-2",
    investigation_id: "investigation-1",
    network: "ethereum",
    address: "0xbbb",
    label: "Exchange",
    created_at: "2026-01-01T00:00:00Z",
  },
  {
    id: "tracked-3",
    investigation_id: "investigation-1",
    network: "bitcoin",
    address: "bc1qtest",
    label: null,
    created_at: "2026-01-01T00:00:00Z",
  },
];

const histories: WalletHistory[] = [
  {
    address: addresses[0],
    transfers: [
      {
        id: "transfer-1",
        tracked_address_id: "tracked-1",
        network: "ethereum",
        transaction_hash: "0xTxA",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "1500",
        asset: "USDC",
        block_height: 100,
        block_time: "2026-01-02T00:00:00Z",
        confirmed: true,
        provider: "Blockscout",
        observed_at: "2026-01-02T00:00:00Z",
        transfer_kind: "erc20",
        from_address: "0xaaa",
        to_address: "0xbbb",
        token_decimals: 2,
      },
      {
        id: "transfer-2",
        tracked_address_id: "tracked-1",
        network: "ethereum",
        transaction_hash: "0xTxB",
        direction: "inbound",
        received_amount: "25",
        spent_amount: "0",
        asset: "ETH",
        block_height: 101,
        block_time: "2026-01-03T00:00:00Z",
        confirmed: true,
        provider: "Blockscout",
        observed_at: "2026-01-03T00:00:00Z",
        transfer_kind: "evm_native",
        from_address: "0xccc",
        to_address: "0xaaa",
      },
    ],
  },
  {
    address: addresses[1],
    transfers: [
      {
        id: "transfer-3",
        tracked_address_id: "tracked-2",
        network: "ethereum",
        transaction_hash: "0xtxa",
        direction: "inbound",
        received_amount: "1500",
        spent_amount: "0",
        asset: "USDC",
        block_height: 100,
        block_time: "2026-01-02T00:00:00Z",
        confirmed: true,
        provider: "Blockscout",
        observed_at: "2026-01-02T00:00:00Z",
        transfer_kind: "erc20",
        from_address: "0xaaa",
        to_address: "0xbbb",
        token_decimals: 2,
      },
    ],
  },
];

describe("getDashboardSummary", () => {
  it("counts saved addresses, networks, unique transactions, and observed connections", () => {
    expect(getDashboardSummary(addresses, histories)).toEqual({
      trackedAddresses: 3,
      networks: 2,
      transactions: 2,
      connections: 2,
    });
  });

  it("returns an empty summary without inventing activity", () => {
    expect(getDashboardSummary([], [])).toEqual({
      trackedAddresses: 0,
      networks: 0,
      transactions: 0,
      connections: 0,
    });
  });

  it("includes relation-only transaction evidence", () => {
    const relationHistory: WalletHistory = {
      address: addresses[0],
      transfers: [],
      relationships: [
        {
          id: "relationship-1",
          tracked_address_id: "tracked-1",
          network: "sui",
          transaction_hash: "0xTxC",
          counterparty_address: "0x2",
          relationship_type: "same_transaction",
          participant_role: "input",
          block_height: 200,
          block_time: "2026-01-04T00:00:00Z",
          confirmed: true,
          provider: "Sui GraphQL RPC",
          source_host: "graphql.mainnet.sui.io",
          observed_at: "2026-01-04T00:00:00Z",
        },
      ],
    };

    expect(
      getDashboardSummary([addresses[0]], [relationHistory]).transactions,
    ).toBe(1);
  });
});

describe("getActiveInvestigationData", () => {
  it("keeps each graph scope within the selected investigation", () => {
    const otherAddress: TrackedAddress = {
      ...addresses[0],
      id: "tracked-other",
      investigation_id: "investigation-2",
      address: "0xother",
    };
    const otherHistory: WalletHistory = {
      address: otherAddress,
      transfers: [],
    };

    expect(
      getActiveInvestigationData(
        [...addresses, otherAddress],
        [...histories, otherHistory],
        "tracked-2",
      ),
    ).toEqual({
      investigationId: "investigation-1",
      addresses,
      histories,
    });
  });
});
