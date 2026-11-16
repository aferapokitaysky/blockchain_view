import { afterEach, describe, expect, it, vi } from "vitest";
import { checkApiHealth, trackWallet } from "./api";

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
});
