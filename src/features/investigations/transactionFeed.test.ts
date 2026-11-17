import { describe, expect, it } from "vitest";
import type {
  AddressScreeningResult,
  ObservedTransfer,
  StoredRiskSignal,
  TrackedAddress,
  WalletHistory,
} from "./api";
import {
  buildTransactionFeed,
  formatFeedTime,
  getTransferRiskPlaceholder,
  type TransactionRiskContext,
} from "./transactionFeed";

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

function riskSignal(
  details: Record<string, unknown>,
  overrides: Partial<StoredRiskSignal> = {},
): StoredRiskSignal {
  return {
    id: "risk-1",
    tracked_address_id: address.id,
    signal_key: "direct_service_exposure",
    category: "exposure",
    severity: "high",
    source_name: "Analyst source",
    source_reference: "https://example.test/evidence",
    observed_at: "2026-11-16T11:00:00Z",
    details,
    created_at: "2026-11-16T11:01:00Z",
    ...overrides,
  };
}

function riskContext(
  signals: StoredRiskSignal[],
  screening: AddressScreeningResult | null = null,
): TransactionRiskContext {
  return { trackedAddressId: address.id, signals, screening };
}

describe("transaction feed", () => {
  it("distinguishes missing transaction evidence from unavailable or unselected data", () => {
    expect(
      getTransferRiskPlaceholder(address.id, address.id, true, ""),
    ).toEqual({ label: "Loading evidence", error: null });
    expect(
      getTransferRiskPlaceholder(address.id, address.id, false, "API offline"),
    ).toEqual({ label: "Evidence unavailable", error: "API offline" });
    expect(
      getTransferRiskPlaceholder(address.id, "another-address", false, ""),
    ).toEqual({ label: "Select wallet for score", error: null });
    expect(
      getTransferRiskPlaceholder(address.id, address.id, false, ""),
    ).toEqual({ label: "No linked evidence", error: null });
  });

  it("scores only analyst evidence explicitly linked to the transaction hash", () => {
    const [item] = buildTransactionFeed(
      [history([transfer()])],
      {},
      riskContext([
        riskSignal({ transaction_hash: "0xABCDEF0123456789ABCDEF0123456789" }),
        riskSignal({ transaction_hash: "0xother" }, { id: "risk-2" }),
      ]),
    );

    expect(item.riskAssessment).toMatchObject({
      score: 48,
      level: "elevated",
    });
    expect(item.riskAssessment?.contributions).toHaveLength(1);
  });

  it("keeps case-sensitive Solana signatures when matching analyst evidence", () => {
    const solanaAddress = { ...address, network: "solana" };
    const solanaTransfer = transfer({
      network: "solana",
      transaction_hash: "5KxQmTestSignature",
      from_address: "8QTestSender",
      to_address: "7BTestRecipient",
    });
    const [item] = buildTransactionFeed(
      [history([solanaTransfer], solanaAddress)],
      { solana: "Solana" },
      {
        trackedAddressId: solanaAddress.id,
        signals: [
          riskSignal(
            { transaction_hash: "5kXQmTestSignature" },
            {
              tracked_address_id: solanaAddress.id,
            },
          ),
        ],
        screening: null,
      },
    );

    expect(item.riskAssessment).toBeNull();
  });

  it("adds direct OFAC evidence only when a matched address participates in the transfer", () => {
    const screening: AddressScreeningResult = {
      network: "ethereum",
      status: "exact_match",
      match_type: "exact_address",
      source: {
        source_name: "OFAC SDN",
        published_at: "2026-11-10T00:00:00Z",
        imported_at: "2026-11-11T00:00:00Z",
        address_count: 1,
      },
      matches: [
        {
          entity_uid: "entity-1",
          entity_name: "Listed entity",
          programs: ["TEST"],
          currency_code: "ETH",
          address: "0xabcdef0123456789abcdef0123456789abcdef01",
        },
      ],
      explanation: "Exact address match.",
    };
    const [item] = buildTransactionFeed(
      [history([transfer()])],
      {},
      riskContext([], screening),
    );

    expect(item.riskAssessment).toMatchObject({
      score: 80,
      level: "high",
    });
    expect(item.riskAssessment?.contributions[0].evidence.source).toContain(
      "OFAC SDN",
    );
  });

  it("preserves case-sensitive matching for Bitcoin Base58 addresses", () => {
    const screening: AddressScreeningResult = {
      network: "bitcoin",
      status: "exact_match",
      match_type: "exact_address",
      source: {
        source_name: "OFAC SDN",
        published_at: "2026-11-10T00:00:00Z",
        imported_at: "2026-11-11T00:00:00Z",
        address_count: 1,
      },
      matches: [
        {
          entity_uid: "entity-1",
          entity_name: "Listed entity",
          programs: ["TEST"],
          currency_code: "BTC",
          address: "1abcDEF0123456789",
        },
      ],
      explanation: "Exact address match.",
    };
    const bitcoinTransfer = transfer({
      network: "bitcoin",
      transfer_kind: "bitcoin",
      from_address: "1AbcDEF0123456789",
      to_address: "1RecipientAddress",
    });
    const [item] = buildTransactionFeed(
      [history([bitcoinTransfer])],
      {},
      riskContext([], screening),
    );

    expect(item.riskAssessment).toBeNull();
  });

  it("does not present unlinked observations as a low transaction score", () => {
    const [item] = buildTransactionFeed(
      [history([transfer()])],
      {},
      riskContext([riskSignal({ summary: "Wallet-level observation" })]),
    );

    expect(item.riskAssessment).toBeNull();
  });

  it("links fan-out evidence only to the confirmed transfers in that pattern", () => {
    const transfers = ["0xa", "0xb", "0xc", "0xd"].map((counterparty, index) =>
      transfer({
        id: `fan-out-${index}`,
        transaction_hash: `0xfan${index}`,
        to_address: counterparty,
      }),
    );
    const otherWallet = { ...address, id: "tracked-2" };
    const items = buildTransactionFeed(
      [
        history(transfers),
        history([transfer({ id: "other-wallet" })], otherWallet),
      ],
      {},
      riskContext([]),
    );

    expect(items.slice(0, 4)).toHaveLength(4);
    for (const item of items.slice(0, 4)) {
      expect(item.riskAssessment).toMatchObject({
        score: 22,
        level: "guarded",
        modelVersion: "0.2.0",
      });
      expect(item.riskAssessment?.contributions[0]).toMatchObject({
        factor: 0.9,
        evidence: {
          scope: "behavioral",
          title: "Observed fan-out pattern",
        },
      });
    }
    expect(
      items.find(({ trackedAddressId }) => trackedAddressId === "tracked-2")
        ?.riskAssessment,
    ).toBeNull();
  });

  it("links rapid-turnover evidence to displayed pair legs, not nearby unrelated transfers", () => {
    const transfers = [
      transfer({
        id: "pair-in-1",
        transaction_hash: "0xpair-in-1",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        from_address: "0xsource-a",
        to_address: address.address,
        block_time: "2026-11-16T10:00:00Z",
      }),
      transfer({
        id: "pair-out-1",
        transaction_hash: "0xpair-out-1",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "90",
        from_address: address.address,
        to_address: "0xdestination-a",
        block_time: "2026-11-16T10:05:00Z",
      }),
      transfer({
        id: "pair-in-2",
        transaction_hash: "0xpair-in-2",
        direction: "inbound",
        received_amount: "200",
        spent_amount: "0",
        from_address: "0xsource-b",
        to_address: address.address,
        block_time: "2026-11-16T11:00:00Z",
      }),
      transfer({
        id: "pair-out-2",
        transaction_hash: "0xpair-out-2",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "180",
        from_address: address.address,
        to_address: "0xdestination-b",
        block_time: "2026-11-16T11:05:00Z",
      }),
      transfer({
        id: "unpaired-outbound",
        transaction_hash: "0xunpaired-outbound",
        received_amount: "0",
        spent_amount: "999",
        block_time: "2026-11-16T11:06:00Z",
      }),
    ];
    const items = buildTransactionFeed(
      [history(transfers)],
      {},
      riskContext([]),
    );

    for (const id of ["pair-in-1", "pair-out-1", "pair-in-2", "pair-out-2"]) {
      const item = items.find((candidate) => candidate.id.startsWith(id));
      expect(item?.riskAssessment).toMatchObject({
        score: 7,
        level: "low",
        modelVersion: "0.2.0",
      });
      expect(item?.riskAssessment?.contributions[0].evidence.title).toBe(
        "Repeated rapid inbound/outbound sequence",
      );
    }
    expect(
      items.find((item) => item.id.startsWith("unpaired-outbound"))
        ?.riskAssessment,
    ).toBeNull();
  });

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

  it("keeps catalog network labels for new EVM chains in the feed", () => {
    const opAddress: TrackedAddress = {
      ...address,
      network: "eip155:10",
    };
    const [item] = buildTransactionFeed(
      [
        history(
          [
            transfer({
              network: "eip155:10",
              asset: "ETH",
            }),
          ],
          opAddress,
        ),
      ],
      { "eip155:10": "OP Mainnet" },
    );

    expect(item.network).toBe("OP Mainnet");
  });

  it("shortens Solana SPL mint labels without changing raw amount precision", () => {
    const [item] = buildTransactionFeed([
      history([
        transfer({
          network: "solana",
          transfer_kind: "solana_spl",
          asset: "So11111111111111111111111111111111111111112",
          token_contract_address: "So11111111111111111111111111111111111111112",
          token_decimals: 9,
          spent_amount: "1250000000",
        }),
      ]),
    ]);

    expect(item.network).toBe("Solana");
    expect(item.asset).toBe("SPL So111111…111112");
    expect(item.amount).toBe("−1.25");
  });

  it("formats native SOL using its nine decimal places", () => {
    const [item] = buildTransactionFeed([
      history([
        transfer({
          network: "solana",
          transfer_kind: "solana_native",
          asset: "SOL",
          token_decimals: 9,
          spent_amount: "1250000000",
        }),
      ]),
    ]);

    expect(item.amount).toBe("−1.25");
    expect(item.asset).toBe("SOL");
  });

  it("sorts observations and includes Solana records from the supported catalog", () => {
    const items = buildTransactionFeed([
      history([
        transfer({ id: "older", block_time: "2026-11-15T11:00:00Z" }),
        transfer({
          id: "solana",
          network: "solana",
          block_time: "2026-11-16T10:00:00Z",
        }),
        transfer({ id: "newer", block_time: "2026-11-16T11:00:00Z" }),
      ]),
    ]);

    expect(items.map(({ id }) => id)).toEqual([
      "newer:transaction",
      "solana:transaction",
      "older:transaction",
    ]);
    expect(items[1].network).toBe("Solana");
  });

  it("formats valid timestamps and handles missing provider time", () => {
    expect(formatFeedTime("invalid")).toBe("Time unavailable");
    expect(formatFeedTime("2026-11-16T11:00:00Z")).not.toBe("Time unavailable");
  });
});
