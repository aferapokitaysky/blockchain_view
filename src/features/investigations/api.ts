import type { SupportedNetwork } from "../networks/detectAddressNetwork";

const API_BASE_URL = (
  import.meta.env.VITE_API_BASE_URL || "http://127.0.0.1:8080"
).replace(/\/$/, "");
const REQUEST_TIMEOUT_MS = 8_000;
const SYNC_TIMEOUT_MS = 260_000;
const ADDRESS_LIST_CONCURRENCY = 4;

type Investigation = {
  id: string;
  name: string;
};

export type TrackedAddress = {
  id: string;
  investigation_id: string;
  network: Lowercase<SupportedNetwork>;
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
  transfer_kind?: "bitcoin" | "evm_native" | "evm_internal" | "erc20";
  token_decimals?: number | null;
  token_contract_address?: string | null;
  from_address?: string | null;
  to_address?: string | null;
  source_host?: string;
};

export type WalletHistory = {
  address: TrackedAddress;
  transfers: ObservedTransfer[];
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
  const investigations = await requestJson<Investigation[]>(
    "/api/v1/investigations",
    undefined,
    REQUEST_TIMEOUT_MS,
    "Could not load investigations. Please try again.",
  );
  if (investigations[0]) return investigations[0].id;

  const created = await requestJson<Investigation>(
    "/api/v1/investigations",
    {
      method: "POST",
      body: JSON.stringify({ name: "Wallet flow investigation" }),
    },
    REQUEST_TIMEOUT_MS,
    "Could not create an investigation. Please try again.",
  );
  return created.id;
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

export async function listTrackedWallets(): Promise<TrackedAddress[]> {
  const investigations = await requestJson<Investigation[]>(
    "/api/v1/investigations",
    undefined,
    REQUEST_TIMEOUT_MS,
    "Could not load investigations. Please try again.",
  );
  const addresses = await mapWithConcurrency(
    investigations,
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
  network: SupportedNetwork;
  address: string;
  label?: string;
}): Promise<TrackedAddress> {
  const investigationId = await ensureInvestigation();
  return requestJson<TrackedAddress>(
    `/api/v1/investigations/${investigationId}/addresses`,
    {
      method: "POST",
      body: JSON.stringify({
        network: input.network.toLowerCase(),
        address:
          input.network === "Bitcoin"
            ? input.address.trim()
            : input.address.trim().toLowerCase(),
        label: input.label?.trim() || null,
      }),
    },
    REQUEST_TIMEOUT_MS,
    "Could not save the wallet. Please try again.",
  );
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

export async function listTrackedWalletHistories(
  addresses: TrackedAddress[],
): Promise<WalletHistory[]> {
  return mapWithConcurrency(
    addresses,
    ADDRESS_LIST_CONCURRENCY,
    async (address) => ({
      address,
      transfers: await listWalletHistory(address),
    }),
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
