import { describe, expect, it } from "vitest";
import { scoreTransfer, type TransferRiskEvidence } from "./scoreTransfer";

function evidence(
  overrides: Partial<TransferRiskEvidence> = {},
): TransferRiskEvidence {
  return {
    id: "evidence-1",
    title: "Direct sanctions match",
    severity: "critical",
    scope: "direct",
    hops: 0,
    source: "OFAC SDN",
    reference: null,
    ...overrides,
  };
}

describe("transfer risk assessment", () => {
  it("returns a zero score for a transfer without evidence", () => {
    expect(scoreTransfer([])).toMatchObject({
      score: 0,
      level: "unknown",
      modelVersion: "0.1.0",
    });
  });

  it("reduces indirect exposure by observed hop distance", () => {
    const direct = scoreTransfer([evidence()]);
    const oneHop = scoreTransfer([evidence({ scope: "indirect", hops: 1 })]);
    const twoHops = scoreTransfer([evidence({ scope: "indirect", hops: 2 })]);

    expect(direct.score).toBe(80);
    expect(oneHop.score).toBe(40);
    expect(twoHops.score).toBe(27);
  });

  it("applies the stronger provisional weight to behavioral evidence", () => {
    const assessment = scoreTransfer([
      evidence({ scope: "behavioral", severity: "high" }),
    ]);

    expect(assessment.contributions[0]).toMatchObject({
      basePoints: 48,
      factor: 0.9,
      points: 43.2,
    });
    expect(assessment.score).toBe(43);
  });

  it("caps aggregate score and sorts strongest evidence first", () => {
    const assessment = scoreTransfer([
      evidence({ id: "behavior", scope: "behavioral", severity: "high" }),
      evidence({ id: "direct-2" }),
      evidence({ id: "direct-1" }),
    ]);

    expect(assessment.score).toBe(100);
    expect(assessment.level).toBe("high");
    expect(
      assessment.contributions.map(({ evidence: item }) => item.id),
    ).toEqual(["direct-1", "direct-2", "behavior"]);
    expect(assessment.disclaimer).toContain("does not establish wrongdoing");
  });

  it("bounds invalid indirect hop counts to the supported range", () => {
    const negative = scoreTransfer([evidence({ scope: "indirect", hops: -3 })]);
    const distant = scoreTransfer([evidence({ scope: "indirect", hops: 99 })]);

    expect(negative.score).toBe(80);
    expect(distant.score).toBe(7);
    expect(
      scoreTransfer([evidence({ scope: "indirect", hops: Number.NaN })]).score,
    ).toBe(7);
  });
});
