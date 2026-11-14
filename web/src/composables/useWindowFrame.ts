/**
 * The Vigil window's title bar, from the page's side.
 *
 * The desktop window has no title bar of its own: the navbar draws the
 * minimise, maximise and close buttons and is what the window is dragged
 * by. The window gives the page `window.ipc.postMessage` to ask for those
 * (see src/window.rs in the collector), and sends a `vigil:window` event
 * whenever it is maximised or put back. The page also says when its theme
 * is changed, so the collector's own windows can follow, and asks for the
 * Settings window when the gear is pressed. In a browser there is no
 * `window.ipc`, `framed` is false and nothing here does anything.
 */
import { readonly, ref } from "vue";

export type WindowAsk =
  | "state"
  | "minimize"
  | "maximize"
  | "close"
  | "drag"
  | "resize:top"
  | "resize:top-left"
  | "resize:top-right"
  | "settings"
  | "theme:dark"
  | "theme:light";

interface Ipc {
  postMessage(message: string): void;
}

function ipc(): Ipc | null {
  const candidate = (window as unknown as { ipc?: Ipc }).ipc;
  return candidate && typeof candidate.postMessage === "function" ? candidate : null;
}

const framed = ref(ipc() !== null);
const maximized = ref(false);
let listening = false;

function ask(what: WindowAsk) {
  ipc()?.postMessage(what);
}

export function useWindowFrame() {
  if (framed.value && !listening) {
    listening = true;
    window.addEventListener("vigil:window", (event) => {
      const detail = (event as CustomEvent<{ maximized?: unknown }>).detail;
      maximized.value = detail?.maximized === true;
    });
    // The window may have been maximised before this page loaded.
    ask("state");
  }
  return { framed: readonly(framed), maximized: readonly(maximized), ask };
}
