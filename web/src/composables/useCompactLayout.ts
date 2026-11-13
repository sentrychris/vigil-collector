/**
 * Whether the dashboard should be the compact one.
 *
 * The Vigil window opens small, as a desktop monitor that sits in a corner
 * of the screen, and at that size it shows a layout made for it: one tight
 * column where everything fits without scrolling. Made wide or maximised,
 * the same window shows the full dashboard. A browser always gets the full
 * dashboard, whatever its width.
 */
import { computed, ref } from "vue";
import { useWindowFrame } from "./useWindowFrame";

/** The window width, in CSS pixels, from which the full dashboard has the
 *  room it needs for two columns. */
export const FULL_FROM_PX = 900;

const width = ref(typeof window === "undefined" ? FULL_FROM_PX : window.innerWidth);
let listening = false;

export function useCompactLayout() {
  const { framed } = useWindowFrame();
  if (framed.value && !listening) {
    listening = true;
    window.addEventListener("resize", () => {
      width.value = window.innerWidth;
    });
  }
  const compact = computed(() => framed.value && width.value < FULL_FROM_PX);
  return { compact };
}
