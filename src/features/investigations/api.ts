import {
  isEvmNetworkId,
  parseNetworkCatalog,
  type NetworkCatalog,
} from "../networks/networkCatalog";

const API_BASE_URL = (
  import.meta.env.VITE_API_BASE_URL || "http://127.0.0.1:8080"
).replace(/\/$/, "");
const REQUEST_TIMEOUT_MS = 8_000;
const SYNC_TIMEOUT_MS = 260_000;
const ADDRESS_LIST_CONCURRENCY = 4;

export type Investigation = {
  id: string;
  name: string;
  created_at: string;
};

export type TrackedAddress = {
  id: string;
  investigation_id: string;
  network: string;
  address: string;
  label: string | null;
  created_at: string;
};

export type SyncResult = {
  provider: string;
  fetched: number;
  saved: number;
  pages: number;
  may_have_more_history: boolean;
  observed_at: string;
};

export type ObservedTransfer = {
  id: string;
  tracked_address_id: string;
  network: string;
  transaction_hash: string;
  direction: "inbound" | "outbound" | "both";
  received_amount: string;
  spent_amount: string;
  asset: string;
  block_height: number | null;
  block_time: string | null;
  confirmed: boolean;
  provider: string;
  observed_at: string;
  event_index?: number;
  transfer_kind?:
    | "bitcoin"
    | "evm_native"
    | "evm_internal"
    | "erc20"
    | "solana_native"
    | "solana_spl"
    | "tron_native"
    | "trc20"
    | "xrpl_native"
    | "cardano_native"
    | "cardano_asset"
    | "aptos_fungible"
    | "sui_balance_change";
  source_host?: string;
  token_decimals?: number | null;
  token_contract_address?: string | null;
  from_address?: string | null;
  to_address?: string | null;
};

export type ObservedRelationship = {
  id: string;
  tracked_address_id: string;
  network: "cardano" | "sui";
  transaction_hash: string;
  counterparty_address: string;
  relationship_type: "same_transaction";
  participant_role: "input" | "output" | "input_and_output";
  block_height: number | null;
  block_time: string | null;
  confirmed: boolean;
  provider: string;
  source_host: string;
  observed_at: string;
};

export type WalletHistory = {
  address: TrackedAddress;
  transfers: ObservedTransfer[];
  relationships?: ObservedRelationship[];
};

export type GraphSnapshot = {
  id: string;
  investigation_id: string;
  title: string;
  focus_network: string | null;
  focus_address: string | null;
  max_depth: 1 | 2 | 3;
  source_transfer_ids: string[];
  source_relationship_ids: string[];
  graph: {
    nodes: Array<{
      id: string;
      network: string;
      address: string;
      label: string;
      tracked: boolean;
      x: number;
      y: number;
    }>;
    edges: Array<{
      id: string;
      from: string;
      to: string;
      asset: string;
      transfers: number;
      amount: string;
      decimals: number;
      latestAt: string;
      latestTimeKind: "block" | "observed";
      providers: string[];
      sourceHosts: string[];
      kind: "transfer" | "same_transaction";
      relationships: number;
    }>;
    truncated: boolean;
    sourceTransferIds: string[];
    sourceRelationshipIds: string[];
  };
  created_at: string;
};

export type AddressScreeningResult = {
  network: string;
  status:
    | "exact_match"
    | "no_exact_match"
    | "source_not_imported"
    | "network_not_covered";
  match_type: "exact_address";
  source: {
    source_name: string;
    published_at: string;
    imported_at: string;
    address_count: number;
  } | null;
  matches: Array<{
    entity_uid: string;
    entity_name: string;
    programs: string[];
    currency_code: string;
    address: string;
  }>;
  explanation: string;
};

export type StoredRiskSignal = {
  id: string;
  tracked_address_id: string;
  signal_key: string;
  category: "sanctions" | "exposure" | "behavior" | "other";
  severity: "critical" | "high" | "medium" | "low" | "informational";
  source_name: string;
  source_reference: string | null;
  observed_at: string;
  details: Record<string, unknown>;
  created_at: string;
};

export class ApiRequestError extends Error {
  readonly status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = "ApiRequestError";
    this.status = status;
  }
}

