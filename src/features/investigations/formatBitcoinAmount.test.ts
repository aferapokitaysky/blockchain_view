import { describe, expect, it } from "vitest";
import { formatBitcoinAmount } from "./formatBitcoinAmount";

describe("formatBitcoinAmount", () => {
  it("formats whole and fractional Bitcoin from exact satoshi values", () => {
    expect(formatBitcoinAmount("100000000")).toBe("1 BTC");
    expect(formatBitcoinAmount("123456789")).toBe("1.23456789 BTC");
    expect(formatBitcoinAmount("1")).toBe("0.00000001 BTC");
  });

  it("keeps large amounts exact and preserves invalid values as satoshis", () => {
    expect(formatBitcoinAmount("2100000000000000")).toBe("21000000 BTC");
    expect(formatBitcoinAmount("not-an-integer")).toBe("not-an-integer sat");
  });
});
