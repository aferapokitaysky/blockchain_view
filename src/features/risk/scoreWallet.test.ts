import { describe, expect, it } from "vitest";
import {
  buildWalletAssessment,
  mapOfacMatches,
  mapStoredRiskSignal,
  scoreWallet,
} from "./scoreWallet";
import type {
  AddressScreeningResult,
  StoredRiskSignal,
} from "../investigations/api";

function storedSignal(
  overrides: Partial<StoredRiskSignal> = {},
): StoredRiskSignal {
  return {
    id: "signal-1",
    tracked_address_id: "wallet-1",
    signal_key: "indirect_service_exposure",
    category: "exposure",
    severity: "high",
    source_name: "Analyst review",
    source_reference: "https://example.test/source/1",
    observed_at: "2026-11-16T10:00:00Z",
    details: { summary: "Documented service relationship", hops: 1 },
    created_at: "2026-11-16T10:05:00Z",
    ...overrides,
  };
}

function screening(
  status: AddressScreeningResult["status"],
): AddressScreeningResult {
  return {
    network: "ethereum",
    status,
    match_type: "exact_address",
    source:
      status === "exact_match" || status === "no_exact_match"
        ? {
            source_name: "OFAC SDN",
            published_at: "11/12/2026",
            imported_at: "2026-11-16T10:00:00Z",
            address_count: 10,
          }
        : null,
    matches:
      status === "exact_match"
        ? [
            {
              entity_uid: "123",
              entity_name: "Example Entity",
              programs: ["TEST"],
              currency_code: "ETH",
              address: "0xabc",
            },
          ]
        : [],
    explanation: "Test response",
  };
}

describe("wallet risk assessment", () => {
  it("applies direct, one-hop, two-hop, and behavioral weights transparently", () => {
    const assessment = scoreWallet([
      {
        id: "direct",
        title: "Direct sanctions entry",
        detail: "Exact match",
        category: "sanctions",
        severity: "high",
        hops: 0,
        source: "OFAC SDN",
        sourceReference: null,
        observedAt: "2026-11-16T10:00:00Z",
      },
      {
        id: "one-hop",
        title: "One hop",
        detail: "Attributed counterpart",
        category: "exposure",
        severity: "high",
        hops: 1,
        source: "Analyst review",
        sourceReference: null,
        observedAt: "2026-11-16T10:00:00Z",
      },
      {
        id: "two-hop-behavior",
        title: "Two hop pattern",
        detail: "Observed behavior",
        category: "behavior",
        severity: "high",
        hops: 2,
        source: "Chain history",
        sourceReference: null,
        observedAt: "2026-11-16T10:00:00Z",
      },
    ]);

    expect(
      assessment.contributions.map(({ contribution }) => contribution),
    ).toEqual([48, 13.2, 11.2]);
    expect(assessment.score).toBe(72);
    expect(assessment.modelVersion).toBe("0.2.0");
  });

  it("maps only exact OFAC matches into a direct sanctions signal", () => {
    expect(mapOfacMatches(screening("exact_match"))).toMatchObject([
      {
        title: "Example Entity",
        category: "sanctions",
        severity: "critical",
        hops: 0,
        source: "OFAC SDN · published 11/12/2026",
      },
    ]);
    expect(mapOfacMatches(screening("no_exact_match"))).toEqual([]);
    expect(mapOfacMatches(screening("source_not_imported"))).toEqual([]);
    expect(mapOfacMatches(screening("network_not_covered"))).toEqual([]);
  });

  it("does not lower the score because an OFAC snapshot has no exact match", () => {
    const signal = storedSignal();
    const withoutScreening = buildWalletAssessment([signal], null);
    const withNonMatch = buildWalletAssessment(
      [signal],
      screening("no_exact_match"),
    );

    expect(withNonMatch.score).toBe(withoutScreening.score);
    expect(withNonMatch.contributions).toHaveLength(1);
  });

  it("keeps provenance and ignores malformed hop values instead of amplifying them", () => {
    const mapped = mapStoredRiskSignal(
      storedSignal({
        source_reference: null,
        details: { hops: "2", note: "Review note" },
      }),
    );

    expect(mapped).toMatchObject({
      title: "Indirect Service Exposure",
      detail: "Review note",
      hops: 0,
      source: "Analyst review",
      sourceReference: null,
    });
  });

  it("caps accumulated contributions at 100 and gives informational evidence no weight", () => {
    const criticalSignal = {
      ...mapStoredRiskSignal(storedSignal()),
      category: "sanctions" as const,
      severity: "critical" as const,
      hops: 0,
    };
    const assessment = scoreWallet([
      criticalSignal,
      { ...criticalSignal, id: "another" },
      { ...criticalSignal, id: "info", severity: "informational" },
    ]);

    expect(assessment.score).toBe(100);
    expect(assessment.contributions[2].contribution).toBe(0);
  });
});
