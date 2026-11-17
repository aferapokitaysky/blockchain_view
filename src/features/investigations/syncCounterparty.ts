import type { SyncResult, TrackedAddress } from "./api";

export type CounterpartySyncOutcome =
  | { status: "sync_failed"; error: unknown }
  | { status: "graph_refresh_failed"; sync: SyncResult; error: unknown }
  | { status: "complete"; sync: SyncResult };

export async function syncCounterpartyImmediately(
  address: TrackedAddress,
  syncHistory: (address: TrackedAddress) => Promise<SyncResult>,
  refreshGraph: (investigationId: string) => Promise<void>,
): Promise<CounterpartySyncOutcome> {
  let sync: SyncResult;
  try {
    sync = await syncHistory(address);
  } catch (error: unknown) {
    return { status: "sync_failed", error };
  }

  try {
    await refreshGraph(address.investigation_id);
  } catch (error: unknown) {
    return { status: "graph_refresh_failed", sync, error };
  }

  return { status: "complete", sync };
}
