import { defineStore } from "pinia";

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
      this.theme = this.theme === "light" ? "dark" : "light";
      body.dataset.theme = this.theme;
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
