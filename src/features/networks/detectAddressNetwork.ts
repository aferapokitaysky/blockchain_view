export type SupportedNetwork = "Bitcoin" | "Ethereum" | "Base" | "Arbitrum";

export type AddressDetection =
  | { status: "empty"; message: string }
  | { status: "invalid"; message: string }
  | { status: "unsupported"; message: string }
  | {
      status: "detected";
      network: "Bitcoin";
      confidence: "format-and-checksum";
      message: string;
    }
  | {
      status: "ambiguous";
      family: "EVM";
      candidates: readonly ["Ethereum", "Base", "Arbitrum"];
      message: string;
    };

const BASE58_ALPHABET =
  "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const BECH32_ALPHABET = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
const BECH32 = 1n;
const BECH32M = 0x2bc830a3n;
const BECH32_GENERATORS = [
  0x3b6a57b2n,
  0x26508e6dn,
  0x1ea119fan,
  0x3d4233ddn,
  0x2a1462b3n,
];

function isHexAddress(address: string): boolean {
  return /^0x[\da-f]{40}$/i.test(address);
}

function decodeBase58(address: string): Uint8Array | null {
  if (address.length < 26 || address.length > 35) return null;

  let value = 0n;
  for (const character of address) {
    const digit = BASE58_ALPHABET.indexOf(character);
    if (digit < 0) return null;
    value = value * 58n + BigInt(digit);
  }

  const encodedHex = value.toString(16);
  const encoded =
    value === 0n
      ? []
      : (encodedHex
          .padStart(Math.ceil(encodedHex.length / 2) * 2, "0")
          .match(/.{2}/g) ?? []);
  const bytes = encoded.map((byte) => Number.parseInt(byte, 16));

  let leadingZeroes = 0;
  while (address[leadingZeroes] === "1") leadingZeroes += 1;
  const decoded = new Uint8Array(leadingZeroes + bytes.length);
  decoded.set(bytes, leadingZeroes);
  return decoded.length === 25 ? decoded : null;
}

async function sha256(bytes: Uint8Array): Promise<Uint8Array> {
  const input = new Uint8Array(bytes).buffer;
  return new Uint8Array(
    await globalThis.crypto.subtle.digest("SHA-256", input),
  );
}

async function isValidBase58CheckAddress(address: string): Promise<boolean> {
  const decoded = decodeBase58(address);
  if (!decoded || (decoded[0] !== 0x00 && decoded[0] !== 0x05)) return false;

  const payload = decoded.slice(0, 21);
  const firstHash = await sha256(payload);
  const secondHash = await sha256(firstHash);
  return decoded.slice(21).every((byte, index) => byte === secondHash[index]);
}

function polymod(values: number[]): bigint {
  let checksum = 1n;
  for (const value of values) {
    const top = checksum >> 25n;
    checksum = ((checksum & 0x1ffffffn) << 5n) ^ BigInt(value);
    for (let index = 0; index < BECH32_GENERATORS.length; index += 1) {
      if (((top >> BigInt(index)) & 1n) !== 0n) {
        checksum ^= BECH32_GENERATORS[index];
      }
    }
  }
  return checksum;
}

function expandHumanReadablePart(hrp: string): number[] {
  const high = [...hrp].map((character) => character.charCodeAt(0) >> 5);
  const low = [...hrp].map((character) => character.charCodeAt(0) & 31);
  return [...high, 0, ...low];
}

function witnessProgramLength(data: number[]): number | null {
  let accumulator = 0;
  let bits = 0;
  let byteCount = 0;

  for (const value of data) {
    accumulator = (accumulator << 5) | value;
    bits += 5;
    while (bits >= 8) {
      bits -= 8;
      byteCount += 1;
      accumulator &= (1 << bits) - 1;
    }
  }

  if (bits >= 5 || ((accumulator << (8 - bits)) & 0xff) !== 0) return null;
  return byteCount;
}

function isValidBech32WitnessAddress(address: string): boolean {
  if (address.length < 14 || address.length > 90) return false;
  if (address !== address.toLowerCase() && address !== address.toUpperCase()) {
    return false;
  }

  const normalized = address.toLowerCase();
  const separator = normalized.lastIndexOf("1");
  if (separator < 1 || separator + 7 > normalized.length) return false;

  const hrp = normalized.slice(0, separator);
  if (hrp !== "bc") return false;

  const encodedData = [...normalized.slice(separator + 1)].map((character) =>
    BECH32_ALPHABET.indexOf(character),
  );
  if (encodedData.some((value) => value < 0)) return false;

  const data = encodedData.slice(0, -6);
  if (data.length < 1) return false;

  const witnessVersion = data[0];
  if (witnessVersion > 16) return false;

  const checksum = polymod([...expandHumanReadablePart(hrp), ...encodedData]);
  const expectedChecksum = witnessVersion === 0 ? BECH32 : BECH32M;
  if (checksum !== expectedChecksum) return false;

  const programLength = witnessProgramLength(data.slice(1));
  if (programLength === null || programLength < 2 || programLength > 40) {
    return false;
  }
  return witnessVersion !== 0 || programLength === 20 || programLength === 32;
}

/** Определяет формат адреса и не выдаёт догадку за конкретную EVM-сеть. */
export async function detectAddressNetwork(
  input: string,
): Promise<AddressDetection> {
  const address = input.trim();
  if (!address) return { status: "empty", message: "Paste a wallet address." };

  if (/^0x/i.test(address)) {
    if (!isHexAddress(address)) {
      return { status: "invalid", message: "Invalid EVM address format." };
    }
    return {
      status: "ambiguous",
      family: "EVM",
      candidates: ["Ethereum", "Base", "Arbitrum"],
      message:
        "Valid EVM format. Select the network; the address cannot identify it.",
    };
  }

  if (/^(tb1|bcrt1)/i.test(address)) {
    return {
      status: "unsupported",
      message: "Bitcoin testnet addresses are not supported yet.",
    };
  }

  if (/^bc1/i.test(address)) {
    if (!isValidBech32WitnessAddress(address)) {
      return {
        status: "invalid",
        message: "Invalid Bitcoin mainnet address or checksum.",
      };
    }
    return {
      status: "detected",
      network: "Bitcoin",
      confidence: "format-and-checksum",
      message: "Bitcoin mainnet address verified by format and checksum.",
    };
  }

  if (/^[13]/.test(address)) {
    if (!(await isValidBase58CheckAddress(address))) {
      return {
        status: "invalid",
        message: "Invalid Bitcoin address or checksum.",
      };
    }
    return {
      status: "detected",
      network: "Bitcoin",
      confidence: "format-and-checksum",
      message: "Bitcoin mainnet address verified by format and checksum.",
    };
  }

  return {
    status: "unsupported",
    message: "This address format is not supported yet.",
  };
}
