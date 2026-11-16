import {
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
  type FormEvent,
} from "react";
import {
  Activity,
  ArrowDownLeft,
  ArrowUpRight,
  Bell,
  Blocks,
  ChevronDown,
  CircleHelp,
  Clock3,
  Copy,
  Crosshair,
  Download,
  ExternalLink,
  Filter,
  Focus,
  GitBranch,
  Layers3,
  Link2,
  Menu,
  MoreHorizontal,
  Plus,
  Search,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  Wallet,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { buildWalletAssessment } from "./features/risk/scoreWallet";
import {
  detectAddressNetwork,
  type AddressDetection,
  type SupportedNetwork,
} from "./features/networks/detectAddressNetwork";
import {
  ApiRequestError,
  checkApiHealth,
  listTrackedWalletHistories,
  listTrackedWallets,
  listWalletHistory,
  listWalletRiskSignals,
  screenWalletAddress,
  syncWalletHistory,
  trackWallet,
  type ObservedTransfer,
  type AddressScreeningResult,
  type StoredRiskSignal,
  type TrackedAddress,
  type WalletHistory,
} from "./features/investigations/api";
import { WalletFlowGraph } from "./features/graph/WalletFlowGraph";
import { formatObservedAmount } from "./features/investigations/formatAssetAmount";
import {
  buildTransactionFeed,
  formatFeedTime,
} from "./features/investigations/transactionFeed";
import "./App.css";

type Network = "Ethereum" | "Bitcoin" | "Base" | "Arbitrum";
const TRANSACTIONS_PER_PAGE = 50;
type GraphNode = {
  id: string;
  address: string;
  name: string;
  network: Network;
  x: number;
  y: number;
  balance: string;
  tag?: string;
  central?: boolean;
};
function safeExternalLink(reference: string | null): string | null {
  if (!reference) return null;
  try {
    const parsed = new URL(reference);
    return parsed.protocol === "https:" || parsed.protocol === "http:"
      ? parsed.href
      : null;
  } catch {
    return null;
  }
}
const nodes: GraphNode[] = [
  {
    id: "seed",
    address: "0x7a3d…4c91",
    name: "Exchange wallet",
    network: "Ethereum",
    x: 502,
    y: 258,
    balance: "$2.84M",
    tag: "Exchange",
    central: true,
  },
  {
    id: "north",
    address: "0xb61c…8a20",
    name: "Northstar",
    network: "Ethereum",
    x: 268,
    y: 117,
    balance: "$418.2K",
    tag: "New",
  },
  {
    id: "cove",
    address: "bc1q…0f8j",
    name: "Unlabeled wallet",
    network: "Bitcoin",
    x: 742,
    y: 112,
    balance: "$92.4K",
  },
  {
    id: "orbit",
    address: "0xe12a…11bf",
    name: "Orbit Capital",
    network: "Base",
    x: 202,
    y: 338,
    balance: "$1.2M",
    tag: "Fund",
  },
  {
    id: "glacier",
    address: "0x041f…2d7e",
    name: "Glacier",
    network: "Arbitrum",
    x: 770,
    y: 355,
    balance: "$74.8K",
  },
  {
    id: "cedar",
    address: "0x69d1…9c34",
    name: "Unlabeled wallet",
    network: "Ethereum",
    x: 428,
    y: 74,
    balance: "$16.1K",
  },
  {
    id: "delta",
    address: "bc1p…58m2",
    name: "Delta desk",
    network: "Bitcoin",
    x: 875,
    y: 227,
    balance: "$310.6K",
    tag: "Desk",
  },
  {
    id: "lumen",
    address: "0xabc2…a803",
    name: "Lumen",
    network: "Base",
    x: 390,
    y: 429,
    balance: "$6.9K",
  },
  {
    id: "harbor",
    address: "0x8d91…e0c1",
    name: "Harbor",
    network: "Ethereum",
    x: 640,
    y: 444,
    balance: "$238K",
    tag: "Bridge",
  },
];

const edges = [
  ["seed", "north", "2.45 ETH"],
  ["seed", "cove", "0.84 BTC"],
  ["seed", "orbit", "18,200 USDC"],
  ["seed", "glacier", "4.12 ETH"],
  ["seed", "cedar", "0.68 ETH"],
  ["seed", "delta", "3.2 BTC"],
  ["seed", "lumen", "2,400 USDC"],
  ["seed", "harbor", "6.8 ETH"],
  ["north", "cedar", "0.21 ETH"],
  ["orbit", "lumen", "850 USDC"],
  ["glacier", "harbor", "1.04 ETH"],
  ["cove", "delta", "0.13 BTC"],
] as const;

const networkMeta: Record<Network, { color: string }> = {
  Ethereum: { color: "#a98cff" },
  Bitcoin: { color: "#f2a84a" },
  Base: { color: "#5595ff" },
  Arbitrum: { color: "#64c8fa" },
};

function NetworkShape({ network }: { network: Network }) {
  return (
    <g
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {network === "Bitcoin" && (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M10 6.5v11m4-11v11M8.8 9h5a2 2 0 0 1 0 4h-5m0 0h5.7a2 2 0 0 1 0 4H8.8" />
        </>
      )}
      {network === "Ethereum" && (
        <>
          <path d="m12 2.5 6.2 9.3L12 15l-6.2-3.2L12 2.5Z" />
          <path d="m5.8 13.1 6.2 3.2 6.2-3.2L12 21.5l-6.2-8.4Z" />
        </>
      )}
      {network === "Base" && (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M20.5 9.5H12a2.5 2.5 0 1 0 0 5h8.5" />
        </>
      )}
      {network === "Arbitrum" && (
        <>
          <path d="m12 2.5 8 4.6v9.8l-8 4.6-8-4.6V7.1l8-4.6Z" />
          <path d="m8 8 4 8m4-8-4 8m-1.8-2.4h3.6" />
        </>
      )}
    </g>
  );
}

function NetworkMark({ network }: { network: Network }) {
  return (
    <span
      className="network-mark"
      style={{ "--network": networkMeta[network].color } as CSSProperties}
      aria-label={network}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <NetworkShape network={network} />
      </svg>
    </span>
  );
}

function TransferGraph({
  selected,
  onSelect,
  zoom,
}: {
  selected: string;
  onSelect: (id: string) => void;
  zoom: number;
}) {
  const points = useMemo(
    () => Object.fromEntries(nodes.map((node) => [node.id, node])),
    [],
  );
  return (
    <svg
      className="flow-svg"
      viewBox="0 0 1000 520"
      role="img"
      aria-label="Граф связей кошельков и переводов"
    >
      <defs>
        <pattern
          id="graph-grid"
          width="24"
          height="24"
          patternUnits="userSpaceOnUse"
        >
          <circle cx="1" cy="1" r="1" fill="#26302f" />
        </pattern>
        <filter id="node-glow" x="-100%" y="-100%" width="300%" height="300%">
          <feGaussianBlur stdDeviation="5" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
        <marker
          id="arrow"
          viewBox="0 0 10 10"
          refX="8"
          refY="5"
          markerWidth="5"
          markerHeight="5"
          orient="auto-start-reverse"
        >
          <path d="M 0 0 L 10 5 L 0 10 z" fill="#59d5ad" opacity=".8" />
        </marker>
      </defs>
      <rect width="1000" height="520" fill="url(#graph-grid)" />
      <g
        className="graph-content"
        style={{ transform: `scale(${zoom})`, transformOrigin: "50% 50%" }}
      >
        {edges.map(([from, to, amount], index) => {
          const a = points[from],
            b = points[to];
          const curve =
            index < 8
              ? (a.y + b.y) / 2 + (a.x < b.x ? -12 : 18)
              : (a.y + b.y) / 2 - 20;
          const midX = (a.x + b.x) / 2;
          return (
            <g className="edge" key={`${from}-${to}`}>
              <path
                d={`M ${a.x} ${a.y} Q ${midX} ${curve} ${b.x} ${b.y}`}
                markerEnd="url(#arrow)"
              />
              <circle className="edge-pulse" r="2.5">
                <animateMotion
                  dur={`${4 + (index % 3)}s`}
                  repeatCount="indefinite"
                  path={`M ${a.x} ${a.y} Q ${midX} ${curve} ${b.x} ${b.y}`}
                />
              </circle>
              {index < 8 && (
                <text x={midX} y={curve - 7} className="edge-label">
                  {amount}
                </text>
              )}
            </g>
          );
        })}
        {nodes.map((node, index) => {
          const active = selected === node.id;
          const radius = node.central ? 28 : 20;
          return (
            <g
              key={node.id}
              className={`graph-node ${active ? "is-selected" : ""}`}
              onClick={() => onSelect(node.id)}
              role="button"
              tabIndex={0}
              aria-label={`${node.name}, ${node.network}`}
              onKeyDown={(event) => event.key === "Enter" && onSelect(node.id)}
            >
              {node.central && (
                <circle cx={node.x} cy={node.y} r="43" className="node-halo">
                  <animate
                    attributeName="r"
                    values="39;47;39"
                    dur="4s"
                    repeatCount="indefinite"
                  />
                </circle>
              )}
              <circle
                cx={node.x}
                cy={node.y}
                r={radius + 7}
                className="node-ring"
              />
              <circle
                cx={node.x}
                cy={node.y}
                r={radius}
                className="node-core"
                style={
                  {
                    "--node-color": networkMeta[node.network].color,
                  } as CSSProperties
                }
                filter={node.central ? "url(#node-glow)" : undefined}
              />
              <g
                transform={`translate(${node.x - 12} ${node.y - 12})`}
                className="node-mark"
                style={
                  {
                    "--node-color": networkMeta[node.network].color,
                  } as CSSProperties
                }
              >
                <NetworkShape network={node.network} />
              </g>
              <text
                x={node.x}
                y={node.y + radius + 19}
                textAnchor="middle"
                className="node-name"
              >
                {node.name}
              </text>
              <text
                x={node.x}
                y={node.y + radius + 34}
                textAnchor="middle"
                className="node-address"
              >
                {node.address}
              </text>
              {node.tag && (
                <g>
                  <rect
                    x={node.x - 29}
                    y={node.y - radius - 25}
                    width="58"
                    height="17"
                    rx="8"
                    className="node-tag"
                  />
                  <text
                    x={node.x}
                    y={node.y - radius - 13}
                    textAnchor="middle"
                    className="node-tag-text"
                  >
                    {node.tag}
                  </text>
                </g>
              )}
              {index === 0 && (
                <circle
                  cx={node.x + radius - 1}
                  cy={node.y - radius + 1}
                  r="4"
                  className="node-live"
                />
              )}
            </g>
          );
        })}
      </g>
    </svg>
  );
}

function App() {
  const [selected, setSelected] = useState("seed");
  const [activeNetwork, setActiveNetwork] = useState<"All networks" | Network>(
    "All networks",
  );
  const [zoom, setZoom] = useState(1);
  const [showAdd, setShowAdd] = useState(false);
  const [query, setQuery] = useState("");
  const [walletLabel, setWalletLabel] = useState("");
  const [networkChoice, setNetworkChoice] = useState<SupportedNetwork | "">("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [formError, setFormError] = useState("");
  const [apiStatus, setApiStatus] = useState<"checking" | "online" | "offline">(
    "checking",
  );
  const [addressDetection, setAddressDetection] = useState<AddressDetection>({
    status: "empty",
    message: "Paste a wallet address.",
  });
  const [saved, setSaved] = useState(false);
  const [notice, setNotice] = useState("");
  const [trackedAddresses, setTrackedAddresses] = useState<TrackedAddress[]>(
    [],
  );
  const [addressesLoading, setAddressesLoading] = useState(true);
  const [addressesError, setAddressesError] = useState("");
  const [selectedAddressId, setSelectedAddressId] = useState("");
  const [isSyncing, setIsSyncing] = useState(false);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyError, setHistoryError] = useState("");
  const [historyRecords, setHistoryRecords] = useState<
    ObservedTransfer[] | null
  >(null);
  const [flowHistories, setFlowHistories] = useState<WalletHistory[] | null>(
    null,
  );
  const [flowLoading, setFlowLoading] = useState(false);
  const [flowError, setFlowError] = useState("");
  const [screeningLoading, setScreeningLoading] = useState(false);
  const [screeningError, setScreeningError] = useState("");
  const [screeningResult, setScreeningResult] =
    useState<AddressScreeningResult | null>(null);
  const [walletRiskSignalResult, setWalletRiskSignalResult] = useState<{
    addressId: string;
    signals: StoredRiskSignal[];
  } | null>(null);
  const [riskSignalFailure, setRiskSignalFailure] = useState<{
    addressId: string;
    message: string;
  } | null>(null);
  const [riskRefreshKey, setRiskRefreshKey] = useState(0);
  const [activityHistories, setActivityHistories] = useState<WalletHistory[]>(
    [],
  );
  const [activityStatus, setActivityStatus] = useState<
    "loading" | "ready" | "error"
  >("loading");
  const [activityError, setActivityError] = useState("");
  const [activityRefreshKey, setActivityRefreshKey] = useState(0);
  const [activityPage, setActivityPage] = useState(0);
  const [activeTab, setActiveTab] = useState("Overview");
  const selectedNode = nodes.find((node) => node.id === selected) ?? nodes[0];
  const selectedTrackedAddress = trackedAddresses.find(
    (address) => address.id === selectedAddressId,
  );
  const walletRiskSignals =
    selectedTrackedAddress &&
    walletRiskSignalResult?.addressId === selectedTrackedAddress.id
      ? walletRiskSignalResult.signals
      : null;
  const riskSignalsError =
    selectedTrackedAddress &&
    riskSignalFailure?.addressId === selectedTrackedAddress.id
      ? riskSignalFailure.message
      : "";
  const riskSignalsLoading = Boolean(
    selectedTrackedAddress && !walletRiskSignals && !riskSignalsError,
  );
  const transactionFeed = useMemo(
    () => buildTransactionFeed(activityHistories),
    [activityHistories],
  );
  const visibleTransfers = transactionFeed.filter(
    (transfer) =>
      activeNetwork === "All networks" || transfer.network === activeNetwork,
  );
  const activityPageCount = Math.ceil(
    visibleTransfers.length / TRANSACTIONS_PER_PAGE,
  );
  const currentActivityPage = Math.min(
    activityPage,
    Math.max(0, activityPageCount - 1),
  );
  const pagedTransfers = visibleTransfers.slice(
    currentActivityPage * TRANSACTIONS_PER_PAGE,
    (currentActivityPage + 1) * TRANSACTIONS_PER_PAGE,
  );
  const walletAssessment = useMemo(
    () => buildWalletAssessment(walletRiskSignals ?? [], screeningResult),
    [walletRiskSignals, screeningResult],
  );

  useEffect(() => {
    let isCurrent = true;
    void detectAddressNetwork(query).then((detection) => {
      if (isCurrent) setAddressDetection(detection);
    });
    return () => {
      isCurrent = false;
    };
  }, [query]);

  useEffect(() => {
    let isCurrent = true;
    const refreshApiStatus = () => {
      void checkApiHealth().then((isOnline) => {
        if (isCurrent) setApiStatus(isOnline ? "online" : "offline");
      });
    };
    refreshApiStatus();
    const refreshInterval = window.setInterval(refreshApiStatus, 30_000);
    return () => {
      isCurrent = false;
      window.clearInterval(refreshInterval);
    };
  }, []);

  useEffect(() => {
    let isCurrent = true;
    if (!selectedTrackedAddress) return;
    const { id: addressId } = selectedTrackedAddress;
    void listWalletRiskSignals(selectedTrackedAddress)
      .then((signals) => {
        if (!isCurrent) return;
        setWalletRiskSignalResult({ addressId, signals });
        setRiskSignalFailure((current) =>
          current?.addressId === addressId ? null : current,
        );
      })
      .catch((error: unknown) => {
        if (!isCurrent) return;
        setRiskSignalFailure({
          addressId,
          message:
            error instanceof ApiRequestError
              ? error.message
              : "Could not load wallet risk signals. Try again.",
        });
      });

    return () => {
      isCurrent = false;
    };
  }, [selectedTrackedAddress, riskRefreshKey]);

  const loadTrackedAddresses = async () => {
    setAddressesError("");
    try {
      const addresses = await listTrackedWallets();
      setTrackedAddresses(addresses);
      setSelectedAddressId((current) =>
        addresses.some((address) => address.id === current)
          ? current
          : (addresses[0]?.id ?? ""),
      );
    } catch (error) {
      setAddressesError(
        error instanceof ApiRequestError
          ? error.message
          : "Could not load saved addresses. Try again.",
      );
    } finally {
      setAddressesLoading(false);
    }
  };

  const retryAddressLoad = async () => {
    setAddressesLoading(true);
    const isOnline = await checkApiHealth();
    setApiStatus(isOnline ? "online" : "offline");
    if (isOnline) {
      await loadTrackedAddresses();
    } else {
      setAddressesError("The API is unavailable. Check the service and retry.");
      setAddressesLoading(false);
    }
  };

  useEffect(() => {
    if (apiStatus === "online") {
      void Promise.resolve().then(loadTrackedAddresses);
    }
  }, [apiStatus]);

  useEffect(() => {
    let isCurrent = true;
    void Promise.resolve().then(async () => {
      if (
        apiStatus === "checking" ||
        (apiStatus === "online" && addressesLoading)
      ) {
        if (isCurrent) setActivityStatus("loading");
        return;
      }
      if (apiStatus !== "online") {
        if (!isCurrent) return;
        setActivityStatus("error");
        setActivityError("The local API is unavailable. Start it and retry.");
        return;
      }
      if (addressesError) {
        if (!isCurrent) return;
        setActivityStatus("error");
        setActivityError(addressesError);
        return;
      }
      if (trackedAddresses.length === 0) {
        if (!isCurrent) return;
        setActivityHistories([]);
        setActivityError("");
        setActivityStatus("ready");
        return;
      }

      if (!isCurrent) return;
      setActivityError("");
      setActivityStatus("loading");
      try {
        const histories = await listTrackedWalletHistories(trackedAddresses);
        if (!isCurrent) return;
        setActivityHistories(histories);
        setActivityStatus("ready");
      } catch (error: unknown) {
        if (!isCurrent) return;
        setActivityError(
          error instanceof ApiRequestError
            ? error.message
            : "Could not load saved transaction observations. Try again.",
        );
        setActivityStatus("error");
      }
    });
    return () => {
      isCurrent = false;
    };
  }, [
    activityRefreshKey,
    addressesError,
    addressesLoading,
    apiStatus,
    trackedAddresses,
  ]);

  const handleSearch = (value: string) => {
    setQuery(value);
    setNetworkChoice("");
    setFormError("");
    if (!value) return;
    const match = nodes.find((node) =>
      `${node.address} ${node.name}`
        .toLowerCase()
        .includes(value.toLowerCase()),
    );
    if (match) setSelected(match.id);
  };

  const submitAddress = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const network =
      addressDetection.status === "detected"
        ? addressDetection.network
        : addressDetection.status === "ambiguous"
          ? networkChoice
          : "";
    if (!network) {
      setFormError(
        addressDetection.status === "ambiguous"
          ? "Select the EVM network before tracking this wallet"
          : "Enter a valid address on a supported network",
      );
      return;
    }

    setFormError("");
    setIsSubmitting(true);
    try {
      const trackedAddress = await trackWallet({
        network,
        address: query,
        label: walletLabel,
      });
      setTrackedAddresses((addresses) => [
        trackedAddress,
        ...addresses.filter((address) => address.id !== trackedAddress.id),
      ]);
      setSelectedAddressId(trackedAddress.id);
      setHistoryRecords(null);
      setScreeningResult(null);
      setShowAdd(false);
      setQuery("");
      setWalletLabel("");
      setNetworkChoice("");
      setNotice(`Wallet saved to ${network}. History can be synced on demand.`);
    } catch (error) {
      setFormError(
        error instanceof ApiRequestError
          ? error.message
          : "Could not save the wallet. Please try again.",
      );
    } finally {
      setIsSubmitting(false);
    }
  };

  const syncSavedAddress = async () => {
    if (!selectedTrackedAddress || isSyncing) return;
    setIsSyncing(true);
    setHistoryError("");
    try {
      const result = await syncWalletHistory(selectedTrackedAddress);
      let records: ObservedTransfer[];
      try {
        records = await listWalletHistory(selectedTrackedAddress);
      } catch {
        setHistoryError(
          "History synced, but saved records could not be loaded. Retry loading history.",
        );
        return;
      }
      setHistoryRecords(records);
      setActivityRefreshKey((revision) => revision + 1);
      const coverage = result.may_have_more_history
        ? " The provider page limit was reached; older history may be missing."
        : "";
      setNotice(
        `Saved ${result.fetched} ${selectedTrackedAddress.network} observations from ${result.provider}.${coverage}`,
      );
    } catch (error) {
      setHistoryError(
        error instanceof ApiRequestError
          ? error.message
          : "Could not sync Bitcoin history. Try again later.",
      );
    } finally {
      setIsSyncing(false);
    }
  };

  const buildWalletConnections = async () => {
    if (flowLoading) return;
    const evmAddresses = trackedAddresses.filter((address) =>
      ["ethereum", "base", "arbitrum"].includes(address.network),
    );
    setFlowLoading(true);
    setFlowError("");
    try {
      setFlowHistories(await listTrackedWalletHistories(evmAddresses));
    } catch (error) {
      setFlowError(
        error instanceof ApiRequestError
          ? error.message
          : "Could not load saved EVM history. Try again.",
      );
    } finally {
      setFlowLoading(false);
    }
  };

  const screenSelectedAddress = async () => {
    if (!selectedTrackedAddress || screeningLoading) return;
    setScreeningLoading(true);
    setScreeningError("");
    setScreeningResult(null);
    try {
      setScreeningResult(await screenWalletAddress(selectedTrackedAddress));
    } catch (error) {
      setScreeningError(
        error instanceof ApiRequestError
          ? error.message
          : "Could not check this address. Try again.",
      );
    } finally {
      setScreeningLoading(false);
    }
  };

  const refreshWalletRiskSignals = () => {
    if (!selectedTrackedAddress) return;
    setWalletRiskSignalResult((current) =>
      current?.addressId === selectedTrackedAddress.id ? null : current,
    );
    setRiskSignalFailure((current) =>
      current?.addressId === selectedTrackedAddress.id ? null : current,
    );
    setRiskRefreshKey((value) => value + 1);
  };

  const viewSavedHistory = async () => {
    if (!selectedTrackedAddress) return;
    setHistoryLoading(true);
    setHistoryError("");
    try {
      setHistoryRecords(await listWalletHistory(selectedTrackedAddress));
    } catch (error) {
      setHistoryError(
        error instanceof ApiRequestError
          ? error.message
          : "Could not load saved history. Try again.",
      );
    } finally {
      setHistoryLoading(false);
    }
  };

  const openAddWallet = () => {
    void checkApiHealth().then((isOnline) =>
      setApiStatus(isOnline ? "online" : "offline"),
    );
    setQuery("");
    setWalletLabel("");
    setNetworkChoice("");
    setFormError("");
    setShowAdd(true);
  };

  const copyAddress = async () => {
    await navigator.clipboard?.writeText(selectedNode.address);
    setNotice("Адрес скопирован");
    window.setTimeout(() => setNotice(""), 2400);
  };

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <a className="brand" href="#overview" aria-label="Tracefold главная">
          <span className="brand-symbol">
            <svg viewBox="0 0 32 32" aria-hidden="true">
              <path d="M6 16h8M18 8l8 8-8 8M14 16l4-8M14 16l4 8" />
              <circle cx="6" cy="16" r="2" />
              <circle cx="26" cy="16" r="2" />
            </svg>
          </span>
          <span>
            TRACE<span className="brand-light">FOLD</span>
          </span>
        </a>
        <div className="workspace-select">
          <div className="workspace-avatar">N</div>
          <span>
            <b>Northstar research</b>
            <small>Personal workspace</small>
          </span>
          <ChevronDown size={15} />
        </div>
        <div className="nav-caption">Workspace</div>
        <nav className="primary-nav" aria-label="Главная навигация">
          {[
            ["Overview", Activity],
            ["Investigations", Crosshair],
            ["Watchlist", Bell],
            ["Entities", Wallet],
          ].map(([label, Icon]) => (
            <button
              key={label as string}
              className={`nav-item ${activeTab === label ? "active" : ""}`}
              onClick={() => setActiveTab(label as string)}
            >
              <Icon size={17} />
              <span>{label as string}</span>
              {label === "Watchlist" && <span className="nav-count">6</span>}
            </button>
          ))}
        </nav>
        <div className="nav-caption network-caption">
          Networks{" "}
          <button
            aria-label="Настроить сети"
            onClick={() =>
              setNotice("Список сетей можно настроить в параметрах")
            }
          >
            <Plus size={15} />
          </button>
        </div>
        <div className="network-list">
          {(["Ethereum", "Bitcoin", "Base", "Arbitrum"] as Network[]).map(
            (network) => (
              <button
                key={network}
                className={`network-item ${activeNetwork === network ? "active" : ""}`}
                onClick={() => {
                  setActivityPage(0);
                  setActiveNetwork(
                    activeNetwork === network ? "All networks" : network,
                  );
                }}
              >
                <NetworkMark network={network} />
                <span>{network}</span>
                <i />
              </button>
            ),
          )}
        </div>
        <div className="sidebar-spacer" />
        <div className="usage-card">
          <div className="usage-head">
            <span>Data sources</span>
            <span className="source-count">
              {apiStatus === "online"
                ? `${trackedAddresses.length} tracked`
                : "API unavailable"}
            </span>
          </div>
          <p>
            Saved history loads on demand. Background monitoring is not enabled.
          </p>
          <button
            onClick={() =>
              setNotice("No chain data provider is configured yet.")
            }
          >
            Manage sources <ExternalLink size={12} />
          </button>
        </div>
        <button className="profile-button">
          <span className="profile-avatar">AK</span>
          <span>
            <b>Alex K.</b>
            <small>Free plan</small>
          </span>
          <MoreHorizontal size={18} />
        </button>
      </aside>

      <main className="main-area" id="overview">
        <header className="topbar">
          <button className="mobile-menu" aria-label="Открыть меню">
            <Menu size={19} />
          </button>
          <div className="breadcrumbs">
            <span>Workspace</span>
            <span>/</span>
            <b>{activeTab}</b>
          </div>
          <div className="top-actions">
            <span className={`live-status api-${apiStatus}`}>
              <i />
              {apiStatus === "online"
                ? "API + data stores connected"
                : apiStatus === "checking"
                  ? "Checking API + data stores"
                  : "Local API unavailable"}
            </span>
            <button
              className="icon-button"
              aria-label="Уведомления"
              onClick={() => setNotice("Новых уведомлений нет")}
            >
              <Bell size={17} />
              <i />
            </button>
            <button
              className="icon-button"
              aria-label="Помощь"
              onClick={() => setNotice("Справка проекта — в документации")}
            >
              <CircleHelp size={17} />
            </button>
            <span className="top-divider" />
            <button className="avatar-small">AK</button>
          </div>
        </header>
        <div className="page-content">
          <div className="page-heading">
            <div>
              <div className="eyebrow">
                <span className="eyebrow-dot demo-dot" />
                Demo investigation
              </div>
              <h1>Wallet flow overview</h1>
              <p>Track how value moves across addresses and networks.</p>
            </div>
            <div className="heading-actions">
              <button
                className="button button-secondary"
                onClick={() => setSaved(!saved)}
              >
                {saved ? <ShieldCheck size={16} /> : <Download size={16} />}{" "}
                {saved ? "Saved to watchlist" : "Save investigation"}
              </button>
              <button className="button button-primary" onClick={openAddWallet}>
                <Plus size={17} /> Add wallet
              </button>
            </div>
          </div>
          <section className="stats-grid" aria-label="Сводка расследования">
            <article className="stat-card">
              <div className="stat-label">
                Tracked wallets <Wallet size={15} />
              </div>
              <div className="stat-value">
                {nodes.length}
                <span className="stat-change positive">
                  <Sparkles size={12} /> +3 today
                </span>
              </div>
              <div className="stat-foot">
                <span className="stat-bars">
                  <i />
                  <i />
                  <i />
                  <i />
                  <i />
                  <i />
                  <i />
                  <i />
                  <i />
                  <i />
                  <i />
                  <i />
                </span>
                <span>Across 4 networks</span>
              </div>
            </article>
            <article className="stat-card">
              <div className="stat-label">
                Connected flows <GitBranch size={15} />
              </div>
              <div className="stat-value">
                24<span className="stat-change positive">+12.8%</span>
              </div>
              <div className="stat-foot">
                <span className="mini-line">
                  <i />
                </span>
                <span>vs. previous 7 days</span>
              </div>
            </article>
            <article className="stat-card">
              <div className="stat-label">
                Total volume <Layers3 size={15} />
              </div>
              <div className="stat-value">
                $4.82M<span className="stat-change positive">+8.2%</span>
              </div>
              <div className="stat-foot">
                <span className="mini-line blue">
                  <i />
                </span>
                <span>Across tracked wallets</span>
              </div>
            </article>
            <article className="stat-card stat-signal">
              <div className="stat-label">
                Selected wallet risk <Crosshair size={15} />
              </div>
              <div className="stat-value">
                {selectedTrackedAddress &&
                !riskSignalsLoading &&
                !riskSignalsError
                  ? walletAssessment.score
                  : "—"}
                {selectedTrackedAddress &&
                  !riskSignalsLoading &&
                  !riskSignalsError && (
                    <>
                      <small className="score-suffix">/100</small>
                      <span className="signal-label">
                        <i /> {walletAssessment.level}
                      </span>
                    </>
                  )}
              </div>
              <div className="stat-foot">
                <span>
                  {riskSignalsLoading
                    ? "Loading selected wallet evidence"
                    : riskSignalsError
                      ? "Evidence could not be loaded"
                      : selectedTrackedAddress
                        ? `${walletAssessment.contributions.length} evidence records · model ${walletAssessment.modelVersion}`
                        : "Select a saved wallet to view its score"}
                </span>
                <button
                  aria-label="Подробнее о сигналах риска"
                  onClick={() =>
                    document
                      .getElementById("risk-signals")
                      ?.scrollIntoView({ behavior: "smooth" })
                  }
                >
                  <ExternalLink size={13} />
                </button>
              </div>
            </article>
          </section>

          <section className="graph-panel" aria-label="Карта переводов">
            <div className="panel-heading">
              <div>
                <h2>Address connections</h2>
                <p>Follow funds between wallets across chains</p>
              </div>
              <div className="graph-actions">
                <button
                  className="select-control"
                  onClick={() =>
                    setActiveNetwork(
                      activeNetwork === "All networks"
                        ? "Ethereum"
                        : "All networks",
                    )
                  }
                >
                  <Blocks size={15} />
                  {activeNetwork}
                  <ChevronDown size={14} />
                </button>
                <button className="select-control time-control">
                  <Clock3 size={15} />
                  Last 30 days
                  <ChevronDown size={14} />
                </button>
                <button
                  className="square-control"
                  aria-label="Фильтры"
                  onClick={() =>
                    setNotice("Фильтр: входящие и исходящие переводы")
                  }
                >
                  <SlidersHorizontal size={16} />
                </button>
                <button
                  className="square-control"
                  aria-label="Другие действия"
                  onClick={() => setNotice("Настройки графа сохранены")}
                >
                  <MoreHorizontal size={17} />
                </button>
              </div>
            </div>
            <div className="graph-legend">
              <span>
                <i className="legend-flow" /> Transaction flow
              </span>
              <span>
                <i className="legend-center" /> Starting address
              </span>
              <span>
                <i className="legend-alert" /> Flagged wallet
              </span>
              <button
                onClick={() => setNotice("Уровень 1: ближайшие контрагенты")}
              >
                <Filter size={13} /> 1 hop
              </button>
            </div>
            <div className="graph-canvas">
              <TransferGraph
                selected={selected}
                onSelect={setSelected}
                zoom={zoom}
              />
              <div className="graph-toolbar">
                <button
                  aria-label="Уменьшить"
                  onClick={() => setZoom(Math.max(0.7, zoom - 0.1))}
                >
                  <ZoomOut size={15} />
                </button>
                <span>{Math.round(zoom * 100)}%</span>
                <button
                  aria-label="Увеличить"
                  onClick={() => setZoom(Math.min(1.4, zoom + 0.1))}
                >
                  <ZoomIn size={15} />
                </button>
                <i />
                <button aria-label="Вписать в экран" onClick={() => setZoom(1)}>
                  <Focus size={15} />
                </button>
              </div>
              <button
                className="expand-button"
                aria-label="Расширить граф"
                onClick={() => setNotice("Граф масштабирован под экран")}
              >
                <Focus size={15} />
              </button>
              <div className="graph-attribution">
                Sample graph <span>·</span> No live updates
              </div>
            </div>
            <div className="graph-footer">
              <div>
                <span className="footer-icon">
                  <Link2 size={14} />
                </span>
                <span>
                  <b>{selectedNode.name}</b>
                  <small>
                    {selectedNode.address}{" "}
                    <button
                      aria-label="Скопировать адрес"
                      onClick={copyAddress}
                    >
                      <Copy size={11} />
                    </button>
                  </small>
                </span>
                <NetworkMark network={selectedNode.network} />
                <span className="footer-balance">
                  {selectedNode.balance}
                  <small>Balance</small>
                </span>
              </div>
              <button
                className="text-action"
                onClick={() =>
                  setNotice(`Открыта карточка ${selectedNode.address}`)
                }
              >
                View details <ExternalLink size={13} />
              </button>
            </div>
          </section>

          <section className="risk-panel" id="risk-signals">
            <div className="risk-heading">
              <div>
                <div className="risk-title">
                  <span className="risk-icon">
                    <Crosshair size={16} />
                  </span>
                  <h2>Wallet risk assessment</h2>
                  <span className="demo-badge">
                    MODEL {walletAssessment.modelVersion}
                  </span>
                </div>
                <p>{walletAssessment.disclaimer}</p>
              </div>
              <div className="risk-heading-actions">
                {selectedTrackedAddress && (
                  <button
                    className="button button-secondary"
                    onClick={refreshWalletRiskSignals}
                    disabled={riskSignalsLoading}
                  >
                    Refresh evidence
                  </button>
                )}
                <button
                  className="button button-secondary"
                  onClick={() => setNotice("Открыта методика оценки риска")}
                >
                  Scoring methodology <ExternalLink size={13} />
                </button>
              </div>
            </div>
            {!selectedTrackedAddress ? (
              <div className="address-state">
                Save an address to view its analyst-entered risk evidence.
              </div>
            ) : riskSignalsLoading ? (
              <div className="address-state" role="status">
                Loading evidence for{" "}
                {selectedTrackedAddress.label || selectedTrackedAddress.address}
                …
              </div>
            ) : riskSignalsError ? (
              <div className="address-state address-error" role="alert">
                <span>{riskSignalsError}</span>
                <button
                  className="text-action"
                  onClick={refreshWalletRiskSignals}
                >
                  Retry
                </button>
              </div>
            ) : (
              <>
                <div className="risk-content">
                  <div className={`risk-score score-${walletAssessment.level}`}>
                    <div
                      className="score-ring"
                      style={
                        {
                          "--score": `${walletAssessment.score}%`,
                        } as CSSProperties
                      }
                    >
                      <span>
                        {walletAssessment.score}
                        <small>/100</small>
                      </span>
                    </div>
                    <b>{walletAssessment.level} review priority</b>
                    <small>
                      {selectedTrackedAddress.label ||
                        selectedTrackedAddress.address}
                    </small>
                  </div>
                  <div className="risk-signal-list">
                    {walletAssessment.contributions.length === 0 ? (
                      <div className="address-state">
                        No analyst-entered evidence is recorded for this
                        address.
                      </div>
                    ) : (
                      walletAssessment.contributions.map((item) => {
                        const signal = item.signal;
                        const sourceHref = safeExternalLink(
                          signal.sourceReference,
                        );
                        return (
                          <article className="risk-signal" key={signal.id}>
                            <span
                              className={`signal-mark severity-${signal.severity}`}
                            >
                              <Activity size={14} />
                            </span>
                            <span className="signal-copy">
                              <b>{signal.title}</b>
                              <small>{signal.detail}</small>
                              <span className="signal-meta">
                                {signal.source} <i />{" "}
                                {signal.hops === 0
                                  ? "Direct evidence"
                                  : `${signal.hops} hop${signal.hops === 1 ? "" : "s"}`}{" "}
                                <i />{" "}
                                {new Date(signal.observedAt).toLocaleDateString(
                                  "en-US",
                                  { month: "short", day: "numeric" },
                                )}
                                {signal.sourceReference && (
                                  <>
                                    <i />{" "}
                                    {sourceHref ? (
                                      <a
                                        href={sourceHref}
                                        target="_blank"
                                        rel="noopener noreferrer"
                                      >
                                        Source record
                                      </a>
                                    ) : (
                                      <span>Source reference on file</span>
                                    )}
                                  </>
                                )}
                              </span>
                            </span>
                            <span className="signal-contribution">
                              <span
                                className={`severity-label severity-text-${signal.severity}`}
                              >
                                {signal.severity}
                              </span>
                              <small>+{item.contribution.toFixed(1)} pts</small>
                              <small>
                                {item.baseWeight} ×{" "}
                                {item.distanceFactor.toFixed(2)} hop ×{" "}
                                {item.categoryFactor.toFixed(2)}{" "}
                                {signal.category}
                              </small>
                            </span>
                          </article>
                        );
                      })
                    )}
                  </div>
                </div>
                <div className="risk-evidence-note">
                  <span>
                    {screeningResult?.status === "exact_match"
                      ? "Exact OFAC match included. Review the source entry."
                      : screeningResult?.status === "no_exact_match"
                        ? "OFAC snapshot checked: no exact match. This does not lower the score or establish safety."
                        : screeningResult?.status === "source_not_imported"
                          ? "OFAC snapshot is unavailable locally; sanctions coverage is incomplete."
                          : screeningResult?.status === "network_not_covered"
                            ? "This network is not covered by the current OFAC address mapping."
                            : "OFAC check has not been run for this address; sanctions evidence is not included."}
                  </span>
                  <button
                    className="text-action"
                    onClick={() => void screenSelectedAddress()}
                    disabled={screeningLoading}
                  >
                    {screeningLoading ? "Checking…" : "Run OFAC check"}
                  </button>
                </div>
              </>
            )}
            <div className="risk-footnote">
              <ShieldCheck size={14} />
              <span>
                {walletAssessment.disclaimer} Freshness and provider reliability
                are shown as context, not scored.
              </span>
              <button
                onClick={() =>
                  setNotice(
                    "Ни один сигнал не является юридическим заключением",
                  )
                }
              >
                How to read this score <ExternalLink size={12} />
              </button>
            </div>
          </section>

          <section
            className="saved-addresses-panel"
            aria-labelledby="saved-addresses-title"
          >
            <div className="activity-head">
              <div>
                <h2 id="saved-addresses-title">Saved addresses</h2>
                <p>Addresses stored in the investigation API</p>
              </div>
              <button
                className="text-action"
                onClick={() => void retryAddressLoad()}
                disabled={addressesLoading}
              >
                {addressesLoading ? "Loading…" : "Refresh list"}
              </button>
            </div>
            {apiStatus === "offline" ? (
              <div className="address-state address-error" role="alert">
                <span>
                  {addressesError ||
                    "The API is unavailable. Check the service and retry."}
                </span>
                <button
                  className="text-action"
                  onClick={() => void retryAddressLoad()}
                  disabled={addressesLoading}
                >
                  Retry
                </button>
              </div>
            ) : addressesLoading ? (
              <p className="address-state" role="status">
                Loading saved addresses…
              </p>
            ) : addressesError ? (
              <div className="address-state address-error" role="alert">
                <span>{addressesError}</span>
                <button
                  className="text-action"
                  onClick={() => void retryAddressLoad()}
                >
                  Retry
                </button>
              </div>
            ) : trackedAddresses.length === 0 ? (
              <p className="address-state">
                No addresses saved yet. Track a wallet to add it here.
              </p>
            ) : (
              <div className="address-management">
                <label
                  className="address-picker-label"
                  htmlFor="tracked-address-picker"
                >
                  Select an address
                </label>
                <select
                  id="tracked-address-picker"
                  value={selectedAddressId}
                  onChange={(event) => {
                    setSelectedAddressId(event.target.value);
                    setHistoryRecords(null);
                    setHistoryError("");
                    setScreeningResult(null);
                    setScreeningError("");
                  }}
                >
                  {trackedAddresses.map((address) => (
                    <option key={address.id} value={address.id}>
                      {address.label ? `${address.label} · ` : ""}
                      {address.address} ({address.network})
                    </option>
                  ))}
                </select>
                <div className="address-management-actions">
                  <button
                    className="button button-secondary"
                    onClick={() => void viewSavedHistory()}
                    disabled={!selectedTrackedAddress || historyLoading}
                  >
                    {historyLoading ? "Loading history…" : "View saved history"}
                  </button>
                  <button
                    className="button button-primary"
                    onClick={() => void syncSavedAddress()}
                    disabled={!selectedTrackedAddress || isSyncing}
                  >
                    {isSyncing ? "Syncing…" : "Sync wallet history"}
                  </button>
                </div>
                <p className="address-note">
                  {selectedTrackedAddress?.network === "bitcoin"
                    ? "Saved history is loaded on demand. Sync requests Bitcoin data from the configured Esplora provider."
                    : "Saved history is loaded on demand from Blockscout. Coverage is bounded to Ethereum, Base and Arbitrum One."}
                </p>
                {historyError && (
                  <div className="address-state address-error" role="alert">
                    <span>{historyError}</span>
                    <button
                      className="text-action"
                      onClick={() => void syncSavedAddress()}
                    >
                      Retry
                    </button>
                  </div>
                )}
              </div>
            )}
            <div className="screening-action">
              <div>
                <strong>OFAC SDN address check</strong>
                <p>
                  Exact match against the locally imported list snapshot. It
                  does not check indirect exposure.
                </p>
              </div>
              <button
                className="button button-secondary"
                onClick={() => void screenSelectedAddress()}
                disabled={!selectedTrackedAddress || screeningLoading}
              >
                {screeningLoading ? "Checking address…" : "Check address"}
              </button>
            </div>
            {screeningError && (
              <div className="address-state address-error" role="alert">
                <span>{screeningError}</span>
                <button
                  className="text-action"
                  onClick={() => void screenSelectedAddress()}
                >
                  Retry
                </button>
              </div>
            )}
            {screeningResult && (
              <div
                className={`screening-result status-${screeningResult.status}`}
                role="status"
              >
                <div className="screening-result-heading">
                  <strong>
                    {screeningResult.status === "exact_match" &&
                      "Exact match found"}
                    {screeningResult.status === "no_exact_match" &&
                      "No exact match in this snapshot"}
                    {screeningResult.status === "source_not_imported" &&
                      "OFAC snapshot is not imported"}
                    {screeningResult.status === "network_not_covered" &&
                      "Network is not covered"}
                  </strong>
                  {screeningResult.source && (
                    <small>
                      {screeningResult.source.source_name} · published{" "}
                      {screeningResult.source.published_at} · imported{" "}
                      {new Date(
                        screeningResult.source.imported_at,
                      ).toLocaleString()}
                    </small>
                  )}
                </div>
                {screeningResult.matches.map((match) => (
                  <div
                    className="screening-match"
                    key={`${match.entity_uid}:${match.currency_code}:${match.address}`}
                  >
                    <strong>{match.entity_name}</strong>
                    <span>
                      {match.currency_code} · ID {match.entity_uid}
                    </span>
                    {match.programs.length > 0 && (
                      <small>{match.programs.join(", ")}</small>
                    )}
                  </div>
                ))}
                <p>{screeningResult.explanation}</p>
              </div>
            )}
            <div className="flow-graph-action">
              <div>
                <strong>Trace direct wallet links</strong>
                <p>
                  Loads saved EVM history and connects observed sender and
                  recipient addresses.
                </p>
              </div>
              <button
                className="button button-secondary"
                onClick={() => void buildWalletConnections()}
                disabled={
                  flowLoading ||
                  addressesLoading ||
                  !trackedAddresses.some((address) =>
                    ["ethereum", "base", "arbitrum"].includes(address.network),
                  )
                }
              >
                {flowLoading ? "Loading histories…" : "Build transfer links"}
              </button>
            </div>
            {flowError && (
              <div className="address-state address-error" role="alert">
                <span>{flowError}</span>
                <button
                  className="text-action"
                  onClick={() => void buildWalletConnections()}
                >
                  Retry
                </button>
              </div>
            )}
            {flowHistories && <WalletFlowGraph histories={flowHistories} />}
          </section>

          <section className="activity-panel">
            <div className="activity-head">
              <div>
                <h2>Recent transactions</h2>
                <p>Saved observations across tracked addresses</p>
              </div>
              <button
                className="text-action"
                onClick={() =>
                  setActivityRefreshKey((revision) => revision + 1)
                }
                disabled={
                  activityStatus === "loading" || apiStatus !== "online"
                }
              >
                Reload saved history
              </button>
            </div>
            <div className="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th>Transaction</th>
                    <th>Flow</th>
                    <th>Amount</th>
                    <th>Network</th>
                    <th>Time</th>
                    <th>Status</th>
                    <th>Tracked wallet · source</th>
                  </tr>
                </thead>
                <tbody>
                  {activityStatus === "loading" && (
                    <tr>
                      <td
                        colSpan={7}
                        className="activity-message"
                        role="status"
                      >
                        Loading saved transaction observations…
                      </td>
                    </tr>
                  )}
                  {activityStatus === "error" && (
                    <tr>
                      <td
                        colSpan={7}
                        className="activity-message activity-error"
                      >
                        {activityError}
                        <button
                          className="text-action"
                          onClick={() =>
                            setActivityRefreshKey((revision) => revision + 1)
                          }
                          disabled={apiStatus !== "online"}
                        >
                          Retry
                        </button>
                      </td>
                    </tr>
                  )}
                  {activityStatus === "ready" &&
                    pagedTransfers.map((transfer) => (
                      <tr key={transfer.id}>
                        <td>
                          <span className="tx-id">
                            {transfer.hash.length > 24
                              ? `${transfer.hash.slice(0, 12)}…${transfer.hash.slice(-8)}`
                              : transfer.hash}
                          </span>
                        </td>
                        <td>
                          <span className="flow-direction">
                            {transfer.direction === "outbound" ? (
                              <ArrowUpRight size={14} />
                            ) : transfer.direction === "both" ? (
                              <GitBranch size={14} />
                            ) : (
                              <ArrowDownLeft size={14} />
                            )}
                            <span>
                              {transfer.from}
                              <small>to {transfer.to}</small>
                            </span>
                          </span>
                        </td>
                        <td>
                          <b className="amount">
                            {transfer.amount} <small>{transfer.asset}</small>
                          </b>
                        </td>
                        <td>
                          <span className="table-network">
                            <NetworkMark network={transfer.network} />
                            {transfer.network}
                          </span>
                        </td>
                        <td className="time-cell">
                          {formatFeedTime(transfer.occurredAt)}
                        </td>
                        <td>
                          <span
                            className={`status ${transfer.confirmed ? "confirmed" : "pending"}`}
                          >
                            <i />
                            {transfer.confirmed ? "Confirmed" : "Unconfirmed"}
                          </span>
                        </td>
                        <td>
                          <span className="activity-source">
                            <strong>{transfer.trackedWallet}</strong>
                            <small>{transfer.provider}</small>
                          </span>
                        </td>
                      </tr>
                    ))}
                </tbody>
              </table>
              {activityStatus === "ready" && visibleTransfers.length === 0 && (
                <div className="empty-state">
                  {trackedAddresses.length === 0
                    ? "No tracked wallets yet. Add an address to begin."
                    : transactionFeed.length === 0
                      ? "No saved observations yet. Sync a tracked wallet to load its history."
                      : "No saved observations for this network."}
                </div>
              )}
              {activityStatus === "ready" && activityPageCount > 1 && (
                <div className="feed-pagination">
                  <span>
                    Showing {currentActivityPage * TRANSACTIONS_PER_PAGE + 1}–
                    {Math.min(
                      (currentActivityPage + 1) * TRANSACTIONS_PER_PAGE,
                      visibleTransfers.length,
                    )}{" "}
                    of {visibleTransfers.length} observations
                  </span>
                  <div>
                    <button
                      className="text-action"
                      onClick={() => setActivityPage(currentActivityPage - 1)}
                      disabled={currentActivityPage === 0}
                    >
                      Previous
                    </button>
                    <span>
                      {currentActivityPage + 1} / {activityPageCount}
                    </span>
                    <button
                      className="text-action"
                      onClick={() => setActivityPage(currentActivityPage + 1)}
                      disabled={currentActivityPage >= activityPageCount - 1}
                    >
                      Next
                    </button>
                  </div>
                </div>
              )}
            </div>
          </section>
          <footer className="page-footer">
            <span>
              <span className="footer-brand-mark">T</span> Tracefold{" "}
              <span>Open-source investigation workspace</span>
            </span>
            <span>
              Overview graph uses sample data <span>·</span>{" "}
              <button
                onClick={() =>
                  setNotice("Источники данных подключаются в настройках")
                }
              >
                Manage data sources
              </button>
            </span>
          </footer>
        </div>
      </main>

      {historyRecords && (
        <div
          className="modal-backdrop"
          onMouseDown={(event) =>
            event.target === event.currentTarget && setHistoryRecords(null)
          }
        >
          <section
            className="add-modal history-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="history-title"
          >
            <div className="modal-top">
              <span className="modal-icon">
                <Activity size={18} />
              </span>
              <button
                className="icon-button"
                aria-label="Close history"
                onClick={() => setHistoryRecords(null)}
              >
                <X size={17} />
              </button>
            </div>
            <h2 id="history-title">Observed wallet history</h2>
            <p>
              {historyRecords.length} saved transaction observations. Source:{" "}
              {historyRecords[0]?.provider ?? "Configured provider"}.{" "}
              {historyRecords[0]?.network === "bitcoin"
                ? "Bitcoin values show address-level inputs and outputs, not transfers between identified owners."
                : "EVM observations include the transaction endpoints and token event details returned by the provider."}
            </p>
            {historyRecords.length === 0 ? (
              <div className="history-empty">
                No saved transactions were returned for this address.
              </div>
            ) : (
              <ul className="history-list">
                {historyRecords.map((record) => (
                  <li key={record.id}>
                    <div className="history-record-top">
                      <code>
                        {record.transaction_hash.slice(0, 12)}…
                        {record.transaction_hash.slice(-8)}
                      </code>
                      <span
                        className={`history-status ${record.confirmed ? "confirmed" : "pending"}`}
                      >
                        {record.confirmed ? "Confirmed" : "Mempool"}
                      </span>
                    </div>
                    <small>
                      {record.block_time
                        ? new Date(record.block_time).toLocaleString()
                        : "Not confirmed"}
                      {record.block_height
                        ? ` · Block ${record.block_height}`
                        : ""}
                      {` · ${record.provider}`}
                    </small>
                    {record.network !== "bitcoin" && (
                      <small className="history-endpoints">
                        {record.from_address ?? "Unknown sender"} →{" "}
                        {record.to_address ?? "Unknown recipient"}
                        {record.token_contract_address
                          ? ` · ${record.token_contract_address}`
                          : ""}
                      </small>
                    )}
                    <div className="history-amounts">
                      {record.received_amount !== "0" && (
                        <span className="history-received">
                          Received{" "}
                          {formatObservedAmount(record, record.received_amount)}{" "}
                          {record.asset}
                        </span>
                      )}
                      {record.spent_amount !== "0" && (
                        <span className="history-spent">
                          Spent{" "}
                          {formatObservedAmount(record, record.spent_amount)}{" "}
                          {record.asset}
                        </span>
                      )}
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </section>
        </div>
      )}

      {showAdd && (
        <div
          className="modal-backdrop"
          onMouseDown={(event) =>
            event.target === event.currentTarget && setShowAdd(false)
          }
        >
          <section
            className="add-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="add-title"
          >
            <div className="modal-top">
              <span className="modal-icon">
                <Wallet size={18} />
              </span>
              <button
                className="icon-button"
                aria-label="Закрыть"
                onClick={() => setShowAdd(false)}
                disabled={isSubmitting}
              >
                <X size={17} />
              </button>
            </div>
            <h2 id="add-title">Add a wallet</h2>
            <p>
              Paste an address to identify its network and follow new transfers.
            </p>
            <form
              onSubmit={submitAddress}
              aria-busy={isSubmitting}
              aria-describedby={formError ? "wallet-form-error" : undefined}
            >
              <label htmlFor="wallet-address">Wallet address</label>
              <div className="address-input">
                <input
                  id="wallet-address"
                  autoFocus
                  placeholder="0x…, bc1…, or address"
                  required
                  minLength={8}
                  value={query}
                  onChange={(event) => handleSearch(event.target.value)}
                  disabled={isSubmitting}
                />
                <Search size={16} />
              </div>
              <div
                className={`detection-note detection-${addressDetection.status}`}
              >
                <span className="detection-spark">
                  <Sparkles size={13} />
                </span>
                <span>
                  <b>Network detection</b>
                  <small>{addressDetection.message}</small>
                </span>
                {addressDetection.status === "detected" && (
                  <span className="network-detected">
                    <NetworkMark network="Bitcoin" /> Bitcoin mainnet
                  </span>
                )}
                {addressDetection.status === "ambiguous" && (
                  <select
                    id="wallet-network"
                    className="network-detected"
                    aria-label="Select EVM network"
                    value={networkChoice}
                    onChange={(event) => {
                      setNetworkChoice(event.target.value as SupportedNetwork);
                      setFormError("");
                    }}
                    required
                    disabled={isSubmitting}
                  >
                    <option value="">Select network</option>
                    {addressDetection.candidates.map((network) => (
                      <option key={network} value={network}>
                        {network}
                      </option>
                    ))}
                  </select>
                )}
              </div>
              <label htmlFor="wallet-label">
                Label <span>Optional</span>
              </label>
              <input
                id="wallet-label"
                className="label-input"
                placeholder="e.g. Treasury wallet"
                maxLength={80}
                value={walletLabel}
                onChange={(event) => setWalletLabel(event.target.value)}
                disabled={isSubmitting}
              />
              <div className="modal-note">
                <ShieldCheck size={14} /> Watch-only. No private keys are
                requested or stored.
              </div>
              {formError && (
                <p className="form-error" id="wallet-form-error" role="alert">
                  {formError}
                </p>
              )}
              <div className="modal-actions">
                <button
                  type="button"
                  className="button button-secondary"
                  onClick={() => setShowAdd(false)}
                  disabled={isSubmitting}
                >
                  Cancel
                </button>
                <button
                  className="button button-primary"
                  type="submit"
                  disabled={
                    isSubmitting ||
                    (addressDetection.status !== "detected" &&
                      !(
                        addressDetection.status === "ambiguous" && networkChoice
                      ))
                  }
                >
                  {isSubmitting ? (
                    <>
                      <span className="button-spinner" aria-hidden="true" />
                      Saving wallet…
                    </>
                  ) : (
                    <>
                      <Plus size={15} /> Track wallet
                    </>
                  )}
                </button>
              </div>
            </form>
          </section>
        </div>
      )}
      {notice && (
        <div className="toast" role="status">
          <span className="toast-check">
            <ShieldCheck size={15} />
          </span>
          {notice}
          <button aria-label="Скрыть уведомление" onClick={() => setNotice("")}>
            <X size={14} />
          </button>
        </div>
      )}
    </div>
  );
}

export default App;
