import { defineStore } from "pinia";
import { useWindowFrame } from "@/composables/useWindowFrame";

const body = <HTMLBodyElement>document.querySelector("body");

export const useThemeStore = defineStore("theme", {
  state: () => ({
    theme: <string | null>null,
    active: body.dataset.theme,
  }),
  actions: {
    load() {
      // Brand is dark-first (BRANDING §3.2); default new visitors to dark.
      // Existing visitors with a saved preference (light or dark) are
      // restored by Pinia persist before this runs, so this only kicks
      // in on first load with nothing in localStorage.
      const theme = this.theme ?? "dark";
      body.dataset.theme = theme;

      this.theme = theme;
      this.active = this.getActiveTheme();
    },
    toggle() {
      this.set(this.theme === "light" ? "dark" : "light");
      // In the Vigil window the compact window keeps the same theme (see
      // src/compact/ in the collector). In a browser this does nothing.
      useWindowFrame().ask(this.theme === "light" ? "theme:light" : "theme:dark");
    },
    /** Changes to the light theme or the dark one. */
    set(theme: "light" | "dark") {
      this.theme = theme;
      body.dataset.theme = theme;
      this.active = this.getActiveTheme();
    },
    getActiveTheme() {
      return body.dataset.theme;
    },
  },
  persist: {
    storage: localStorage,
    pick: ["theme", "active"],
  },
});
