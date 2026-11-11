/**
 * Sizes arrive from the collector in MiB (process memory, interface
 * totals) or GiB (memory, disks). Show each in the largest of MiB / GiB /
 * TiB that keeps the number at 1 or more, so a 1863 GiB drive reads as
 * 1.82 TiB.
 */
const UNITS = ["MiB", "GiB", "TiB"] as const;
export type SizeUnit = (typeof UNITS)[number];

export interface FormattedSize {
  value: string;
  unit: SizeUnit;
}

export function formatSize(
  amount: number | null | undefined,
  from: SizeUnit = "GiB",
  decimals = 2,
): FormattedSize {
  const n = Number(amount);
  if (amount == null || !Number.isFinite(n)) return { value: "--", unit: from };
  let v = n * 1024 ** UNITS.indexOf(from);
  let i = 0;
  while (Math.abs(v) >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  return { value: v.toFixed(decimals), unit: UNITS[i] };
}

/** ``formatSize`` as one string, e.g. ``"1.82 TiB"``. */
export function sizeText(
  amount: number | null | undefined,
  from: SizeUnit = "GiB",
  decimals = 2,
): string {
  const f = formatSize(amount, from, decimals);
  return `${f.value} ${f.unit}`;
}
