import {
  useCallback,
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
  CircleHelp,
  Crosshair,
  ExternalLink,
  GitBranch,
  Menu,
  Plus,
  Search,
  ShieldCheck,
  Sparkles,
  Wallet,
  X,
} from "lucide-react";
import { buildWalletAssessment } from "./features/risk/scoreWallet";
import {
  detectAddressNetwork,
  type AddressDetection,
} from "./features/networks/detectAddressNetwork";
import {
  getNetworkName,
  getNetworkSyncAvailability,
  hasAddressScreeningMapping,
  isEvmNetworkId,
  LEGACY_NETWORKS,
  type NetworkCatalog,
} from "./features/networks/networkCatalog";
import {
  ApiRequestError,
  checkApiHealth,
  createInvestigation,
  listGraphSnapshots,
  listInvestigations,
  listTrackedWalletHistories,
  listTrackedWallets,
  listSupportedNetworks,
  listWalletHistory,
  listWalletRiskSignals,
  screenWalletAddress,
  saveGraphSnapshot,
  syncWalletHistory,
  trackWallet,
  type ObservedTransfer,
  type GraphSnapshot,
  type Investigation,
  type AddressScreeningResult,
  type StoredRiskSignal,
  type TrackedAddress,
  type WalletHistory,
} from "./features/investigations/api";
import { InvestigationManager } from "./features/investigations/InvestigationManager";
import { WatchlistPage } from "./features/watchlist/WatchlistPage";
import {
  readStoredInvestigationId,
  resolveActiveInvestigationId,
  storeInvestigationId,
} from "./features/investigations/activeInvestigation";
import { WalletFlowGraph } from "./features/graph/WalletFlowGraph";
import { formatObservedAmount } from "./features/investigations/formatAssetAmount";
import {
  buildTransactionFeed,
  formatFeedTime,
  getTransferRiskPlaceholder,
} from "./features/investigations/transactionFeed";
import { syncCounterpartyImmediately } from "./features/investigations/syncCounterparty";
import {
  getActiveInvestigationData,
  getDashboardSummary,
} from "./features/overview/dashboardSummary";
import "./App.css";

type Network = string;
const TRANSACTIONS_PER_PAGE = 50;
const PRIMARY_NAVIGATION = [
  { label: "Overview", Icon: Activity, available: true },
  { label: "Investigations", Icon: Crosshair, available: true },
  { label: "Watchlist", Icon: Bell, available: true },
  { label: "Entities", Icon: Wallet, available: false },
] as const;
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
const networkMeta: Record<string, { color: string }> = {
  Ethereum: { color: "#a98cff" },
  ethereum: { color: "#a98cff" },
  Bitcoin: { color: "#f2a84a" },
  bitcoin: { color: "#f2a84a" },
  Base: { color: "#5595ff" },
  base: { color: "#5595ff" },
  Arbitrum: { color: "#64c8fa" },
  "Arbitrum One": { color: "#64c8fa" },
  arbitrum: { color: "#64c8fa" },
  Solana: { color: "#a98cff" },
  solana: { color: "#a98cff" },
  sui: { color: "#68d5c2" },
  Sui: { color: "#68d5c2" },
  TRON: { color: "#ff5964" },
  tron: { color: "#ff5964" },
  "XRP Ledger": { color: "#20a8d8" },
  "xrp ledger": { color: "#20a8d8" },
  xrpl: { color: "#20a8d8" },
};

function networkColor(network: string): string {
  return (
    networkMeta[network]?.color ??
    (isEvmNetworkId(network) ? "#7aabff" : "#a3b0a7")
  );
}

function NetworkShape({ network }: { network: Network }) {
  const normalizedNetwork = network.toLowerCase();
  return (
    <g
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {normalizedNetwork === "bitcoin" && (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M10 6.5v11m4-11v11M8.8 9h5a2 2 0 0 1 0 4h-5m0 0h5.7a2 2 0 0 1 0 4H8.8" />
        </>
      )}
      {normalizedNetwork === "ethereum" && (
        <>
          <path d="m12 2.5 6.2 9.3L12 15l-6.2-3.2L12 2.5Z" />
          <path d="m5.8 13.1 6.2 3.2 6.2-3.2L12 21.5l-6.2-8.4Z" />
        </>
      )}
      {normalizedNetwork === "sui" && (
        <>
          <path d="m12 2.5 7 4v8l-7 4-7-4v-8l7-4Z" />
          <path d="m8 9 4 2.3L16 9m-8 4 4 2.3 4-2.3" />
          <path d="M12 11.3v4" />
        </>
      )}
      {normalizedNetwork === "base" && (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M20.5 9.5H12a2.5 2.5 0 1 0 0 5h8.5" />
        </>
      )}
      {(normalizedNetwork === "arbitrum" ||
        normalizedNetwork === "arbitrum one") && (
        <>
          <path d="m12 2.5 8 4.6v9.8l-8 4.6-8-4.6V7.1l8-4.6Z" />
          <path d="m8 8 4 8m4-8-4 8m-1.8-2.4h3.6" />
        </>
      )}
      {normalizedNetwork === "solana" && (
        <>
          <path
            d="M6 5.5h12.5l-2.5 2.4H3.5L6 5.5Z"
            fill="currentColor"
            stroke="none"
          />
          <path
            d="M6 10.8h14.5L18 13.2H3.5l2.5-2.4Z"
            fill="currentColor"
            stroke="none"
          />
          <path
            d="M6 16.1h12.5L16 18.5H3.5l2.5-2.4Z"
            fill="currentColor"
            stroke="none"
          />
        </>
      )}
      {normalizedNetwork === "tron" && (
        <>
          <path d="M4 4.5 20 8 12 20 4 4.5Z" />
          <path d="m7.1 10.3 9.8 0M9.4 13.1l5.2 0" />
          <circle cx="4" cy="4.5" r="1.1" fill="currentColor" stroke="none" />
          <circle cx="20" cy="8" r="1.1" fill="currentColor" stroke="none" />
          <circle cx="12" cy="20" r="1.1" fill="currentColor" stroke="none" />
        </>
      )}
      {(normalizedNetwork === "xrpl" || normalizedNetwork === "xrp ledger") && (
        <>
          <path d="M4 6h4.2c2.2 0 3.2 1.1 3.8 2.1l.1.2c.6 1 1.3 1.7 2.7 1.7h5.2" />
          <path d="M4 18h4.2c2.2 0 3.2-1.1 3.8-2.1l.1-.2c.6-1 1.3-1.7 2.7-1.7h5.2" />
          <circle cx="4" cy="6" r="1.2" fill="currentColor" />
          <circle cx="20" cy="10" r="1.2" fill="currentColor" />
          <circle cx="4" cy="18" r="1.2" fill="currentColor" />
          <circle cx="20" cy="14" r="1.2" fill="currentColor" />
        </>
      )}
      {!networkMeta[normalizedNetwork] && (
        <>
          <circle cx="12" cy="12" r="9" />
          <path d="M7 12h10M12 7v10" />
        </>
      )}
    </g>
  );
}

