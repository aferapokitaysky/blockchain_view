const SATOSHIS_PER_BITCOIN = 100_000_000n;

export function formatBitcoinAmount(satoshisValue: string): string {
  if (!/^\d+$/.test(satoshisValue)) return `${satoshisValue} sat`;

  const satoshis = BigInt(satoshisValue);
  const whole = satoshis / SATOSHIS_PER_BITCOIN;
  const fraction = (satoshis % SATOSHIS_PER_BITCOIN)
    .toString()
    .padStart(8, "0")
    .replace(/0+$/, "");

  return `${whole}${fraction ? `.${fraction}` : ""} BTC`;
}