async function requestJson<T>(
  path: string,
  init?: RequestInit,
  timeoutMs = REQUEST_TIMEOUT_MS,
  failureMessage = "The API request failed. Please try again.",
): Promise<T> {
  const controller = new AbortController();
  const timeout = globalThis.setTimeout(() => controller.abort(), timeoutMs);

  try {
    const response = await fetch(`${API_BASE_URL}${path}`, {
      ...init,
      signal: controller.signal,
      headers: {
        ...(init?.body ? { "Content-Type": "application/json" } : {}),
        ...init?.headers,
      },
    });

    if (!response.ok) {
      if (response.status === 409) {
        throw new ApiRequestError(
          "This address is already in the investigation.",
          response.status,
        );
      }
      if (response.status === 422) {
        throw new ApiRequestError(
          "The API rejected this address or label. Check the details and try again.",
          response.status,
        );
      }
      if (response.status === 502) {
        throw new ApiRequestError(
          "The blockchain data provider is unavailable. Try again later.",
          response.status,
        );
      }
      throw new ApiRequestError(failureMessage, response.status);
    }

    return (await response.json()) as T;
  } catch (error) {
    if (error instanceof ApiRequestError) throw error;
    if (error instanceof DOMException && error.name === "AbortError") {
      throw new ApiRequestError(
        "The API did not respond in time. Check the connection and try again.",
        0,
      );
    }
    throw new ApiRequestError(
      "Could not connect to the local API. Start the service and try again.",
      0,
    );
  } finally {
    globalThis.clearTimeout(timeout);
  }
}

async function ensureInvestigation(): Promise<string> {
  const investigations = await listInvestigations();
  if (investigations[0]) return investigations[0].id;

  const created = await createInvestigation("Wallet flow investigation");
  return created.id;
}

export async function listInvestigations(): Promise<Investigation[]> {
  return requestJson<Investigation[]>(
    "/api/v1/investigations",
    undefined,
    REQUEST_TIMEOUT_MS,
    "Could not load investigations. Please try again.",
  );
}

export async function createInvestigation(
  name: string,
): Promise<Investigation> {
  return requestJson<Investigation>(
    "/api/v1/investigations",
    {
      method: "POST",
      body: JSON.stringify({ name: name.trim() }),
    },
    REQUEST_TIMEOUT_MS,
    "Could not create the investigation. Please try again.",
  );
}

async function mapWithConcurrency<T, R>(
  values: T[],
  concurrency: number,
  mapValue: (value: T) => Promise<R>,
): Promise<R[]> {
  const results = new Array<R>(values.length);
  let nextIndex = 0;
  const workerCount = Math.min(concurrency, values.length);

  await Promise.all(
    Array.from({ length: workerCount }, async () => {
      while (nextIndex < values.length) {
        const index = nextIndex++;
        results[index] = await mapValue(values[index]);
      }
    }),
  );
  return results;
}

export async function listTrackedWallets(
  investigations?: Investigation[],
): Promise<TrackedAddress[]> {
  const investigationList = investigations ?? (await listInvestigations());
  const addresses = await mapWithConcurrency(
    investigationList,
    ADDRESS_LIST_CONCURRENCY,
    (investigation) =>
      requestJson<TrackedAddress[]>(
        `/api/v1/investigations/${investigation.id}/addresses`,
        undefined,
        REQUEST_TIMEOUT_MS,
        "Could not load saved addresses. Please try again.",
      ),
  );
  return addresses.flat();
}

export async function trackWallet(input: {
  network: string;
  address: string;
  label?: string;
  investigationId?: string;
}): Promise<TrackedAddress> {
  const investigationId =
    input.investigationId || (await ensureInvestigation());
  return requestJson<TrackedAddress>(
    `/api/v1/investigations/${investigationId}/addresses`,
    {
      method: "POST",
      body: JSON.stringify({
        network: input.network.toLowerCase(),
        address: isEvmNetworkId(input.network)
          ? input.address.trim().toLowerCase()
          : input.address.trim(),
        label: input.label?.trim() || null,
      }),
    },
    REQUEST_TIMEOUT_MS,
    "Could not save the wallet. Please try again.",
  );
}