function NetworkMark({ network }: { network: Network }) {
  return (
    <span
      className="network-mark"
      data-network={network.toLowerCase()}
      style={{ "--network": networkColor(network) } as CSSProperties}
      aria-label={network}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <NetworkShape network={network} />
      </svg>
    </span>
  );
}

function App() {
  const [activeNetwork, setActiveNetwork] = useState<"All networks" | Network>(
    "All networks",
  );
  const [showAdd, setShowAdd] = useState(false);
  const [query, setQuery] = useState("");
  const [walletLabel, setWalletLabel] = useState("");
  const [networkChoice, setNetworkChoice] = useState("");
  const [networkCatalog, setNetworkCatalog] = useState<NetworkCatalog>({
    networks: LEGACY_NETWORKS,
  });
  const [networkCatalogLoading, setNetworkCatalogLoading] = useState(true);
  const [networkCatalogError, setNetworkCatalogError] = useState("");
  const [networkCatalogRetry, setNetworkCatalogRetry] = useState(0);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [formError, setFormError] = useState("");
  const [apiStatus, setApiStatus] = useState<"checking" | "online" | "offline">(
    "checking",
  );
  const [addressDetection, setAddressDetection] = useState<AddressDetection>({
    status: "empty",
    message: "Paste a wallet address.",
  });
  const [notice, setNotice] = useState("");
  const [trackedAddresses, setTrackedAddresses] = useState<TrackedAddress[]>(
    [],
  );
  const [investigations, setInvestigations] = useState<Investigation[]>([]);
  const [activeInvestigationId, setActiveInvestigationId] = useState<
    string | null
  >(() => readStoredInvestigationId());
  const [isCreatingInvestigation, setIsCreatingInvestigation] = useState(false);
  const [addressesLoading, setAddressesLoading] = useState(true);
  const [addressesError, setAddressesError] = useState("");
  const [selectedAddressId, setSelectedAddressId] = useState("");
  const [isSyncing, setIsSyncing] = useState(false);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyError, setHistoryError] = useState("");
  const [historyRecords, setHistoryRecords] = useState<
    ObservedTransfer[] | null
  >(null);
  const [flowLoading, setFlowLoading] = useState(false);
  const [flowError, setFlowError] = useState("");
  const [flowSnapshots, setFlowSnapshots] = useState<GraphSnapshot[]>([]);
  const [openedFlowSnapshot, setOpenedFlowSnapshot] =
    useState<GraphSnapshot | null>(null);
  const [savingFlowSnapshot, setSavingFlowSnapshot] = useState(false);
  const [flowSnapshotError, setFlowSnapshotError] = useState("");
  const [walletInvestigationId, setWalletInvestigationId] = useState("");
  const [trackSource, setTrackSource] = useState<"manual" | "counterparty">(
    "manual",
  );
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
  const networkNames = useMemo(
    () =>
      Object.fromEntries(
        networkCatalog.networks.map(({ id, name }) => [id, name]),
      ),
    [networkCatalog.networks],
  );
  const evmNetworks = useMemo(
    () =>
      networkCatalog.networks
        .filter(({ family }) => family === "evm")
        .sort((left, right) => left.name.localeCompare(right.name)),
    [networkCatalog.networks],
  );
  const addressDetectionNetworks = useMemo(() => {
    if (addressDetection.status !== "ambiguous") return [];
    return addressDetection.family === "EVM"
      ? evmNetworks
      : addressDetection.family === "Hex-compatible"
        ? [
            ...evmNetworks,
            ...networkCatalog.networks.filter(
              ({ family }) => family === "aptos" || family === "sui",
            ),
          ]
        : networkCatalog.networks.filter(({ family, id }) =>
            addressDetection.family === "Tron-compatible"
              ? family === "tron"
              : addressDetection.family === "Solana-compatible"
                ? family === "solana"
                : addressDetection.family === "Cardano-compatible"
                  ? id === "cardano"
                  : addressDetection.family === "Move-compatible"
                    ? family === "aptos" || family === "sui"
                    : family === "solana",
          );
  }, [addressDetection, evmNetworks, networkCatalog.networks]);
  const selectedNetworkChoice = networkCatalog.networks.find(
    ({ id }) => id === networkChoice,
  );
  const networkFilterOptions = useMemo(() => {
    const networkIds = new Set([
      ...LEGACY_NETWORKS.map(({ id }) => id),
      ...trackedAddresses
        .filter(
          ({ investigation_id }) => investigation_id === activeInvestigationId,
        )
        .map(({ network }) => network.toLowerCase()),
    ]);
    return [...networkIds].map((id) => ({
      id,
      name: getNetworkName(id, networkCatalog.networks),
    }));
  }, [activeInvestigationId, trackedAddresses, networkCatalog.networks]);
  const selectedTrackedAddress = trackedAddresses.find(
    (address) =>
      address.id === selectedAddressId &&
      address.investigation_id === activeInvestigationId,
  );
  const activeInvestigationAddresses = useMemo(
    () =>
      trackedAddresses.filter(
        ({ investigation_id }) => investigation_id === activeInvestigationId,
      ),
    [trackedAddresses, activeInvestigationId],
  );
  const activeInvestigationData = useMemo(
    () =>
      getActiveInvestigationData(
        trackedAddresses,
        activityHistories,
        activeInvestigationId,
      ),
    [trackedAddresses, activityHistories, activeInvestigationId],
  );
  const flowHistories =
    activityStatus === "ready" ? activeInvestigationData.histories : null;
  const addressCountsByInvestigation = useMemo(
    () =>
      new Map(
        investigations.map(({ id }) => [
          id,
          trackedAddresses.filter((address) => address.investigation_id === id)
            .length,
        ]),
      ),
    [investigations, trackedAddresses],
  );
  const visibleFlowSnapshots = useMemo(
    () =>
      flowSnapshots.filter(
        ({ investigation_id }) =>
          investigation_id === activeInvestigationData.investigationId,
      ),
    [flowSnapshots, activeInvestigationData.investigationId],
  );
  const activeFlowSnapshot =
    openedFlowSnapshot?.investigation_id ===
    activeInvestigationData.investigationId
      ? openedFlowSnapshot
      : null;
  const dashboardSummary = useMemo(
    () =>
      getDashboardSummary(
        activeInvestigationData.addresses,
        activityStatus === "ready" ? activeInvestigationData.histories : [],
      ),
    [activeInvestigationData, activityStatus],
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
  const selectedObservedTransfers = useMemo(
    () =>
      activityStatus === "ready" && selectedTrackedAddress
        ? (activityHistories.find(
            (history) => history.address.id === selectedTrackedAddress.id,
          )?.transfers ?? [])
        : [],
    [activityHistories, activityStatus, selectedTrackedAddress],
  );
  const transactionFeed = useMemo(
    () =>
      buildTransactionFeed(
        activityHistories,
        networkNames,
        selectedTrackedAddress
          ? {
              trackedAddressId: selectedTrackedAddress.id,
              signals: walletRiskSignals ?? [],
              screening: screeningResult,
            }
          : null,
      ),
    [
      activityHistories,
      networkNames,
      selectedTrackedAddress,
      walletRiskSignals,
      screeningResult,
    ],
  );
  const visibleTransfers = transactionFeed.filter(
    (transfer) =>
      activeNetwork === "All networks" || transfer.networkId === activeNetwork,
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
    () =>
      buildWalletAssessment(
        walletRiskSignals ?? [],
        screeningResult,
        selectedObservedTransfers,
      ),
    [walletRiskSignals, screeningResult, selectedObservedTransfers],
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
    void listSupportedNetworks()
      .then((catalog) => {
        if (!isCurrent) return;
        const mergedById = new Map(
          LEGACY_NETWORKS.map((network) => [network.id, network]),
        );
        catalog.networks.forEach((network) =>
          mergedById.set(network.id, network),
        );
        setNetworkCatalog({
          ...catalog,
          networks: [...mergedById.values()],
        });
        setNetworkCatalogError("");
      })
      .catch((error: unknown) => {
        if (!isCurrent) return;
        setNetworkCatalog({ networks: LEGACY_NETWORKS });
        setNetworkCatalogError(
          error instanceof ApiRequestError
            ? error.message
            : "The network catalog response was invalid.",
        );
      })
      .finally(() => {
        if (isCurrent) setNetworkCatalogLoading(false);
      });
    return () => {
      isCurrent = false;
    };
  }, [networkCatalogRetry]);

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

  const loadTrackedAddresses = useCallback(async () => {
    setAddressesError("");
    try {
      const loadedInvestigations = await listInvestigations();
      const addresses = await listTrackedWallets(loadedInvestigations);
      const nextInvestigationId = resolveActiveInvestigationId(
        loadedInvestigations,
        activeInvestigationId ?? readStoredInvestigationId(),
      );
      setInvestigations(loadedInvestigations);
      setTrackedAddresses(addresses);
      setActiveInvestigationId(nextInvestigationId);
      storeInvestigationId(nextInvestigationId);
      setSelectedAddressId((current) =>
        addresses.some(
          (address) =>
            address.id === current &&
            address.investigation_id === nextInvestigationId,
        )
          ? current
          : (addresses.find(
              (address) => address.investigation_id === nextInvestigationId,
            )?.id ?? ""),
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
  }, [activeInvestigationId]);

  const selectInvestigation = (investigationId: string) => {
    if (!investigations.some(({ id }) => id === investigationId)) return;
    setActiveInvestigationId(investigationId);
    storeInvestigationId(investigationId);
    setSelectedAddressId(
      trackedAddresses.find(
        ({ investigation_id }) => investigation_id === investigationId,
      )?.id ?? "",
    );
    setActiveNetwork("All networks");
    setActivityPage(0);
    setHistoryRecords(null);
    setHistoryError("");
    setScreeningResult(null);
    setScreeningError("");
    setOpenedFlowSnapshot(null);
    setFlowError("");
    setFlowSnapshotError("");
    setActivityError("");
    setActivityStatus("loading");
    setActiveTab("Overview");
  };

  const handleCreateInvestigation = async (name: string) => {
    setIsCreatingInvestigation(true);
    try {
      const created = await createInvestigation(name);
      setInvestigations((current) => [
        created,
        ...current.filter(({ id }) => id !== created.id),
      ]);
      setActiveInvestigationId(created.id);
      storeInvestigationId(created.id);
      setSelectedAddressId("");
      setActivityHistories([]);
      setActivityError("");
      setActivityStatus("ready");
      setHistoryRecords(null);
      setHistoryError("");
      setScreeningResult(null);
      setScreeningError("");
      setFlowSnapshots((current) =>
        current.filter(
          ({ investigation_id }) => investigation_id === created.id,
        ),
      );
      setOpenedFlowSnapshot(null);
      setActivityPage(0);
      setActiveNetwork("All networks");
      setFlowError("");
      setFlowSnapshotError("");
      setNotice(`Created investigation “${created.name}”.`);
    } finally {
      setIsCreatingInvestigation(false);
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
  }, [apiStatus, loadTrackedAddresses]);

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
      if (activeInvestigationAddresses.length === 0) {
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
        const histories = await listTrackedWalletHistories(
          activeInvestigationAddresses,
        );
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
    activeInvestigationAddresses,
    addressesError,
    addressesLoading,
    apiStatus,
  ]);

  useEffect(() => {
    if (activityStatus !== "ready") return;

    let isCurrent = true;
    const { investigationId } = activeInvestigationData;
    if (!investigationId) {
      return;
    }

    void listGraphSnapshots(investigationId)
      .then((snapshots) => {
        if (!isCurrent) return;
        setFlowSnapshots(snapshots);
        setFlowError("");
      })
      .catch((error: unknown) => {
        if (!isCurrent) return;
        setFlowError(
          error instanceof ApiRequestError
            ? error.message
            : "Could not load saved graph snapshots. Try again.",
        );
      });
    return () => {
      isCurrent = false;
    };
  }, [activityStatus, activeInvestigationData]);

  const handleSearch = (value: string) => {
    setQuery(value);
    setNetworkChoice("");
    setFormError("");
  };

  const refreshInvestigationGraph = async (
    investigationId: string,
    addresses = trackedAddresses,
  ) => {
    const investigationAddresses = addresses.filter(
      (address) => address.investigation_id === investigationId,
    );
    const [histories, snapshots] = await Promise.all([
      listTrackedWalletHistories(investigationAddresses),
      listGraphSnapshots(investigationId),
    ]);
    setActivityHistories((current) => [
      ...current.filter(
        (history) => history.address.investigation_id !== investigationId,
      ),
      ...histories,
    ]);
    setFlowSnapshots(snapshots);
    setOpenedFlowSnapshot(null);
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
          ? "Select the network before tracking this wallet"
          : "Enter a valid address on a supported network",
      );
      return;
    }

    setFormError("");
    setIsSubmitting(true);
    const shouldSyncCounterparty =
      trackSource === "counterparty" && Boolean(walletInvestigationId);
    try {
      const trackedAddress = await trackWallet({
        network,
        address: query,
        label: walletLabel,
        investigationId:
          (trackSource === "counterparty" ? walletInvestigationId : null) ||
          activeInvestigationId ||
          undefined,
      });
      if (!activeInvestigationId && trackSource === "manual") {
        try {
          const loadedInvestigations = await listInvestigations();
          setInvestigations(loadedInvestigations);
          setActiveInvestigationId(trackedAddress.investigation_id);
          storeInvestigationId(trackedAddress.investigation_id);
        } catch {
          setAddressesError(
            "Wallet saved, but the investigation list could not be refreshed.",
          );
          setActiveInvestigationId(trackedAddress.investigation_id);
          storeInvestigationId(trackedAddress.investigation_id);
        }
      }
      const nextAddresses = [
        trackedAddress,
        ...trackedAddresses.filter(
          (address) => address.id !== trackedAddress.id,
        ),
      ];
      setTrackedAddresses(nextAddresses);
      setSelectedAddressId(trackedAddress.id);
      setHistoryRecords(null);
      setScreeningResult(null);
      setShowAdd(false);
      setQuery("");
      setWalletLabel("");
      setNetworkChoice("");
      setWalletInvestigationId("");
      setTrackSource("manual");
      if (shouldSyncCounterparty) {
        setIsSyncing(true);
        setHistoryError("");
        setNotice(`Wallet saved to ${network}. Syncing its history…`);
        const outcome = await syncCounterpartyImmediately(
          trackedAddress,
          syncWalletHistory,
          async (investigationId) => {
            setActivityRefreshKey((revision) => revision + 1);
            await refreshInvestigationGraph(investigationId, nextAddresses);
          },
        );
        if (outcome.status === "sync_failed") {
          setHistoryError(
            outcome.error instanceof ApiRequestError
              ? outcome.error.message
              : "Could not sync this address yet. Use Sync wallet history to retry.",
          );
          setNotice(
            `Wallet saved to ${network}; history sync failed. The address remains in the investigation.`,
          );
        } else if (outcome.status === "graph_refresh_failed") {
          setFlowError("History synced, but the graph could not be refreshed.");
          setNotice(
            `Wallet saved and history synced from ${outcome.sync.provider}. Reload the graph to continue.`,
          );
        } else {
          const coverage = outcome.sync.may_have_more_history
            ? " Older history may need another sync."
            : "";
          setNotice(
            `Wallet saved. Loaded ${outcome.sync.fetched} observations from ${outcome.sync.provider}.${coverage}`,
          );
        }
        setIsSyncing(false);
      } else {
        setNotice(
          `Wallet saved to ${network}. History can be synced on demand.`,
        );
      }
      if (!shouldSyncCounterparty && flowHistories && walletInvestigationId) {
        try {
          await refreshInvestigationGraph(walletInvestigationId, nextAddresses);
        } catch {
          setFlowError("Wallet saved, but the graph could not be refreshed.");
        }
      }
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

  const syncSavedAddress = async (
    address: TrackedAddress | undefined = selectedTrackedAddress,
  ) => {
    if (!address || isSyncing) return;
    setIsSyncing(true);
    setHistoryError("");
    try {
      const result = await syncWalletHistory(address);
      let records: ObservedTransfer[];
      try {
        records = await listWalletHistory(address);
      } catch {
        setHistoryError(
          "History synced, but saved records could not be loaded. Retry loading history.",
        );
        return;
      }
      setHistoryRecords(records);
      setActivityRefreshKey((revision) => revision + 1);
      try {
        await refreshInvestigationGraph(
          address.investigation_id,
          trackedAddresses,
        );
      } catch {
        setFlowError("History synced, but the graph could not be refreshed.");
      }
      const coverage = result.may_have_more_history
        ? " The provider page limit was reached; older history may be missing."
        : "";
      setNotice(
        `Saved ${result.fetched} ${getNetworkName(address.network, networkCatalog.networks)} observations from ${result.provider}.${coverage}`,
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
    const investigationId = activeInvestigationData.investigationId;
    if (!investigationId) return;
    setFlowLoading(true);
    setFlowError("");
    try {
      await refreshInvestigationGraph(investigationId);
    } catch (error) {
      setFlowError(
        error instanceof ApiRequestError
          ? error.message
          : "Could not load saved transaction history. Try again.",
      );
    } finally {
      setFlowLoading(false);
    }
  };

  const saveCurrentGraphSnapshot = async (
    input: Omit<GraphSnapshot, "id" | "investigation_id" | "created_at">,
  ) => {
    const investigationId = activeInvestigationData.investigationId;
    if (!investigationId || savingFlowSnapshot) return;
    setSavingFlowSnapshot(true);
    setFlowSnapshotError("");
    try {
      const snapshot = await saveGraphSnapshot({
        investigationId,
        title: input.title,
        focusNetwork: input.focus_network,
        focusAddress: input.focus_address,
        maxDepth: input.max_depth,
        sourceTransferIds: input.source_transfer_ids,
        sourceRelationshipIds: input.source_relationship_ids,
        graph: input.graph,
      });
      setFlowSnapshots((current) => [snapshot, ...current]);
      setOpenedFlowSnapshot(snapshot);
      setNotice("Investigation snapshot saved.");
    } catch (error) {
      setFlowSnapshotError(
        error instanceof ApiRequestError
          ? error.message
          : "Could not save the investigation snapshot. Try again.",
      );
    } finally {
      setSavingFlowSnapshot(false);
    }
  };

  const screenSelectedAddress = async () => {
    if (!selectedTrackedAddress || screeningLoading) return;
    if (!hasAddressScreeningMapping(selectedTrackedAddress.network)) {
      setScreeningError(
        "Exact-address screening is not available for this network yet.",
      );
      setScreeningResult(null);
      return;
    }
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
    setWalletInvestigationId("");
    setTrackSource("manual");
    setShowAdd(true);
  };

  const selectWatchlistAddress = (addressId: string) => {
    setSelectedAddressId(addressId);
    setHistoryRecords(null);
    setHistoryError("");
    setScreeningResult(null);
    setScreeningError("");
  };

  const openWatchlistAddress = (addressId: string) => {
    selectWatchlistAddress(addressId);
    setActiveTab("Overview");
  };

  const syncWatchlistAddress = (address: TrackedAddress) => {
    selectWatchlistAddress(address.id);
    void syncSavedAddress(address);
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
        <button
          className="workspace-select"
          onClick={() => setActiveTab("Investigations")}
        >
          <div className="workspace-avatar">TF</div>
          <span>
            <b>
              {investigations.find(({ id }) => id === activeInvestigationId)
                ?.name ?? "No investigation"}
            </b>
            <small>
              {activeInvestigationId
                ? `${activeInvestigationAddresses.length} saved addresses`
                : "Create an investigation"}
            </small>
          </span>
        </button>
        <div className="nav-caption">Workspace</div>
        <nav className="primary-nav" aria-label="Главная навигация">
          {PRIMARY_NAVIGATION.map(({ label, Icon, available }) => (
            <button
              key={label}
              className={`nav-item ${activeTab === label ? "active" : ""}`}
              onClick={() => {
                if (available) setActiveTab(label);
              }}
              disabled={!available}
              title={available ? undefined : `${label} is not available yet`}
              aria-label={available ? label : `${label}, not available yet`}
            >
              <Icon size={17} />
              <span>{label}</span>
              {!available && <small className="nav-coming-soon">Soon</small>}
            </button>
          ))}
        </nav>
        <div className="nav-caption network-caption">
          Networks{" "}
          <button
            aria-label="Настроить сети"
            onClick={() =>
              setNotice(
                "Список сетей загружается из API. Настройте провайдеры в backend environment.",
              )
            }
          >
            <Plus size={15} />
          </button>
        </div>
        <div className="network-list">
          {networkFilterOptions.map(({ id, name }) => (
            <button
              key={id}
              className={`network-item ${activeNetwork === id ? "active" : ""}`}
              onClick={() => {
                setActivityPage(0);
                setActiveNetwork(activeNetwork === id ? "All networks" : id);
              }}
            >
              <NetworkMark network={name} />
              <span>{name}</span>
              <i />
            </button>
          ))}
        </div>
        <div className="sidebar-spacer" />
        <div className="usage-card">
          <div className="usage-head">
            <span>Data sources</span>
            <span className="source-count">
              {apiStatus === "online"
                ? `${activeInvestigationAddresses.length} tracked`
                : "API unavailable"}
            </span>
          </div>
          <p>
            Provider coverage and keys are set in the backend environment. The
            Compose worker polls saved history when running.
          </p>
        </div>
        <div className="profile-button">
          <span className="profile-avatar">TF</span>
          <span>
            <b>Local analyst</b>
            <small>Local instance</small>
          </span>
        </div>
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
          </div>
        </header>
        {activeTab === "Investigations" ? (
          <div className="page-content">
            <InvestigationManager
              investigations={investigations}
              activeInvestigationId={activeInvestigationId}
              addressesByInvestigation={addressCountsByInvestigation}
              loading={addressesLoading}
              saving={isCreatingInvestigation}
              error={addressesError}
              onSelect={selectInvestigation}
              onCreate={handleCreateInvestigation}
              onRetry={() => void retryAddressLoad()}
            />
          </div>
        ) : activeTab === "Watchlist" ? (
          <div className="page-content">
            <WatchlistPage
              addresses={activeInvestigationAddresses}
              histories={activeInvestigationData.histories}
              networkNames={networkNames}
              selectedAddressId={selectedAddressId}
              historyStatus={activityStatus}
              loading={addressesLoading}
              syncing={isSyncing}
              canSync={apiStatus === "online"}
              error={addressesError || historyError || activityError}
              investigationName={
                investigations.find(({ id }) => id === activeInvestigationId)
                  ?.name ?? ""
              }
              onSelectAddress={selectWatchlistAddress}
              onOpenAddress={openWatchlistAddress}
              onAddAddress={openAddWallet}
              onSyncAddress={syncWatchlistAddress}
              onRetry={() => void retryAddressLoad()}
              onOpenInvestigations={() => setActiveTab("Investigations")}
            />
          </div>
        ) : (
          <div className="page-content">
            <div className="page-heading">
              <div>
                <div className="eyebrow">
                  <span className="eyebrow-dot" />
                  Investigation
                </div>
                <h1>Address flow overview</h1>
                <p>
                  Review observed transfers and participant links from saved
                  addresses.
                </p>
              </div>
              <div className="heading-actions">
                <button
                  className="button button-primary"
                  onClick={openAddWallet}
                >
                  <Plus size={17} /> Add wallet
                </button>
              </div>
            </div>
            <section className="stats-grid" aria-label="Investigation summary">
              <article className="stat-card">
                <div className="stat-label">
                  Tracked addresses <Wallet size={15} />
                </div>
                <div className="stat-value">
                  {addressesLoading ? "—" : dashboardSummary.trackedAddresses}
                </div>
                <div className="stat-foot">
                  <span>Saved in this investigation</span>
                </div>
              </article>
              <article className="stat-card">
                <div className="stat-label">
                  Networks <Blocks size={15} />
                </div>
                <div className="stat-value">
                  {addressesLoading ? "—" : dashboardSummary.networks}
                </div>
                <div className="stat-foot">
                  <span>Represented by saved addresses</span>
                </div>
              </article>
              <article className="stat-card">
                <div className="stat-label">
                  Transactions observed <Activity size={15} />
                </div>
                <div className="stat-value">
                  {activityStatus === "ready"
                    ? dashboardSummary.transactions
                    : "—"}
                </div>
                <div className="stat-foot">
                  <span>Unique transaction hashes in saved history</span>
                </div>
              </article>
              <article className="stat-card">
                <div className="stat-label">
                  Connections <GitBranch size={15} />
                </div>
                <div className="stat-value">
                  {activityStatus === "ready"
                    ? dashboardSummary.connections
                    : "—"}
                </div>
                <div className="stat-foot">
                  <span>Observed links in the current graph</span>
                </div>
              </article>
            </section>

            <section
              className="live-graph-region"
              aria-label="Wallet connections"
            >
              {activityStatus === "loading" || apiStatus === "checking" ? (
                <div className="address-state" role="status">
                  Loading saved addresses and transaction history…
                </div>
              ) : activityStatus === "error" ? (
                <div className="address-state address-error" role="alert">
                  <span>{activityError}</span>
                  <button
                    className="text-action"
                    onClick={() =>
                      setActivityRefreshKey((revision) => revision + 1)
                    }
                    disabled={apiStatus !== "online"}
                  >
                    Retry
                  </button>
                </div>
              ) : activeInvestigationAddresses.length === 0 ? (
                <div className="flow-graph-empty">
                  <span className="flow-graph-empty-mark" aria-hidden="true">
                    <svg viewBox="0 0 40 40">
                      <circle cx="10" cy="20" r="4" />
                      <circle cx="30" cy="10" r="4" />
                      <circle cx="30" cy="30" r="4" />
                      <path d="M14 18 26 12M14 22l12 6" />
                    </svg>
                  </span>
                  <div>
                    <strong>No addresses in this investigation</strong>
                    <p>
                      Add a public wallet address, then sync its history to
                      start mapping observed connections.
                    </p>
                  </div>
                  <button
                    className="button button-primary"
                    onClick={openAddWallet}
                  >
                    <Plus size={16} /> Add wallet
                  </button>
                </div>
              ) : flowHistories ? (
                <>
                  <WalletFlowGraph
                    key={`${activeInvestigationData.investigationId}:${activeFlowSnapshot?.id ?? "live"}`}
                    histories={flowHistories}
                    networkNames={networkNames}
                    snapshots={visibleFlowSnapshots}
                    openedSnapshot={activeFlowSnapshot}
                    savingSnapshot={savingFlowSnapshot}
                    onSaveSnapshot={(snapshot) =>
                      void saveCurrentGraphSnapshot(snapshot)
                    }
                    onOpenSnapshot={setOpenedFlowSnapshot}
                    onTrackCounterparty={(
                      network,
                      address,
                      investigationId,
                    ) => {
                      setQuery(address);
                      setNetworkChoice(network);
                      setWalletLabel("");
                      setWalletInvestigationId(investigationId);
                      setTrackSource("counterparty");
                      setFormError("");
                      setShowAdd(true);
                    }}
                  />
                  {dashboardSummary.connections === 0 &&
                    selectedTrackedAddress && (
                      <div className="flow-graph-action">
                        <div>
                          <strong>History is saved per address</strong>
                          <p>
                            Sync the selected wallet to load provider
                            observations. Some networks expose participants
                            without a direct sender or recipient.
                          </p>
                        </div>
                        <button
                          className="button button-secondary"
                          onClick={() => void syncSavedAddress()}
                          disabled={isSyncing || apiStatus !== "online"}
                        >
                          {isSyncing
                            ? "Syncing history…"
                            : "Sync selected wallet"}
                        </button>
                      </div>
                    )}
                  {flowLoading && (
                    <div className="address-state" role="status">
                      Refreshing graph evidence…
                    </div>
                  )}
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
                  {flowSnapshotError && (
                    <div className="address-state address-error" role="alert">
                      {flowSnapshotError}
                    </div>
                  )}
                </>
              ) : (
                <div className="address-state" role="status">
                  Loading saved graph evidence…
                </div>
              )}
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
                  {selectedTrackedAddress.label ||
                    selectedTrackedAddress.address}
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
                    <div
                      className={`risk-score score-${walletAssessment.level}`}
                    >
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
                          {activityStatus === "ready"
                            ? "No matching observations or analyst evidence are recorded for this address."
                            : "Saved transaction observations are not loaded, so behavior signals are unavailable."}
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
                                  {new Date(
                                    signal.observedAt,
                                  ).toLocaleDateString("en-US", {
                                    month: "short",
                                    day: "numeric",
                                  })}
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
                                <small>
                                  +{item.contribution.toFixed(1)} pts
                                </small>
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
                              : selectedTrackedAddress &&
                                  !hasAddressScreeningMapping(
                                    selectedTrackedAddress.network,
                                  )
                                ? "Exact-address screening is not mapped for this network; no screening result is included."
                                : "OFAC check has not been run for this address; sanctions evidence is not included."}
                    </span>
                    <button
                      className="text-action"
                      onClick={() => void screenSelectedAddress()}
                      disabled={
                        screeningLoading ||
                        !selectedTrackedAddress ||
                        !hasAddressScreeningMapping(
                          selectedTrackedAddress.network,
                        )
                      }
                    >
                      {screeningLoading ? "Checking…" : "Run OFAC check"}
                    </button>
                  </div>
                </>
              )}
              <div className="risk-footnote">
                <ShieldCheck size={14} />
                <span>
                  {walletAssessment.disclaimer} Freshness and provider
                  reliability are shown as context, not scored.
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
              ) : activeInvestigationAddresses.length === 0 ? (
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
                    {activeInvestigationAddresses.map((address) => (
                      <option key={address.id} value={address.id}>
                        {address.label ? `${address.label} · ` : ""}
                        {address.address} (
                        {getNetworkName(
                          address.network,
                          networkCatalog.networks,
                        )}
                        )
                      </option>
                    ))}
                  </select>
                  <div className="address-management-actions">
                    <button
                      className="button button-secondary"
                      onClick={() => void viewSavedHistory()}
                      disabled={!selectedTrackedAddress || historyLoading}
                    >
                      {historyLoading
                        ? "Loading history…"
                        : "View saved history"}
                    </button>
                    <button
                      className="button button-primary"
                      onClick={() => void syncSavedAddress()}
                      disabled={
                        !selectedTrackedAddress ||
                        isSyncing ||
                        getNetworkSyncAvailability(
                          selectedTrackedAddress.network,
                          networkCatalog,
                        ) === false
                      }
                    >
                      {isSyncing ? "Syncing…" : "Sync wallet history"}
                    </button>
                  </div>
                  <p className="address-note">
                    {selectedTrackedAddress?.network === "bitcoin"
                      ? "Saved history is loaded on demand. Sync requests Bitcoin data from the configured Esplora provider."
                      : selectedTrackedAddress &&
                          getNetworkSyncAvailability(
                            selectedTrackedAddress.network,
                            networkCatalog,
                          ) === false
                        ? "History sync is not configured for this network. Existing saved observations remain available."
                        : `Saved history is loaded on demand from the configured provider. Network: ${selectedTrackedAddress ? getNetworkName(selectedTrackedAddress.network, networkCatalog.networks) : "—"}.`}
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
                    Exact checks use the local list snapshot and currently map
                    Bitcoin and Ethereum addresses. Indirect exposure is not
                    checked.
                  </p>
                </div>
                <button
                  className="button button-secondary"
                  onClick={() => void screenSelectedAddress()}
                  disabled={
                    !selectedTrackedAddress ||
                    screeningLoading ||
                    !hasAddressScreeningMapping(selectedTrackedAddress.network)
                  }
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
              {activeInvestigationAddresses.some(
                ({ network }) => network === "solana",
              ) && (
                <p className="network-catalog-state" role="note">
                  Solana history is partial: finalized SOL transfers and parsed
                  SPL transfers from transactions that reference this wallet.
                  Incoming SPL activity can be missed when the wallet appears
                  only as token-balance owner metadata. Token-account discovery,
                  unsupported programs, and public RPC retention also leave
                  gaps.
                </p>
              )}
              {activeInvestigationAddresses.some(
                ({ network }) => network === "tron",
              ) && (
                <p className="network-catalog-state" role="note">
                  TRON history covers confirmed native TRX transfers and indexed
                  TRC-20 Transfer events. Other contract calls, internal TRX
                  movements, TRC-10, TRC-721, and events omitted or delayed by
                  TronGrid are not included.
                </p>
              )}
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
                      <th title="Review priority only. Evidence does not establish wrongdoing or legal status.">
                        Risk score
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {activityStatus === "loading" && (
                      <tr>
                        <td
                          colSpan={8}
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
                          colSpan={8}
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
                      pagedTransfers.map((transfer) => {
                        const riskPlaceholder = getTransferRiskPlaceholder(
                          transfer.trackedAddressId,
                          selectedTrackedAddress?.id ?? null,
                          riskSignalsLoading,
                          riskSignalsError,
                        );
                        return (
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
                                {transfer.amount}{" "}
                                <small>{transfer.asset}</small>
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
                                {transfer.confirmed
                                  ? "Confirmed"
                                  : "Unconfirmed"}
                              </span>
                            </td>
                            <td>
                              <span className="activity-source">
                                <strong>{transfer.trackedWallet}</strong>
                                <small>{transfer.provider}</small>
                              </span>
                            </td>
                            <td>
                              {transfer.riskAssessment ? (
                                <span
                                  className={`transfer-risk ${transfer.riskAssessment.level}`}
                                  title={[
                                    transfer.riskAssessment.disclaimer,
                                    ...transfer.riskAssessment.contributions.map(
                                      ({
                                        evidence,
                                        basePoints,
                                        factor,
                                        points,
                                      }) =>
                                        `${evidence.title}: ${basePoints} × ${factor} = ${points} points · ${evidence.source}${evidence.reference ? ` · ${evidence.reference}` : ""}`,
                                    ),
                                  ].join("\n")}
                                  aria-label={`Review priority ${transfer.riskAssessment.score}, ${transfer.riskAssessment.level}`}
                                >
                                  {transfer.riskAssessment.score} ·{" "}
                                  {transfer.riskAssessment.level}
                                </span>
                              ) : (
                                <span
                                  className="transfer-risk-none"
                                  title={riskPlaceholder.error ?? undefined}
                                >
                                  {riskPlaceholder.label}
                                </span>
                              )}
                            </td>
                          </tr>
                        );
                      })}
                  </tbody>
                </table>
                {activityStatus === "ready" &&
                  visibleTransfers.length === 0 && (
                    <div className="empty-state">
                      {activeInvestigationAddresses.length === 0
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
              <span>Provider history can be incomplete or delayed.</span>
            </footer>
          </div>
        )}
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
                  placeholder="0x…, bc1…, T… or address"
                  required
                  minLength={/^0x[\da-f]{1,64}$/i.test(query) ? 3 : 8}
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
                    <NetworkMark network={addressDetection.network} />
                    {addressDetection.network === "bitcoin"
                      ? "Bitcoin mainnet"
                      : (networkCatalog.networks.find(
                          ({ id }) => id === addressDetection.network,
                        )?.name ?? addressDetection.network)}
                  </span>
                )}
                {addressDetection.status === "ambiguous" && (
                  <select
                    id="wallet-network"
                    className="network-detected"
                    aria-label="Select network"
                    aria-describedby={
                      selectedNetworkChoice?.requires_api_key &&
                      !selectedNetworkChoice.history_sync_available
                        ? "network-sync-note"
                        : undefined
                    }
                    value={networkChoice}
                    onChange={(event) => {
                      setNetworkChoice(event.target.value);
                      setFormError("");
                    }}
                    required
                    disabled={isSubmitting}
                  >
                    <option value="">Select network</option>
                    {addressDetectionNetworks.map((network) => (
                      <option key={network.id} value={network.id}>
                        {network.name} · {network.native_asset}
                        {network.chain_id ? ` · chain ${network.chain_id}` : ""}
                      </option>
                    ))}
                  </select>
                )}
                {selectedNetworkChoice?.requires_api_key &&
                  !selectedNetworkChoice.history_sync_available && (
                    <small
                      id="network-sync-note"
                      className="network-catalog-state"
                      role="status"
                    >
                      {selectedNetworkChoice.requires_paid_tier
                        ? "History sync requires an Etherscan API key on an eligible paid plan."
                        : "History sync requires an Etherscan API key on the backend."}
                    </small>
                  )}
              </div>
              {networkCatalogLoading && (
                <p className="network-catalog-state" role="status">
                  Loading supported networks…
                </p>
              )}
              {networkCatalogError && (
                <div
                  className="network-catalog-state network-catalog-error"
                  role="alert"
                >
                  <span>
                    {networkCatalogError} Built-in networks remain available.
                  </span>
                  <button
                    type="button"
                    className="text-action"
                    onClick={() => {
                      setNetworkCatalogLoading(true);
                      setNetworkCatalogError("");
                      setNetworkCatalogRetry((value) => value + 1);
                    }}
                    disabled={networkCatalogLoading}
                  >
                    Retry
                  </button>
                </div>
              )}
              {addressDetection.status === "ambiguous" &&
                !networkCatalogLoading &&
                addressDetectionNetworks.length === 0 && (
                  <p className="network-catalog-state" role="status">
                    No matching networks are available in the catalog.
                  </p>
                )}
              {networkChoice &&
                getNetworkSyncAvailability(networkChoice, networkCatalog) ===
                  false && (
                  <p className="network-catalog-state" role="status">
                    History sync is not configured for this network. You can
                    still track the address.
                  </p>
                )}
              {networkChoice &&
                getNetworkSyncAvailability(networkChoice, networkCatalog) ===
                  true && (
                  <p className="network-catalog-state" role="status">
                    A provider endpoint is configured. Requests can still be
                    rate-limited or unavailable.
                  </p>
                )}
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
