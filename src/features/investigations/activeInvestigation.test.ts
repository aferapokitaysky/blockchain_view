import { describe, expect, it } from "vitest";
import type { Investigation } from "./api";
import {
  ACTIVE_INVESTIGATION_STORAGE_KEY,
  readStoredInvestigationId,
  resolveActiveInvestigationId,
  storeInvestigationId,
} from "./activeInvestigation";

const investigations: Investigation[] = [
  {
    id: "investigation-a",
    name: "Treasury review",
    created_at: "2026-11-16T10:00:00Z",
  },
  {
    id: "investigation-b",
    name: "Exchange review",
    created_at: "2026-11-17T10:00:00Z",
  },
];

function createStorage() {
  const values = new Map<string, string>();
  return {
    getItem(key: string) {
      return values.get(key) ?? null;
    },
    setItem(key: string, value: string) {
      values.set(key, value);
    },
    removeItem(key: string) {
      values.delete(key);
    },
  };
}

describe("active investigation selection", () => {
  it("keeps a saved selection only while it exists on the server", () => {
    expect(
      resolveActiveInvestigationId(investigations, "investigation-b"),
    ).toBe("investigation-b");
    expect(resolveActiveInvestigationId(investigations, "removed-id")).toBe(
      "investigation-a",
    );
    expect(resolveActiveInvestigationId([], "investigation-a")).toBeNull();
  });

  it("persists and clears the selected investigation id", () => {
    const storage = createStorage();
    storeInvestigationId("investigation-b", storage);

    expect(storage.getItem(ACTIVE_INVESTIGATION_STORAGE_KEY)).toBe(
      "investigation-b",
    );
    expect(readStoredInvestigationId(storage)).toBe("investigation-b");

    storeInvestigationId(null, storage);
    expect(readStoredInvestigationId(storage)).toBeNull();
  });

  it("does not fail if browser storage is unavailable", () => {
    const unavailableStorage = {
      getItem() {
        throw new Error("storage is blocked");
      },
      setItem() {
        throw new Error("storage is blocked");
      },
      removeItem() {
        throw new Error("storage is blocked");
      },
    };

    storeInvestigationId("investigation-a", unavailableStorage);
    expect(readStoredInvestigationId(unavailableStorage)).toBeNull();
  });
});
