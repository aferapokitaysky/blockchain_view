import { describe, expect, it } from "vitest";
import { formatAssetAmount, formatObservedAmount } from "./formatAssetAmount";

describe("formatAssetAmount", () => {
  it("formats large token amounts without floating-point rounding", () => {
    expect(formatAssetAmount("123456789012345678901", 6)).toBe(
      "123,456,789,012,345.678901",
    );
  });

  it("removes trailing fractional zeroes and handles whole units", () => {
    expect(formatAssetAmount("120000000", 6)).toBe("120");
    expect(formatAssetAmount("42", 0)).toBe("42");
  });

  it("does not assume a token precision when the provider omitted decimals", () => {
    expect(
      formatObservedAmount(
        { network: "base", transfer_kind: "erc20", token_decimals: null },
        "123456789",
      ),
    ).toBe("123,456,789 raw units");
  });
});
