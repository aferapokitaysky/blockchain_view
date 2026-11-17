import { afterEach, describe, expect, it, vi } from "vitest";
import {
  checkApiHealth,
  listSupportedNetworks,
  listTrackedWallets,
  listTrackedWalletHistories,
  listGraphSnapshots,
  listWalletHistory,
  listWalletRiskSignals,
  screenWalletAddress,
  saveGraphSnapshot,
  syncWalletHistory,
  trackWallet,
} from "./api";

const investigation = {
  id: "550e8400-e29b-41d4-a716-446655440000",
  name: "Wallet flow investigation",
};

const trackedAddress = {
  id: "650e8400-e29b-41d4-a716-446655440000",
  investigation_id: investigation.id,
  network: "base",
  address: "0x0123456789abcdef0123456789abcdef01234567",
  label: "Treasury",
  created_at: "2026-11-16T10:00:00Z",
};

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("investigation API client", () => {
  it("loads Cardano transfers and participant evidence in one history response", async () => {
    const cardanoAddress = { ...trackedAddress, network: "cardano", address: "addr1test" };
    const relationship = {
      id: "relationship-1",
      tracked_address_id: cardanoAddress.id,
      network: "cardano",
      transaction_hash: "a".repeat(64),
      counterparty_address: "Ae2tdPwUPEZLs4HtbuNey7tK4hTKrwNwYtGqp7bDfCy2WdR3P6735W5Yfpe",
      relationship_type: "same_transaction",
      participant_role: "input",
      block_height: 42,
      block_time: "2026-11-16T10:00:00Z",
      confirmed: true,
      provider: "Blockfrost Cardano Mainnet",
      source_host: "cardano-mainnet.blockfrost.io",
      observed_at: "2026-11-16T10:00:00Z",
    };
    const fetchMock = vi.fn().mockResolvedValueOnce(jsonResponse({ transfers: [], relationships: [relationship] }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(listTrackedWalletHistories([cardanoAddress])).resolves.toEqual([
      { address: cardanoAddress, transfers: [], relationships: [relationship] },
    ]);
    expect(fetchMock).toHaveBeenCalledWith(
      `http://127.0.0.1:8080/api/v1/investigations/${investigation.id}/addresses/${cardanoAddress.id}/history`,
      expect.objectContaining({ headers: {} }),
    );
  });

  it("loads and validates the server network catalog", async () => {
    const catalog = {
      history_sync_available: true,
      networks: [
        {
          id: "eip155:10",
          name: "OP Mainnet",
          family: "evm",
          chain_id: 10,
          native_asset: "ETH",
          explorer_url: "https://explorer.optimism.io/",
        },
      ],
    };
    const fetchMock = vi.fn().mockResolvedValueOnce(jsonResponse(catalog));
    vi.stubGlobal("fetch", fetchMock);

    await expect(listSupportedNetworks()).resolves.toEqual(catalog);
    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:8080/api/v1/networks",
      expect.objectContaining({ headers: {} }),
    );
  });

  it("rejects malformed network catalog data instead of exposing unknown ids", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(
      jsonResponse({
        history_sync_available: false,
        networks: [
          {
            id: "custom-chain",
            name: "Unknown",
            family: "evm",
            chain_id: 999,
            native_asset: "ETH",
            explorer_url: null,
          },
        ],
      }),
    );
    vi.stubGlobal("fetch", fetchMock);

    await expect(listSupportedNetworks()).rejects.toThrow(
      "Network catalog contains no supported networks",
    );
  });

  it("creates a default investigation and saves the normalized address details", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse([]))
      .mockResolvedValueOnce(jsonResponse(investigation, 201))
      .mockResolvedValueOnce(jsonResponse(trackedAddress, 201));
    vi.stubGlobal("fetch", fetchMock);

    const result = await trackWallet({
      network: "Base",
      address: "  0x0123456789ABCDEF0123456789ABCDEF01234567  ",
      label: " Treasury ",
    });

    expect(result).toEqual(trackedAddress);
    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "http://127.0.0.1:8080/api/v1/investigations",
      expect.objectContaining({ headers: {} }),
    );
    expect(JSON.parse(fetchMock.mock.calls[1][1].body)).toEqual({
      name: "Wallet flow investigation",
    });
    expect(JSON.parse(fetchMock.mock.calls[2][1].body)).toEqual({
      network: "base",
      address: trackedAddress.address,
      label: "Treasury",
    });
  });

  it("reuses an existing investigation and explains duplicate-address errors", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse([investigation]))
      .mockResolvedValueOnce(
        jsonResponse(
          {
            error: {
              code: "conflict",
              message: "Запись с таким адресом уже есть в расследовании",
            },
          },
          409,
        ),
      );
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      trackWallet({
        network: "Ethereum",
        address: "0x0123456789ABCDEF0123456789ABCDEF01234567",
      }),
    ).rejects.toMatchObject({
      message: "This address is already in the investigation.",
      status: 409,
    });
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("preserves Base58 case when tracking TRON addresses", async () => {
    const tronAddress = "TUoHaVjx7n5xz8LwPRDckgFrDWhMhuSuJM";
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse([investigation]))
      .mockResolvedValueOnce(
        jsonResponse(
          {
            ...trackedAddress,
            network: "tron",
            address: tronAddress,
          },
          201,
        ),
      );
    vi.stubGlobal("fetch", fetchMock);

    await trackWallet({ network: "TRON", address: ` ${tronAddress} ` });

    expect(JSON.parse(fetchMock.mock.calls[1][1].body)).toMatchObject({
      network: "tron",
      address: tronAddress,
    });
  });

  it("saves and lists graph snapshots within an investigation", async () => {
    const graph = {
      nodes: [],
      edges: [],
      truncated: false,
      sourceTransferIds: ["event-1"],
      sourceRelationshipIds: [],
    };
    const snapshot = {
      id: "snapshot-1",
      investigation_id: investigation.id,
      title: "TRON flow",
      focus_network: "tron",
      focus_address: "TUoHaVjx7n5xz8LwPRDckgFrDWhMhuSuJM",
      max_depth: 2 as const,
      source_transfer_ids: ["event-1"],
      source_relationship_ids: [],
      graph,
      created_at: "2026-11-17T10:00:00Z",
    };
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse(snapshot, 201))
      .mockResolvedValueOnce(jsonResponse([snapshot]));
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      saveGraphSnapshot({
        investigationId: investigation.id,
        title: snapshot.title,
        focusNetwork: snapshot.focus_network,
        focusAddress: snapshot.focus_address,
        maxDepth: snapshot.max_depth,
        sourceTransferIds: snapshot.source_transfer_ids,
        sourceRelationshipIds: snapshot.source_relationship_ids,
        graph,
      }),
    ).resolves.toEqual(snapshot);
    await expect(listGraphSnapshots(investigation.id)).resolves.toEqual([
      snapshot,
    ]);
    expect(JSON.parse(fetchMock.mock.calls[0][1].body)).toMatchObject({
      focus_network: "tron",
      focus_address: snapshot.focus_address,
      source_transfer_ids: ["event-1"],
    });
  });

  it("reports readiness only when the API and its data stores respond", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse({ status: "ready" }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(checkApiHealth()).resolves.toBe(true);
    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:8080/health/ready",
      expect.objectContaining({ headers: {} }),
    );
  });

  it("starts a bounded on-demand Bitcoin history sync", async () => {
    const result = {
      provider: "Esplora (blockstream.info)",
      fetched: 25,
      saved: 25,
      pages: 1,
      may_have_more_history: false,
      observed_at: "2026-11-16T10:00:00Z",
    };
    const fetchMock = vi.fn().mockResolvedValueOnce(jsonResponse(result));
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      syncWalletHistory({ ...trackedAddress, network: "bitcoin" }),
    ).resolves.toEqual(result);
    expect(fetchMock).toHaveBeenCalledWith(
      `http://127.0.0.1:8080/api/v1/investigations/${investigation.id}/addresses/${trackedAddress.id}/sync`,
      expect.objectContaining({ method: "POST" }),
    );
  });

  it("reads only the selected tracked address history", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(jsonResponse([]));
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      listWalletHistory({ ...trackedAddress, network: "bitcoin" }),
    ).resolves.toEqual([]);
    expect(fetchMock).toHaveBeenCalledWith(
      `http://127.0.0.1:8080/api/v1/investigations/${investigation.id}/addresses/${trackedAddress.id}/transfers`,
      expect.objectContaining({ headers: {} }),
    );
  });

  it("lists saved addresses across investigations and accepts optional EVM fields", async () => {
    const secondInvestigation = {
      ...investigation,
      id: "750e8400-e29b-41d4-a716-446655440000",
    };
    const evmAddress = {
      ...trackedAddress,
      investigation_id: secondInvestigation.id,
      network: "base" as const,
    };
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse([investigation, secondInvestigation]))
      .mockResolvedValueOnce(jsonResponse([trackedAddress]))
      .mockResolvedValueOnce(jsonResponse([evmAddress]));
    vi.stubGlobal("fetch", fetchMock);

    await expect(listTrackedWallets()).resolves.toEqual([
      trackedAddress,
      evmAddress,
    ]);
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      `http://127.0.0.1:8080/api/v1/investigations/${investigation.id}/addresses`,
      expect.objectContaining({ headers: {} }),
    );

    const transfer = {
      id: "record-1",
      tracked_address_id: trackedAddress.id,
      network: "base",
      transaction_hash: "0xabc",
      direction: "inbound",
      received_amount: "1.2",
      spent_amount: "0",
      asset: "USDC",
      block_height: 12,
      block_time: null,
      confirmed: true,
      provider: "test",
      observed_at: "2026-11-16T10:00:00Z",
      event_index: 2,
      asset_decimals: 6,
      asset_contract: "0xcontract",
    };
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValueOnce(jsonResponse([transfer])),
    );
    await expect(listWalletHistory(evmAddress)).resolves.toEqual([transfer]);
  });

  it("screens the selected address against the local source snapshot", async () => {
    const result = {
      network: "ethereum",
      status: "no_exact_match",
      match_type: "exact_address",
      source: null,
      matches: [],
      explanation: "No exact match in the imported snapshot.",
    };
    const fetchMock = vi.fn().mockResolvedValueOnce(jsonResponse(result));
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      screenWalletAddress({ ...trackedAddress, network: "ethereum" }),
    ).resolves.toEqual(result);
    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:8080/api/v1/screening/address?network=ethereum&address=0x0123456789abcdef0123456789abcdef01234567",
      expect.objectContaining({ headers: {} }),
    );
  });

  it("loads risk evidence from the selected address scope", async () => {
    const signals = [
      {
        id: "signal-1",
        tracked_address_id: trackedAddress.id,
        signal_key: "rapid_pass_through",
        category: "behavior",
        severity: "medium",
        source_name: "Analyst review",
        source_reference: null,
        observed_at: "2026-11-16T10:00:00Z",
        details: { summary: "Repeated rapid transfers" },
        created_at: "2026-11-16T10:05:00Z",
      },
    ];
    const fetchMock = vi.fn().mockResolvedValueOnce(jsonResponse(signals));
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      listWalletRiskSignals({ ...trackedAddress, network: "ethereum" }),
    ).resolves.toEqual(signals);
    expect(fetchMock).toHaveBeenCalledWith(
      `http://127.0.0.1:8080/api/v1/investigations/${investigation.id}/addresses/${trackedAddress.id}/risk-signals`,
      expect.objectContaining({ headers: {} }),
    );
  });

  it("caps parallel address requests and reports history read errors accurately", async () => {
    const investigations = Array.from({ length: 9 }, (_, index) => ({
      ...investigation,
      id: `investigation-${index}`,
    }));
    let activeRequests = 0;
    let maximumActiveRequests = 0;
    const fetchMock = vi.fn(async (url: string) => {
      if (url.endsWith("/api/v1/investigations")) {
        return jsonResponse(investigations);
      }
      activeRequests += 1;
      maximumActiveRequests = Math.max(maximumActiveRequests, activeRequests);
      await new Promise((resolve) => setTimeout(resolve, 2));
      activeRequests -= 1;
      return jsonResponse([]);
    });
    vi.stubGlobal("fetch", fetchMock);

    await expect(listTrackedWallets()).resolves.toEqual([]);
    expect(maximumActiveRequests).toBe(4);

    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValueOnce(jsonResponse({}, 500)),
    );
    await expect(
      listWalletHistory({ ...trackedAddress, network: "bitcoin" }),
    ).rejects.toMatchObject({
      message: "Could not load saved wallet history. Please try again.",
      status: 500,
    });
  });
});
