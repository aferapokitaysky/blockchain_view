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
});