export async function listSupportedNetworks(): Promise<NetworkCatalog> {
  const response = await requestJson<unknown>(
    "/api/v1/networks",
    undefined,
    REQUEST_TIMEOUT_MS,
    "Could not load supported networks. Try again.",
  );
  return parseNetworkCatalog(response);
}

export async function syncWalletHistory(
  address: TrackedAddress,
): Promise<SyncResult> {
  return requestJson<SyncResult>(
    `/api/v1/investigations/${address.investigation_id}/addresses/${address.id}/sync`,
    { method: "POST" },
    SYNC_TIMEOUT_MS,
    "Could not sync Bitcoin history. Please try again.",
  );
}

export async function listWalletHistory(
  address: TrackedAddress,
): Promise<ObservedTransfer[]> {
  return requestJson<ObservedTransfer[]>(
    `/api/v1/investigations/${address.investigation_id}/addresses/${address.id}/transfers`,
    undefined,
    REQUEST_TIMEOUT_MS,
    "Could not load saved wallet history. Please try again.",
  );
}

async function listWalletHistoryRecord(
  address: TrackedAddress,
): Promise<Pick<WalletHistory, "transfers" | "relationships">> {
  if (address.network !== "cardano" && address.network !== "sui") {
    return {
      transfers: await listWalletHistory(address),
      relationships: [],
    };
  }
  return requestJson<Pick<WalletHistory, "transfers" | "relationships">>(
    `/api/v1/investigations/${address.investigation_id}/addresses/${address.id}/history`,
    undefined,
    REQUEST_TIMEOUT_MS,
    "Could not load saved wallet history and participants. Please try again.",
  );
}

export async function listWalletRiskSignals(
  address: TrackedAddress,
): Promise<StoredRiskSignal[]> {
  return requestJson<StoredRiskSignal[]>(
    `/api/v1/investigations/${address.investigation_id}/addresses/${address.id}/risk-signals`,
    undefined,
    REQUEST_TIMEOUT_MS,
    "Could not load wallet risk signals. Please try again.",
  );
}

export async function listTrackedWalletHistories(
  addresses: TrackedAddress[],
): Promise<WalletHistory[]> {
  return mapWithConcurrency(
    addresses,
    ADDRESS_LIST_CONCURRENCY,
    async (address) => ({
      address,
      ...(await listWalletHistoryRecord(address)),
    }),
  );
}

export async function listGraphSnapshots(
  investigationId: string,
): Promise<GraphSnapshot[]> {
  return requestJson<GraphSnapshot[]>(
    `/api/v1/investigations/${investigationId}/graph-snapshots`,
    undefined,
    REQUEST_TIMEOUT_MS,
    "Could not load saved investigation snapshots. Please try again.",
  );
}

export async function saveGraphSnapshot(input: {
  investigationId: string;
  title: string;
  focusNetwork: string | null;
  focusAddress: string | null;
  maxDepth: 1 | 2 | 3;
  sourceTransferIds: string[];
  sourceRelationshipIds: string[];
  graph: GraphSnapshot["graph"];
}): Promise<GraphSnapshot> {
  return requestJson<GraphSnapshot>(
    `/api/v1/investigations/${input.investigationId}/graph-snapshots`,
    {
      method: "POST",
      body: JSON.stringify({
        title: input.title,
        focus_network: input.focusNetwork,
        focus_address: input.focusAddress,
        max_depth: input.maxDepth,
        source_transfer_ids: input.sourceTransferIds,
        source_relationship_ids: input.sourceRelationshipIds,
        graph: input.graph,
      }),
    },
    REQUEST_TIMEOUT_MS,
    "Could not save the investigation snapshot. Please try again.",
  );
}

export async function screenWalletAddress(
  address: TrackedAddress,
): Promise<AddressScreeningResult> {
  const params = new URLSearchParams({
    network: address.network,
    address: address.address,
  });
  return requestJson<AddressScreeningResult>(
    `/api/v1/screening/address?${params.toString()}`,
    undefined,
    REQUEST_TIMEOUT_MS,
    "Could not check the address against the imported OFAC snapshot. Please try again.",
  );
}

export async function checkApiHealth(): Promise<boolean> {
  try {
    await requestJson<{ status: string }>("/health/ready");
    return true;
  } catch {
    return false;
  }
}
