export type SupportedNetwork =
  | "bitcoin"
  | "cardano"
  | "tron"
  | "solana"
  | "xrpl"
  | "ethereum"
  | "base"
  | "arbitrum"
  | `eip155:${number}`;

export type AddressDetection =
  | { status: "empty"; message: string }
  | { status: "invalid"; message: string }
  | { status: "unsupported"; message: string }
  | {
      status: "detected";
      network: "bitcoin";
      confidence: "format-and-checksum";
      message: string;
    }
  | {
      status: "detected";
      network: "xrpl";
      confidence: "format-and-checksum";
      message: string;
    }
  | {
      status: "detected";
      network: "ton";
      confidence: "format-and-checksum";
      message: string;
    }
  | {
      status: "detected";
      network: "cardano";
      confidence: "format-and-checksum";
      message: string;
    }
  | {
      status: "ambiguous";
      family:
        "EVM" | "Solana-compatible" | "Tron-compatible" | "Cardano-compatible";
      message: string;
    };

const BASE58_ALPHABET =
  "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const XRPL_BASE58_ALPHABET =
  "rpshnaf39wBUDNEGHJKLM4PQRST7VWXYZ2bcdeCg65jkm8oFqi1tuvAxyz";
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

function decodeBase58(
  address: string,
  alphabet = BASE58_ALPHABET,
  zeroDigit = "1",
): Uint8Array | null {
  if (address.length < 25 || address.length > 44) return null;

  let value = 0n;
  for (const character of address) {
    const digit = alphabet.indexOf(character);
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
  while (address[leadingZeroes] === zeroDigit) leadingZeroes += 1;
  const decoded = new Uint8Array(leadingZeroes + bytes.length);
  decoded.set(bytes, leadingZeroes);
  return decoded;
}

async function sha256(bytes: Uint8Array): Promise<Uint8Array> {
  const input = new Uint8Array(bytes).buffer;
  return new Uint8Array(
    await globalThis.crypto.subtle.digest("SHA-256", input),
  );
}

async function base58CheckVersion(address: string): Promise<number | null> {
  const decoded = decodeBase58(address);
  if (
    !decoded ||
    decoded.length !== 25 ||
    ![0x00, 0x05, 0x41].includes(decoded[0])
  )
    return null;

  const payload = decoded.slice(0, 21);
  const firstHash = await sha256(payload);
  const secondHash = await sha256(firstHash);
  return decoded.slice(21).every((byte, index) => byte === secondHash[index])
    ? decoded[0]
    : null;
}

async function isValidXrplClassicAddress(address: string): Promise<boolean> {
  const decoded = decodeBase58(address, XRPL_BASE58_ALPHABET, "r");
  if (!decoded || decoded.length !== 25 || decoded[0] !== 0) return false;
  const payload = decoded.slice(0, 21);
  const first = await sha256(payload);
  const second = await sha256(first);
  return decoded.slice(21).every((byte, index) => byte === second[index]);
}

function isValidTonAddress(address: string): boolean {
  const raw = /^(-?\d+):([\da-f]{64})$/i.exec(address);
  if (raw) return raw[1] === "0" || raw[1] === "-1";
  if (!/^[A-Za-z0-9_-]{48}$/.test(address)) return false;
  try {
    const binary = atob(address.replace(/-/g, "+").replace(/_/g, "/"));
    const bytes = Uint8Array.from(binary, (character) =>
      character.charCodeAt(0),
    );
    if (
      bytes.length !== 36 ||
      ![0x11, 0x51].includes(bytes[0]) ||
      ![0, 255].includes(bytes[1])
    ) {
      return false;
    }
    let crc = 0;
    for (const byte of bytes.slice(0, 34)) {
      crc ^= byte << 8;
      for (let bit = 0; bit < 8; bit += 1) {
        crc =
          (crc & 0x8000) !== 0
            ? ((crc << 1) ^ 0x1021) & 0xffff
            : (crc << 1) & 0xffff;
      }
    }
    return bytes[34] === crc >> 8 && bytes[35] === (crc & 0xff);
  } catch {
    return false;
  }
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

function isCardanoMainnetPaymentAddress(address: string): boolean {
  if (
    address.length < 50 ||
    address.length > 120 ||
    address !== address.toLowerCase()
  )
    return false;
  const separator = address.lastIndexOf("1");
  if (separator !== 4 || address.slice(0, separator) !== "addr") return false;
  const encoded = [...address.slice(separator + 1)].map((character) =>
    BECH32_ALPHABET.indexOf(character),
  );
  if (encoded.length < 7 || encoded.some((value) => value < 0)) return false;
  if (polymod([...expandHumanReadablePart("addr"), ...encoded]) !== BECH32)
    return false;
  const data = encoded.slice(0, -6);
  let accumulator = 0;
  let bits = 0;
  const bytes: number[] = [];
  for (const value of data) {
    accumulator = (accumulator << 5) | value;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      bytes.push((accumulator >> bits) & 0xff);
    }
  }
  if (bits >= 5 || ((accumulator << (8 - bits)) & 0xff) !== 0) return false;
  if (bytes.length < 29) return false;
  const addressType = bytes[0] >> 4;
  return addressType <= 7 && (bytes[0] & 0x0f) === 1;
}

/** Определяет формат адреса и не выдаёт догадку за конкретную EVM-сеть. */
export async function detectAddressNetwork(
  input: string,
): Promise<AddressDetection> {
  const address = input.trim();
  if (!address) return { status: "empty", message: "Paste a wallet address." };

  if (address.startsWith("addr1")) {
    if (!isCardanoMainnetPaymentAddress(address)) {
      return {
        status: "invalid",
        message: "Invalid Cardano mainnet payment address or checksum.",
      };
    }
    return {
      status: "detected",
      network: "cardano",
      confidence: "format-and-checksum",
      message:
        "Cardano mainnet payment address verified by format and checksum.",
    };
  }
  if (/^(Ae2|DdzFF)/.test(address)) {
    return {
      status: "ambiguous",
      family: "Cardano-compatible",
      message:
        "This looks like a Cardano Byron address. Choose Cardano; the API will validate its checksum and network.",
    };
  }

  if (/^(-?\d+:|[A-Za-z0-9_-]{48}$)/.test(address)) {
    if (!isValidTonAddress(address)) {
      return { status: "invalid", message: "Invalid TON address or checksum." };
    }
    return {
      status: "detected",
      network: "ton",
      confidence: "format-and-checksum",
      message: "TON mainnet address verified by format and checksum.",
    };
  }

  if (/^0x/i.test(address)) {
    if (!isHexAddress(address)) {
      return { status: "invalid", message: "Invalid EVM address format." };
    }
    return {
      status: "ambiguous",
      family: "EVM",
      message:
        "This EVM address format is shared across networks. Choose a network to continue.",
    };
  }

  if (/^(tb1|bcrt1)/i.test(address)) {
    return {
      status: "unsupported",
      message: "Bitcoin testnet addresses are not supported yet.",
    };
  }

  if (address.startsWith("X")) {
    return {
      status: "unsupported",
      message:
        "X-addresses are not supported. Use a classic XRP Ledger address.",
    };
  }

  if (/^r/.test(address)) {
    if (!(await isValidXrplClassicAddress(address))) {
      return {
        status: "invalid",
        message: "Invalid XRP Ledger classic address or checksum.",
      };
    }
    return {
      status: "detected",
      network: "xrpl",
      confidence: "format-and-checksum",
      message: "XRP Ledger classic address verified by format and checksum.",
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
      network: "bitcoin",
      confidence: "format-and-checksum",
      message: "Bitcoin mainnet address verified by format and checksum.",
    };
  }

  if (/^[13]/.test(address)) {
    const version = await base58CheckVersion(address);
    if (version === 0x00 || version === 0x05) {
      return {
        status: "detected",
        network: "bitcoin",
        confidence: "format-and-checksum",
        message: "Bitcoin mainnet address verified by format and checksum.",
      };
    }
  }

  const tronVersion = await base58CheckVersion(address);
  if (tronVersion === 0x41) {
    return {
      status: "ambiguous",
      family: "Tron-compatible",
      message:
        "This address has a valid TRON Base58Check format. Confirm TRON explicitly; address format alone does not prove network ownership.",
    };
  }

  if (decodeBase58(address)?.length === 32) {
    return {
      status: "ambiguous",
      family: "Solana-compatible",
      message:
        "This address has a 32-byte Solana-compatible public-key format. Select Solana to continue; format alone does not prove the network.",
    };
  }

  if (/^[13]/.test(address)) {
    return {
      status: "invalid",
      message: "Invalid Bitcoin address or checksum.",
    };
  }

  return {
    status: "unsupported",
    message: "This address format is not supported yet.",
  };
}
