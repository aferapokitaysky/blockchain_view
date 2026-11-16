import type { ObservedTransfer } from "./api";
import { formatBitcoinAmount } from "./formatBitcoinAmount";

export function formatAssetAmount(rawAmount: string, decimals: number): string {
  const value = BigInt(rawAmount);
  if (decimals <= 0) return value.toLocaleString();
  const negative = value < 0n;
  const digits = (negative ? -value : value)
    .toString()
    .padStart(decimals + 1, "0");
  const whole = digits.slice(0, -decimals);
  const fraction = digits.slice(-decimals).replace(/0+$/, "");
  const formattedWhole = BigInt(whole).toLocaleString();
  return `${negative ? "-" : ""}${formattedWhole}${fraction ? `.${fraction}` : ""}`;
}

export function formatObservedAmount(
  transfer: Pick<
    ObservedTransfer,
    "network" | "transfer_kind" | "token_decimals"
  >,
  rawAmount: string,
): string {
  if (transfer.network === "bitcoin") return formatBitcoinAmount(rawAmount);
  if (
    transfer.transfer_kind === "erc20" &&
    (transfer.token_decimals === null || transfer.token_decimals === undefined)
  ) {
    return `${BigInt(rawAmount).toLocaleString()} raw units`;
  }
  return formatAssetAmount(rawAmount, transfer.token_decimals ?? 18);
}
