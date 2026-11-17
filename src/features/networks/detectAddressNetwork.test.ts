import { describe, expect, it } from "vitest";
import { detectAddressNetwork } from "./detectAddressNetwork";

describe("detectAddressNetwork", () => {
  it("asks users to choose Aptos or Sui for Move-compatible hex addresses", async () => {
    await expect(
      detectAddressNetwork(`0x${"ab".repeat(32)}`),
    ).resolves.toMatchObject({
      status: "ambiguous",
      family: "Move-compatible",
    });
    await expect(detectAddressNetwork("0x1")).resolves.toMatchObject({
      status: "ambiguous",
      family: "Move-compatible",
    });
  });

  it("keeps 20-byte hex addresses selectable across EVM, Aptos, and Sui", async () => {
    await expect(
      detectAddressNetwork(`0x${"ab".repeat(20)}`),
    ).resolves.toMatchObject({
      status: "ambiguous",
      family: "Hex-compatible",
    });
  });
  it("recognizes TON raw addresses and rejects malformed checksum-friendly candidates", async () => {
    await expect(
      detectAddressNetwork(`0:${"ab".repeat(32)}`),
    ).resolves.toMatchObject({
      status: "detected",
      network: "ton",
      confidence: "format-and-checksum",
    });
    await expect(detectAddressNetwork("A".repeat(48))).resolves.toMatchObject({
      status: "invalid",
    });
  });
  it("validates Shelley Cardano mainnet payment addresses and routes Byron addresses conservatively", async () => {
    await expect(
      detectAddressNetwork(
        "addr1qx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3n0d3vllmyqwsx5wktcd8cc3sq835lu7drv2xwl2wywfgse35a3x",
      ),
    ).resolves.toMatchObject({ status: "detected", network: "cardano" });
    await expect(
      detectAddressNetwork(
        "addr1qx2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3n0d3vllmyqwsx5wktcd8cc3sq835lu7drv2xwl2wywfgse35a3y",
      ),
    ).resolves.toMatchObject({ status: "invalid" });
    await expect(
      detectAddressNetwork(
        "Ae2tdPwUPEZLs4HtbuNey7tK4hTKrwNwYtGqp7bDfCy2WdR3P6735W5Yfpe",
      ),
    ).resolves.toMatchObject({
      status: "ambiguous",
      family: "Cardano-compatible",
    });
  });
  it("does not guess the network for a hex format shared by EVM and Move", async () => {
    const result = await detectAddressNetwork(
      "0x742d35Cc6634C0532925a3b844Bc454e4438f44e",
    );

    expect(result.status).toBe("ambiguous");
    if (result.status === "ambiguous") {
      expect(result.family).toBe("Hex-compatible");
      expect(result.message).toContain("EVM, Aptos, or Sui");
    }
  });

  it("validates a Bitcoin Base58Check mainnet address", async () => {
    await expect(
      detectAddressNetwork("1BoatSLRHtKNngkdXEeobR76b53LETtpyT"),
    ).resolves.toMatchObject({
      status: "detected",
      network: "bitcoin",
      confidence: "format-and-checksum",
    });
  });

  it("asks users to confirm a checksum-valid TRON Base58Check address", async () => {
    await expect(
      detectAddressNetwork("TUoHaVjx7n5xz8LwPRDckgFrDWhMhuSuJM"),
    ).resolves.toMatchObject({
      status: "ambiguous",
      family: "Tron-compatible",
      message: expect.stringContaining("Confirm TRON explicitly"),
    });
    await expect(
      detectAddressNetwork("TUoHaVjx7n5xz8LwPRDckgFrDWhMhuSujN"),
    ).resolves.not.toMatchObject({ network: "tron" });
  });

  it("detects an XRP Ledger classic address by its checksum", async () => {
    await expect(
      detectAddressNetwork("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh"),
    ).resolves.toMatchObject({ status: "detected", network: "xrpl" });
    await expect(
      detectAddressNetwork("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTi"),
    ).resolves.toMatchObject({ status: "invalid" });
  });

  it("explains that X-addresses are not supported", async () => {
    await expect(detectAddressNetwork("X7EVg4kB2e7S")).resolves.toMatchObject({
      status: "unsupported",
      message: expect.stringContaining("classic XRP Ledger address"),
    });
  });

  it("validates a Bitcoin Bech32 mainnet address", async () => {
    await expect(
      detectAddressNetwork("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"),
    ).resolves.toMatchObject({
      status: "detected",
      network: "bitcoin",
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

  it("routes shortened Move addresses to explicit Aptos or Sui selection", async () => {
    await expect(detectAddressNetwork("0x1234")).resolves.toMatchObject({
      status: "ambiguous",
      family: "Move-compatible",
    });
  });

  it("asks users to confirm Solana-compatible public-key formats", async () => {
    await expect(
      detectAddressNetwork("So11111111111111111111111111111111111111112"),
    ).resolves.toMatchObject({
      status: "ambiguous",
      family: "Solana-compatible",
      message: expect.stringContaining(
        "format alone does not prove the network",
      ),
    });

    await expect(
      detectAddressNetwork("not-a-public-key"),
    ).resolves.toMatchObject({
      status: "unsupported",
    });
    await expect(detectAddressNetwork("0".repeat(32))).resolves.toMatchObject({
      status: "unsupported",
    });
  });
});
