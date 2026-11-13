const KB = 1024;
const MB = 1024 * 1024;
const GB = 1024 * 1024 * 1024;

export interface FormattedRate {
  value: string;
  unit: "B/s" | "KB/s" | "MB/s" | "GB/s";
}

/** A throughput in bytes a second, in the largest unit that keeps the
 *  number at 1 or more. */
export function formatRate(bytesPerSecond: number): FormattedRate {
  const n = Number.isFinite(bytesPerSecond) ? Math.max(0, bytesPerSecond) : 0;
  if (n >= GB) return { value: (n / GB).toFixed(2), unit: "GB/s" };
  if (n >= MB) return { value: (n / MB).toFixed(2), unit: "MB/s" };
  if (n >= KB) return { value: (n / KB).toFixed(1), unit: "KB/s" };
  return { value: String(Math.round(n)), unit: "B/s" };
}

/** ``formatRate`` as one string, e.g. ``"244.0 KB/s"``. */
export function rateText(bytesPerSecond: number): string {
  const f = formatRate(bytesPerSecond);
  return `${f.value} ${f.unit}`;
}
