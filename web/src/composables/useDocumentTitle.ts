/**
 * Reactive page-title composable.
 *
 * Each view declares its own title once in `<script setup>`; the
 * composable handles the reactivity (re-runs when a getter's deps
 * change) and appends a consistent app suffix so callers don't have
 * to repeat it.
 *
 *   useDocumentTitle("Fleet overview");          // → "Fleet overview | Vigil"
 *   useDocumentTitle(() => host.value?.name);    // tracks reactive deps
 */
import { toValue, watchEffect } from "vue";
import type { MaybeRefOrGetter } from "vue";
import { config } from "@/config";

const APP_SUFFIX = config.app.name ? ` | ${config.app.name}` : "";

export function useDocumentTitle(
  value: MaybeRefOrGetter<string | null | undefined>,
): void {
  watchEffect(() => {
    const v = toValue(value);
    document.title = v ? `${v}${APP_SUFFIX}` : (config.app.name || document.title);
  });
}
