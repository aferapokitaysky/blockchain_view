export type NetworkFamily = "evm" | "utxo";

export type NetworkCatalogEntry = {
  id: string;
  name: string;
  family: NetworkFamily;
  chain_id: number | null;
  native_asset: string;
  explorer_url: string | null;
  history_sync_available?: boolean;
};

export type NetworkCatalog = {
  networks: NetworkCatalogEntry[];
  history_sync_available?: boolean;
};

const NETWORK_ID_PATTERN = /^(bitcoin|ethereum|base|arbitrum|eip155:[1-9]\d*)$/;

export function parseNetworkCatalog(value: unknown): NetworkCatalog {
  if (!value || typeof value !== "object" || !("networks" in value)) {
    throw new Error("Network catalog response is invalid");
  }

  const response = value as {
    networks?: unknown;
    history_sync_available?: unknown;
  };
  if (
    !Array.isArray(response.networks) ||
    typeof response.history_sync_available !== "boolean"
  ) {
    throw new Error("Network catalog response is invalid");
  }

  const seenNetworkIds = new Set<string>();
  const networks = response.networks.flatMap((candidate) => {
    if (!candidate || typeof candidate !== "object") return [];
    const entry = candidate as Partial<NetworkCatalogEntry>;
    if (
      typeof entry.id !== "string" ||
      !NETWORK_ID_PATTERN.test(entry.id) ||
      typeof entry.name !== "string" ||
      !entry.name.trim() ||
      (entry.family !== "evm" && entry.family !== "utxo") ||
      !(
        entry.chain_id === null ||
        (typeof entry.chain_id === "number" &&
          Number.isSafeInteger(entry.chain_id) &&
          entry.chain_id > 0)
      ) ||
      typeof entry.native_asset !== "string" ||
      !entry.native_asset.trim() ||
      !(entry.explorer_url === null || isHttpsUrl(entry.explorer_url)) ||
      (entry.history_sync_available !== undefined &&
        typeof entry.history_sync_available !== "boolean")
    ) {
      return [];
    }

    const isEvmId =
      entry.id === "ethereum" ||
      entry.id === "base" ||
      entry.id === "arbitrum" ||
      entry.id.startsWith("eip155:");
    const legacyNetwork = LEGACY_NETWORKS.find(({ id }) => id === entry.id);
    if (
      (isEvmId &&
        (entry.family !== "evm" ||
          typeof entry.chain_id !== "number" ||
          (legacyNetwork !== undefined &&
            entry.chain_id !== legacyNetwork.chain_id) ||
          (entry.id.startsWith("eip155:") &&
            Number(entry.id.slice("eip155:".length)) !== entry.chain_id))) ||
      (entry.id === "bitcoin" &&
        (entry.family !== "utxo" || entry.chain_id !== null))
    ) {
      return [];
    }

    if (seenNetworkIds.has(entry.id)) return [];
    seenNetworkIds.add(entry.id);
    return [entry as NetworkCatalogEntry];
  });

  if (networks.length === 0) {
    throw new Error("Network catalog contains no supported networks");
  }

  return {
    networks,
    history_sync_available: response.history_sync_available,
  };
}

function isHttpsUrl(value: unknown): value is string {
  if (typeof value !== "string") return false;
  try {
    return new URL(value).protocol === "https:";
  } catch {
    return false;
  }
}

export const LEGACY_NETWORKS: NetworkCatalogEntry[] = [
  {
    id: "ethereum",
    name: "Ethereum",
    family: "evm",
    chain_id: 1,
    native_asset: "ETH",
    explorer_url: "https://eth.blockscout.com/",
  },
  {
    id: "bitcoin",
    name: "Bitcoin",
    family: "utxo",
    chain_id: null,
    native_asset: "BTC",
    explorer_url: "https://blockstream.info/",
  },
  {
    id: "base",
    name: "Base",
    family: "evm",
    chain_id: 8453,
    native_asset: "ETH",
    explorer_url: "https://base.blockscout.com/",
  },
  {
    id: "arbitrum",
    name: "Arbitrum One",
    family: "evm",
    chain_id: 42161,
    native_asset: "ETH",
    explorer_url: "https://arbitrum.blockscout.com/",
  },
];

const LEGACY_EVM_NETWORKS = new Set(["ethereum", "base", "arbitrum"]);

export function isEvmNetworkId(networkId: string): boolean {
  const normalizedId = networkId.toLowerCase();
  const chainId = /^eip155:([1-9]\d*)$/.exec(normalizedId)?.[1];
  return (
    LEGACY_EVM_NETWORKS.has(normalizedId) ||
    (chainId !== undefined && Number.isSafeInteger(Number(chainId)))
  );
}

export function hasAddressScreeningMapping(networkId: string): boolean {
  return ["bitcoin", "ethereum", "base", "arbitrum"].includes(
    networkId.toLowerCase(),
  );
}

export function getNetworkName(
  networkId: string,
  networks: NetworkCatalogEntry[],
): string {
  return (
    LEGACY_NETWORKS.find(({ id }) => id === networkId.toLowerCase())?.name ??
    networks.find(({ id }) => id.toLowerCase() === networkId.toLowerCase())
      ?.name ??
    networkId
  );
}

export function getNetworkSyncAvailability(
  networkId: string,
  catalog: NetworkCatalog,
): boolean | undefined {
  const network = catalog.networks.find(
    ({ id }) => id.toLowerCase() === networkId.toLowerCase(),
  );
  if (network?.history_sync_available !== undefined) {
    return network.history_sync_available;
  }
  return networkId.toLowerCase() === "bitcoin"
    ? undefined
    : catalog.history_sync_available;
}
