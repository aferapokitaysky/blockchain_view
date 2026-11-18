export type MarkKind =
  | "bitcoin"
  | "ethereum"
  | "base"
  | "arbitrum"
  | "polygon"
  | "bnb"
  | "optimism"
  | "solana"
  | "tron"
  | "xrpl"
  | "ton"
  | "cardano"
  | "aptos"
  | "sui"
  | "evm"
  | "generic";

const MARKS_BY_NETWORK_ID: Record<string, MarkKind> = {
  bitcoin: "bitcoin",
  ethereum: "ethereum",
  base: "base",
  arbitrum: "arbitrum",
  "eip155:137": "polygon",
  "eip155:56": "bnb",
  "eip155:10": "optimism",
  solana: "solana",
  tron: "tron",
  xrpl: "xrpl",
  ton: "ton",
  cardano: "cardano",
  aptos: "aptos",
  sui: "sui",
};

export function getNetworkMarkKind(
  networkId: string,
  chainId?: number | null,
): MarkKind {
  const id = networkId.trim().toLowerCase();
  const knownMark = MARKS_BY_NETWORK_ID[id];
  if (knownMark) return knownMark;
  if (id.startsWith("eip155:") || (chainId != null && chainId > 0)) {
    return "evm";
  }
  return "generic";
}

export function getEvmMarkId(networkId: string, chainId?: number | null) {
  const match = /^eip155:([1-9]\d*)$/.exec(networkId.trim().toLowerCase());
  const resolvedChainId = chainId ?? (match ? Number(match[1]) : null);
  return resolvedChainId === null
    ? `evm-${hashNetworkId(networkId).toString(36)}`
    : `evm-${resolvedChainId}`;
}

export function hashNetworkId(networkId: string): number {
  let hash = 2166136261;
  for (const character of networkId.toLowerCase()) {
    hash = Math.imul(hash ^ character.charCodeAt(0), 16777619);
  }
  return hash >>> 0;
}
