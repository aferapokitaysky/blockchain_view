import { describe, expect, it, vi } from "vitest";
import type { SyncResult, TrackedAddress } from "./api";
import { syncCounterpartyImmediately } from "./syncCounterparty";

const address: TrackedAddress = {
  id: "address-1",
  investigation_id: "investigation-1",
  network: "ethereum",
  address: `0x${"1".repeat(40)}`,
  label: null,
  created_at: "2025-01-01T00:00:00Z",
};

const syncResult: SyncResult = {
  provider: "Blockscout",
  fetched: 1,
  saved: 1,
  pages: 1,
  may_have_more_history: false,
  observed_at: "2025-01-01T00:00:00Z",
};

describe("syncCounterpartyImmediately", () => {
  it("loads the new address before refreshing its investigation graph", async () => {
    const steps: string[] = [];
    const result = await syncCounterpartyImmediately(
      address,
      async (tracked) => {
        steps.push(`sync:${tracked.id}`);
        return syncResult;
      },
      async (investigationId) => {
        steps.push(`graph:${investigationId}`);
      },
    );

    expect(result).toEqual({ status: "complete", sync: syncResult });
    expect(steps).toEqual(["sync:address-1", "graph:investigation-1"]);
  });

  it("preserves the saved address when provider sync fails", async () => {
    const providerError = new Error("provider unavailable");
    const syncHistory = vi.fn().mockRejectedValue(providerError);
    const refreshGraph = vi.fn();

    const result = await syncCounterpartyImmediately(
      address,
      syncHistory,
      refreshGraph,
    );

    expect(result).toEqual({ status: "sync_failed", error: providerError });
    expect(syncHistory).toHaveBeenCalledWith(address);
    expect(refreshGraph).not.toHaveBeenCalled();
  });

  it("reports graph refresh failure separately after a successful sync", async () => {
    const graphError = new Error("graph unavailable");
    const result = await syncCounterpartyImmediately(
      address,
      async () => syncResult,
      async () => {
        throw graphError;
      },
    );

    expect(result).toEqual({
      status: "graph_refresh_failed",
      sync: syncResult,
      error: graphError,
    });
  });
});
