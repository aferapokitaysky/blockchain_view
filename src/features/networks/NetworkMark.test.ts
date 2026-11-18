import { describe, expect, it } from "vitest";
import { getEvmMarkId, getNetworkMarkKind } from "./networkMarkModel";

describe("network mark selection", () => {
  it.each([
    ["bitcoin", "bitcoin"],
    ["ethereum", "ethereum"],
    ["base", "base"],
    ["arbitrum", "arbitrum"],
    ["eip155:137", "polygon"],
    ["eip155:56", "bnb"],
    ["eip155:10", "optimism"],
    ["solana", "solana"],
    ["tron", "tron"],
    ["xrpl", "xrpl"],
    ["ton", "ton"],
    ["cardano", "cardano"],
    ["aptos", "aptos"],
    ["sui", "sui"],
  ])("maps %s to its own mark", (networkId, expectedKind) => {
    expect(getNetworkMarkKind(networkId)).toBe(expectedKind);
  });

  it("uses distinct stable IDs for different EVM chain IDs", () => {
    const first = getEvmMarkId("eip155:8453");
    const second = getEvmMarkId("eip155:42161");

    expect(first).toBe("evm-8453");
    expect(second).toBe("evm-42161");
    expect(first).not.toBe(second);
    expect(getEvmMarkId("eip155:8453")).toBe(first);
  });

  it("uses chain IDs from catalog entries for EVM aliases", () => {
    expect(getNetworkMarkKind("mantle", 5000)).toBe("evm");
    expect(getEvmMarkId("mantle", 5000)).toBe("evm-5000");
  });
});
