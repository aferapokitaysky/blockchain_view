import { describe, expect, it } from "vitest";
import type { ObservedTransfer } from "../investigations/api";
import { deriveBehaviorSignals } from "./behaviorSignals";
import { buildWalletAssessment } from "./scoreWallet";

function transfer(overrides: Partial<ObservedTransfer> = {}): ObservedTransfer {
  const id = overrides.id ?? "transfer-1";
  return {
    id,
    tracked_address_id: "wallet-1",
    network: "ethereum",
    transaction_hash: `0x${id}`,
    direction: "outbound",
    received_amount: "0",
    spent_amount: "1",
    asset: "ETH",
    block_height: 100,
    block_time: "2026-11-16T10:00:00Z",
    confirmed: true,
    provider: "Blockscout",
    observed_at: "2026-11-16T10:01:00Z",
    from_address: "0xwallet",
    to_address: "0xcounterparty",
    ...overrides,
  };
}

describe("derived transaction behavior signals", () => {
  it("reports distinct confirmed counterparties in one rolling 24-hour window", () => {
    const signals = deriveBehaviorSignals(
      ["0xa", "0xb", "0xc", "0xd"].map((address, index) =>
        transfer({
          id: `transfer-${index}`,
          to_address: address,
          block_time: `2026-11-16T${String(8 + index).padStart(2, "0")}:00:00Z`,
        }),
      ),
    );

    expect(signals).toMatchObject([
      {
        title: "Observed fan-out pattern",
        category: "behavior",
        severity: "medium",
        hops: 0,
        source: "Saved chain observations · Blockscout",
      },
    ]);
    expect(signals[0].detail).toContain("4 distinct outbound counterparties");
    expect(signals[0].detail).toContain(
      "from 2026-11-16T08:00:00.000Z to 2026-11-16T11:00:00.000Z (UTC)",
    );
    expect(signals[0].detail).toContain(
      "does not establish common ownership or intent",
    );
  });

  it("requires four distinct counterparties and ignores unconfirmed or malformed observations", () => {
    const signals = deriveBehaviorSignals([
      transfer({ id: "one", to_address: "0xa" }),
      transfer({ id: "two", to_address: "0xa" }),
      transfer({ id: "three", to_address: "0xb" }),
      transfer({ id: "four", to_address: "0xc" }),
      transfer({ id: "five", to_address: "0xd", confirmed: false }),
      transfer({ id: "six", to_address: "0xe", block_time: "invalid" }),
    ]);

    expect(signals).toEqual([]);
  });

  it("only counts events that fit inside a rolling 24-hour period", () => {
    const signals = deriveBehaviorSignals(
      ["0xa", "0xb", "0xc", "0xd"].map((address, index) =>
        transfer({
          id: `transfer-${index}`,
          to_address: address,
          block_time: `2026-11-${index < 2 ? "15" : "16"}T${index % 2 === 0 ? "01" : "02"}:00:00Z`,
        }),
      ),
    );

    expect(signals).toEqual([]);
  });

  it("derives inbound patterns and applies the existing behavioral factor", () => {
    const transfers = ["0xa", "0xb", "0xc", "0xd"].map((address, index) =>
      transfer({
        id: `inbound-${index}`,
        direction: "inbound",
        spent_amount: "0",
        received_amount: "1",
        from_address: address,
        to_address: "0xwallet",
      }),
    );
    const assessment = buildWalletAssessment([], null, transfers);

    expect(assessment.contributions).toHaveLength(1);
    expect(assessment.contributions[0]).toMatchObject({
      baseWeight: 24,
      distanceFactor: 1,
      categoryFactor: 0.7,
      contribution: 16.8,
    });
    expect(assessment.contributions[0].signal.title).toBe(
      "Observed fan-in pattern",
    );
  });

  it("reports repeated same-network, same-asset inbound/outbound pairs with exact evidence", () => {
    const signals = deriveBehaviorSignals([
      transfer({
        id: "inbound-a",
        direction: "inbound",
        received_amount: "1000000000000000000000000000000",
        spent_amount: "0",
        asset: "USDC",
        transfer_kind: "erc20",
        token_decimals: 6,
        token_contract_address: "0x1111111111111111111111111111111111111111",
        block_time: "2026-11-16T10:00:00Z",
        from_address: "0xsource-a",
      }),
      transfer({
        id: "outbound-a",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "1300000000000000000000000000000",
        asset: "USDC",
        transfer_kind: "erc20",
        token_decimals: 6,
        token_contract_address: "0x1111111111111111111111111111111111111111",
        block_time: "2026-11-16T10:10:00Z",
        to_address: "0xdestination-a",
      }),
      transfer({
        id: "inbound-b",
        direction: "inbound",
        received_amount: "1000000000000000000000000000000",
        spent_amount: "0",
        asset: "USDC",
        transfer_kind: "erc20",
        token_decimals: 6,
        token_contract_address: "0x1111111111111111111111111111111111111111",
        block_time: "2026-11-16T11:00:00Z",
        from_address: "0xsource-b",
      }),
      transfer({
        id: "outbound-b",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "700000000000000000000000000000",
        asset: "USDC",
        transfer_kind: "erc20",
        token_decimals: 6,
        token_contract_address: "0x1111111111111111111111111111111111111111",
        block_time: "2026-11-16T11:05:00Z",
        to_address: "0xdestination-b",
      }),
    ]);

    expect(signals).toHaveLength(1);
    expect(signals[0]).toMatchObject({
      title: "Repeated rapid inbound/outbound sequence",
      category: "behavior",
      severity: "low",
      source: "Saved chain observations · Blockscout",
      observedAt: "2026-11-16T11:05:00.000Z",
    });
    expect(signals[0].detail).toContain("2 non-overlapping");
    expect(signals[0].detail).toContain("70–130%");
    expect(signals[0].detail).toContain("from Blockscout");
    expect(signals[0].detail).toContain("0xoutbound-a");
    expect(signals[0].detail).toContain("0xinbound-b");
    expect(signals[0].detail).toContain("raw 1000000000000000000000000000000");
    expect(signals[0].detail).toContain(
      "do not prove the same funds continued or establish intent",
    );
  });

  it("does not combine different networks or assets to reach the repeat threshold", () => {
    const signals = deriveBehaviorSignals([
      transfer({
        id: "eth-in",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T10:00:00Z",
      }),
      transfer({
        id: "eth-out",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-16T10:01:00Z",
      }),
      transfer({
        id: "usdc-in",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        asset: "USDC",
        block_time: "2026-11-16T10:02:00Z",
      }),
      transfer({
        id: "usdc-out",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        asset: "USDC",
        block_time: "2026-11-16T10:03:00Z",
      }),
      transfer({
        id: "base-in",
        network: "base",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T10:04:00Z",
      }),
      transfer({
        id: "base-out",
        network: "base",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-16T10:05:00Z",
      }),
    ]);

    expect(signals.some(({ title }) => title.includes("rapid"))).toBe(false);
  });

  it("requires inbound before outbound within the inclusive ten-minute window", () => {
    const exactBoundary = [
      transfer({
        id: "in-1",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T10:00:00Z",
      }),
      transfer({
        id: "out-1",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-16T10:10:00Z",
      }),
      transfer({
        id: "in-2",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T11:00:00Z",
      }),
      transfer({
        id: "out-2",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-16T11:10:00Z",
      }),
    ];
    expect(
      deriveBehaviorSignals(exactBoundary).some(({ title }) =>
        title.includes("rapid"),
      ),
    ).toBe(true);

    const reversed = [
      transfer({
        id: "out-early-1",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-16T10:00:00Z",
      }),
      transfer({
        id: "in-late-1",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T10:01:00Z",
      }),
      transfer({
        id: "out-early-2",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-16T11:00:00Z",
      }),
      transfer({
        id: "in-late-2",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T11:01:00Z",
      }),
    ];
    expect(
      deriveBehaviorSignals(reversed).some(({ title }) =>
        title.includes("rapid"),
      ),
    ).toBe(false);

    const tooLate = exactBoundary.map((item) =>
      item.id === "out-1" || item.id === "out-2"
        ? {
            ...item,
            block_time: new Date(
              Date.parse(item.block_time!) + 1,
            ).toISOString(),
          }
        : item,
    );
    expect(
      deriveBehaviorSignals(tooLate).some(({ title }) =>
        title.includes("rapid"),
      ),
    ).toBe(false);
  });

  it("rejects amounts outside the ratio band and legs from the same transaction", () => {
    const signals = deriveBehaviorSignals([
      transfer({
        id: "in-69",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T10:00:00Z",
      }),
      transfer({
        id: "out-69",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "69",
        block_time: "2026-11-16T10:01:00Z",
      }),
      transfer({
        id: "in-131",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T11:00:00Z",
      }),
      transfer({
        id: "out-131",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "131",
        block_time: "2026-11-16T11:01:00Z",
      }),
      transfer({
        id: "in-same-tx-a",
        transaction_hash: "same-signature-a",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T12:00:00Z",
      }),
      transfer({
        id: "out-same-tx-a",
        transaction_hash: "same-signature-a",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-16T12:01:00Z",
      }),
      transfer({
        id: "in-same-tx-b",
        transaction_hash: "same-signature-b",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T13:00:00Z",
      }),
      transfer({
        id: "out-same-tx-b",
        transaction_hash: "same-signature-b",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-16T13:01:00Z",
      }),
    ]);

    expect(signals.some(({ title }) => title.includes("rapid"))).toBe(false);
  });

  it("ignores unconfirmed events and missing on-chain timestamps for rapid signals", () => {
    const signals = deriveBehaviorSignals([
      transfer({
        id: "in-1",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T10:00:00Z",
      }),
      transfer({
        id: "out-1",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-16T10:01:00Z",
      }),
      transfer({
        id: "in-2",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T11:00:00Z",
        confirmed: false,
      }),
      transfer({
        id: "out-2",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: null,
        observed_at: "2026-11-16T11:01:00Z",
      }),
    ]);

    expect(signals.some(({ title }) => title.includes("rapid"))).toBe(false);
  });

  it("does not combine pairs that fall outside the same rolling 24-hour window", () => {
    const signals = deriveBehaviorSignals([
      transfer({
        id: "old-in",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-14T10:00:00Z",
      }),
      transfer({
        id: "old-out",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-14T10:01:00Z",
      }),
      transfer({
        id: "recent-in",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T10:00:00Z",
      }),
      transfer({
        id: "recent-out",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-16T10:01:00Z",
      }),
    ]);

    expect(signals.some(({ title }) => title.includes("rapid"))).toBe(false);
  });

  it("pairs each event at most once, ignores duplicates, and caps displayed examples", () => {
    const transfers = [
      transfer({
        id: "in-a",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T10:00:00Z",
      }),
      transfer({
        id: "in-b",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T10:02:00Z",
      }),
      transfer({
        id: "out-a",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-16T10:03:00Z",
      }),
      transfer({
        id: "out-b",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-16T10:04:00Z",
      }),
      transfer({
        id: "in-c",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T11:00:00Z",
      }),
      transfer({
        id: "out-c",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-16T11:01:00Z",
      }),
      transfer({
        id: "in-d",
        direction: "inbound",
        received_amount: "100",
        spent_amount: "0",
        block_time: "2026-11-16T12:00:00Z",
      }),
      transfer({
        id: "out-d",
        direction: "outbound",
        received_amount: "0",
        spent_amount: "100",
        block_time: "2026-11-16T12:01:00Z",
      }),
    ];
    const signals = deriveBehaviorSignals([
      ...transfers,
      transfers[0],
      transfers[0],
      transfers[2],
    ]).filter(({ title }) => title.includes("rapid"));

    expect(signals).toHaveLength(1);
    expect(signals[0].detail).toContain("4 non-overlapping");
    expect(signals[0].detail).toContain("showing 3 of 4 pairs");
    expect(signals[0].severity).toBe("medium");
  });
});
