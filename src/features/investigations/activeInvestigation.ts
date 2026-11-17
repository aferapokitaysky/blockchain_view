import type { Investigation } from "./api";

export const ACTIVE_INVESTIGATION_STORAGE_KEY =
  "tracefold.active-investigation-id";

type StorageLike = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function browserStorage(): StorageLike | null {
  try {
    return globalThis.localStorage;
  } catch {
    return null;
  }
}

export function readStoredInvestigationId(
  storage: StorageLike | null = browserStorage(),
): string | null {
  try {
    const value = storage?.getItem(ACTIVE_INVESTIGATION_STORAGE_KEY)?.trim();
    return value || null;
  } catch {
    return null;
  }
}

export function storeInvestigationId(
  investigationId: string | null,
  storage: StorageLike | null = browserStorage(),
): void {
  try {
    if (investigationId) {
      storage?.setItem(ACTIVE_INVESTIGATION_STORAGE_KEY, investigationId);
    } else {
      storage?.removeItem(ACTIVE_INVESTIGATION_STORAGE_KEY);
    }
  } catch {
    // Недоступное хранилище не должно блокировать работу с расследованиями.
  }
}

export function resolveActiveInvestigationId(
  investigations: Investigation[],
  preferredId: string | null,
): string | null {
  if (preferredId && investigations.some(({ id }) => id === preferredId)) {
    return preferredId;
  }
  return investigations[0]?.id ?? null;
}
