import "./NetworkMark.css";

import {
  getEvmMarkId,
  getNetworkMarkKind,
  hashNetworkId,
  type MarkKind,
} from "./networkMarkModel";

type NetworkMarkProps = {
  networkId: string;
  label: string;
  chainId?: number | null;
};

function NetworkShape({
  kind,
  networkId,
  chainId,
}: {
  kind: MarkKind;
  networkId: string;
  chainId?: number | null;
}) {
  if (kind === "evm") {
    const markId = getEvmMarkId(networkId, chainId);
    const seed = hashNetworkId(markId);
    const rotation = seed % 360;
    const innerRadius = 4 + (seed % 3);
    return (
      <g className="network-mark__evm" transform={`rotate(${rotation} 12 12)`}>
        <path d="M12 2.5 20.2 7.2v9.6L12 21.5l-8.2-4.7V7.2L12 2.5Z" />
        <circle cx="12" cy="12" r={innerRadius} />
        <path d={`M12 3v${18}`} strokeDasharray={`${4 + (seed % 4)} 3`} />
      </g>
    );
  }

  return (
    <g
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {kind === "bitcoin" && (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M10 6v12m4-12v12M8.7 9h5.1a2 2 0 0 1 0 4H8.7m0 0h5.7a2 2 0 0 1 0 4H8.7" />
        </>
      )}
      {kind === "ethereum" && (
        <>
          <path d="m12 2.5 6.4 9.2L12 15l-6.4-3.3L12 2.5Z" />
          <path d="m5.6 13.2 6.4 3.3 6.4-3.3L12 21.5l-6.4-8.3Z" />
        </>
      )}
      {kind === "base" && (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M20.5 9.5H12a2.5 2.5 0 1 0 0 5h8.5" />
        </>
      )}
      {kind === "arbitrum" && (
        <>
          <path d="m12 2.5 8 4.6v9.8l-8 4.6-8-4.6V7.1l8-4.6Z" />
          <path d="m8 8 4 8m4-8-4 8m-1.8-2.4h3.6" />
        </>
      )}
      {kind === "polygon" && (
        <>
          <path d="M7.2 8.5 12 5.8l4.8 2.7v5L12 16.2l-4.8-2.7v-5Z" />
          <path d="m4 10.3 3.2-1.8m9.6 5.2 3.2-1.8M8 17.8l4 2.3 4-2.3" />
        </>
      )}
      {kind === "bnb" && (
        <>
          <path d="m12 3 2.4 2.4L12 7.8 9.6 5.4 12 3Zm-4 4 2.4 2.4L8 11.8 5.6 9.4 8 7Zm8 0 2.4 2.4L16 11.8l-2.4-2.4L16 7Zm-4 4 2.4 2.4L12 15.8l-2.4-2.4L12 11Zm-4 4 2.4 2.4L8 19.8l-2.4-2.4L8 15Zm8 0 2.4 2.4L16 19.8l-2.4-2.4L16 15Z" />
        </>
      )}
      {kind === "optimism" && (
        <>
          <path d="M4 6h9.5a5 5 0 0 1 0 10H9" />
          <path d="m8 12 4 4-4 4" />
          <circle cx="18" cy="7" r="2.5" />
        </>
      )}
      {kind === "solana" && (
        <>
          <path
            d="M6 5.5h12.5L16 8H3.5l2.5-2.5Zm0 5.2h14.5L18 13.2H3.5l2.5-2.5Zm0 5.3h12.5L16 18.5H3.5l2.5-2.5Z"
            fill="currentColor"
            stroke="none"
          />
        </>
      )}
      {kind === "tron" && (
        <>
          <path d="M4 4.5 20 8 12 20 4 4.5Z" />
          <path d="m7.1 10.3 9.8 0M9.4 13.1h5.2" />
          <circle cx="4" cy="4.5" r="1.1" fill="currentColor" />
          <circle cx="20" cy="8" r="1.1" fill="currentColor" />
          <circle cx="12" cy="20" r="1.1" fill="currentColor" />
        </>
      )}
      {kind === "xrpl" && (
        <>
          <path d="M4 6h4.2c2.2 0 3.2 1.1 3.8 2.1l.1.2c.6 1 1.3 1.7 2.7 1.7H20M4 18h4.2c2.2 0 3.2-1.1 3.8-2.1l.1-.2c.6-1 1.3-1.7 2.7-1.7H20" />
        </>
      )}
      {kind === "ton" && (
        <>
          <path d="M12 3 20 7.5v9L12 21 4 16.5v-9L12 3Z" />
          <path d="m9 8 3 8 3-8m-6 0h6" />
        </>
      )}
      {kind === "cardano" && (
        <>
          <circle cx="12" cy="12" r="1.5" fill="currentColor" />
          <circle cx="12" cy="6" r="1.2" fill="currentColor" />
          <circle cx="12" cy="18" r="1.2" fill="currentColor" />
          <circle cx="6.8" cy="9" r="1.2" fill="currentColor" />
          <circle cx="17.2" cy="9" r="1.2" fill="currentColor" />
          <circle cx="6.8" cy="15" r="1.2" fill="currentColor" />
          <circle cx="17.2" cy="15" r="1.2" fill="currentColor" />
          <circle cx="8.3" cy="5.3" r=".7" fill="currentColor" />
          <circle cx="15.7" cy="5.3" r=".7" fill="currentColor" />
          <circle cx="8.3" cy="18.7" r=".7" fill="currentColor" />
          <circle cx="15.7" cy="18.7" r=".7" fill="currentColor" />
        </>
      )}
      {kind === "aptos" && (
        <>
          <path d="M4 9.5h5.3c1.8 0 2.7 1 3.5 2.5s1.7 2.5 3.5 2.5H20M4 14.5h5.3c1.8 0 2.7-1 3.5-2.5s1.7-2.5 3.5-2.5H20" />
          <circle cx="4" cy="9.5" r="1" fill="currentColor" />
          <circle cx="20" cy="14.5" r="1" fill="currentColor" />
        </>
      )}
      {kind === "sui" && (
        <>
          <path d="m12 2.5 7 4v8l-7 4-7-4v-8l7-4Z" />
          <path d="M8 9 12 11.3 16 9m-8 4 4 2.3 4-2.3m-4-2.7v4.9" />
        </>
      )}
      {kind === "generic" && (
        <>
          <circle cx="12" cy="12" r="8.5" />
          <path d="M7.5 12h9" />
        </>
      )}
    </g>
  );
}

export function NetworkMark({ networkId, label, chainId }: NetworkMarkProps) {
  const kind = getNetworkMarkKind(networkId, chainId);
  return (
    <span
      className="network-mark"
      data-network-id={networkId.toLowerCase()}
      data-mark-id={kind === "evm" ? getEvmMarkId(networkId, chainId) : kind}
      style={{ "--network": networkColor(kind) } as React.CSSProperties}
      role="img"
      aria-label={label}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
        <NetworkShape kind={kind} networkId={networkId} chainId={chainId} />
      </svg>
    </span>
  );
}

function networkColor(kind: MarkKind): string {
  const colors: Record<MarkKind, string> = {
    bitcoin: "#f2a84a",
    ethereum: "#a98cff",
    base: "#5595ff",
    arbitrum: "#64c8fa",
    polygon: "#a98cff",
    bnb: "#f0b90b",
    optimism: "#ff5964",
    solana: "#9b8cff",
    tron: "#ff5964",
    xrpl: "#20a8d8",
    ton: "#6ca8ff",
    cardano: "#5595ff",
    aptos: "#b6e7f6",
    sui: "#68d5c2",
    evm: "#7aabff",
    generic: "#a3b0a7",
  };
  return colors[kind];
}
