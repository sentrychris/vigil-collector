/**
 * The names of a machine's parts, as the cards have room to show them.
 */

/** A graphics card's model without its maker: "NVIDIA GeForce RTX 3070"
 *  as "GeForce RTX 3070". */
export function gpuModel(name: string | null | undefined): string {
  const full = (name ?? "").trim();
  const model = full.replace(/^(NVIDIA|AMD|Intel)\s+/, "");
  return model || full;
}

/** What memory is fitted, as much as is known: "DDR4 · Corsair". */
export function memoryName(
  mem: { type?: string; manufacturer?: string } | null | undefined,
): string {
  return [mem?.type, mem?.manufacturer].filter(Boolean).join(" · ");
}
