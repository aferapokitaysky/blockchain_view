import { describe, expect, it } from "vitest";
import { detectAddressNetwork } from "./detectAddressNetwork";

describe("detectAddressNetwork", () => {
  it("does not guess which EVM chain owns a valid address", async () => {
    const result = await detectAddressNetwork(
      "0x742d35Cc6634C0532925a3b844Bc454e4438f44e",
    );

    expect(result.status).toBe("ambiguous");
    if (result.status === "ambiguous") {
      expect(result.candidates).toEqual(["Ethereum", "Base", "Arbitrum"]);
    }
  });

  it("validates a Bitcoin Base58Check mainnet address", async () => {
    await expect(
      detectAddressNetwork("1BoatSLRHtKNngkdXEeobR76b53LETtpyT"),
    ).resolves.toMatchObject({
      status: "detected",
      network: "Bitcoin",
      confidence: "format-and-checksum",
    });
  });

  it("validates a Bitcoin Bech32 mainnet address", async () => {
    await expect(
      detectAddressNetwork("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"),
    ).resolves.toMatchObject({
      status: "detected",
      network: "Bitcoin",
      confidence: "format-and-checksum",
    });
  });

  it("rejects malformed Bitcoin addresses and unsupported testnet addresses", async () => {
    await expect(
      detectAddressNetwork("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t5"),
    ).resolves.toMatchObject({ status: "invalid" });
    await expect(
      detectAddressNetwork("tb1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh"),
    ).resolves.toMatchObject({ status: "unsupported" });
  });

  it("rejects malformed EVM addresses instead of assigning a chain", async () => {
    await expect(detectAddressNetwork("0x1234")).resolves.toMatchObject({
      status: "invalid",
    });
  });

  it("does not treat generic base58 text as a supported chain", async () => {
    await expect(
      detectAddressNetwork("So11111111111111111111111111111111111111112"),
    ).resolves.toMatchObject({ status: "unsupported" });
  });
});
