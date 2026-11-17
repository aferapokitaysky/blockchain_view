import { describe, expect, it } from "vitest";
import {
  getNetworkName,
  getNetworkSyncAvailability,
  hasAddressScreeningMapping,
  isEvmNetworkId,
  LEGACY_NETWORKS,
  parseNetworkCatalog,
} from "./networkCatalog";

describe("network catalog helpers", () => {
  it("recognizes legacy EVM ids and catalog chain ids without mixing networks", () => {
    expect(isEvmNetworkId("ethereum")).toBe(true);
    expect(isEvmNetworkId("eip155:10")).toBe(true);
    expect(isEvmNetworkId("eip155:0")).toBe(false);
    expect(isEvmNetworkId("eip155:999999999999999999999999")).toBe(false);
    expect(isEvmNetworkId("bitcoin")).toBe(false);
    expect(isEvmNetworkId("eip155:not-a-chain")).toBe(false);
  });

  it("does not send dynamic EVM network ids to the legacy screening endpoint", () => {
    expect(hasAddressScreeningMapping("ethereum")).toBe(true);
    expect(hasAddressScreeningMapping("eip155:10")).toBe(false);
  });

  it("accepts Solana only with its dedicated family and no EVM chain id", () => {
    const catalog = parseNetworkCatalog({
      history_sync_available: true,
      networks: [
        {
          id: "solana",
          name: "Solana",
          family: "solana",
          chain_id: null,
          native_asset: "SOL",
          explorer_url: "https://explorer.solana.com/",
          history_sync_available: true,
        },
      ],
    });

    expect(catalog.networks).toMatchObject([
      { id: "solana", family: "solana", chain_id: null },
    ]);
    expect(() =>
      parseNetworkCatalog({
        history_sync_available: true,
        networks: [
          {
            ...LEGACY_NETWORKS[0],
            id: "solana",
            family: "evm",
            chain_id: 1,
          },
        ],
      }),
    ).toThrow("contains no supported networks");
  });

  it("resolves catalog labels and preserves an unknown id as a visible fallback", () => {
    const networks = [
      ...LEGACY_NETWORKS,
      {
        id: "eip155:10",
        name: "OP Mainnet",
        family: "evm" as const,
        chain_id: 10,
        native_asset: "ETH",
        explorer_url: "https://explorer.optimism.io/",
      },
    ];

    expect(getNetworkName("eip155:10", networks)).toBe("OP Mainnet");
    expect(getNetworkName("eip155:84532", networks)).toBe("eip155:84532");
  });

  it("exposes provider availability only when the catalog explicitly reports it", () => {
    expect(
      getNetworkSyncAvailability("ethereum", {
        networks: [{ ...LEGACY_NETWORKS[0], history_sync_available: false }],
        history_sync_available: true,
      }),
    ).toBe(false);
    expect(
      getNetworkSyncAvailability("eip155:10", {
        networks: LEGACY_NETWORKS,
      }),
    ).toBeUndefined();
    expect(
      getNetworkSyncAvailability("bitcoin", {
        networks: LEGACY_NETWORKS,
        history_sync_available: false,
      }),
    ).toBeUndefined();
  });

  it("validates catalog ids and safe explorer URLs before using server data", () => {
    const catalog = parseNetworkCatalog({
      history_sync_available: true,
      networks: [
        {
          id: "eip155:10",
          name: "OP Mainnet",
          family: "evm",
          chain_id: 10,
          native_asset: "ETH",
          explorer_url: "https://explorer.optimism.io/",
          history_sync_available: false,
        },
        {
          id: "eip155:11",
          name: "unsafe",
          family: "evm",
          chain_id: 11,
          native_asset: "ETH",
          explorer_url: "javascript:alert(1)",
        },
        {
          id: "custom-chain",
          name: "unknown",
          family: "evm",
          chain_id: 12,
          native_asset: "ETH",
          explorer_url: null,
        },
        {
          id: "ethereum",
          name: "Misconfigured Ethereum",
          family: "evm",
          chain_id: 10,
          native_asset: "ETH",
          explorer_url: null,
        },
        {
          id: "eip155:10",
          name: "Duplicate OP Mainnet",
          family: "evm",
          chain_id: 10,
          native_asset: "ETH",
          explorer_url: "https://other.example/",
        },
      ],
    });

    expect(catalog.networks).toEqual([
      expect.objectContaining({ id: "eip155:10", chain_id: 10 }),
    ]);
    expect(getNetworkSyncAvailability("eip155:10", catalog)).toBe(false);
    expect(getNetworkSyncAvailability("ethereum", catalog)).toBe(true);
  });

  it("rejects malformed or empty catalogs", () => {
    expect(() => parseNetworkCatalog(null)).toThrow("response is invalid");
    expect(() =>
      parseNetworkCatalog({
        networks: LEGACY_NETWORKS,
        history_sync_available: "unknown",
      }),
    ).toThrow("response is invalid");
    expect(() =>
      parseNetworkCatalog({ networks: [], history_sync_available: false }),
    ).toThrow("contains no supported networks");
  });
});
