import { createRouter, createWebHistory } from "vue-router";
import { config } from "@/config";
import { useControlStore } from "@/stores/control";
import DashboardView from "../views/DashboardView.vue";

const router = createRouter({
  history: createWebHistory(config.app.base_url),
  routes: [
    {
      path: "/",
      name: "dashboard",
      component: DashboardView,
    },
    {
      // Only in the Vigil window, which has the control API behind it. In
      // a browser this is one more unknown address and lands on the
      // dashboard. Loaded on demand so browsers never download it.
      path: "/settings",
      name: "settings",
      component: () => import("../views/SettingsView.vue"),
      beforeEnter: async () => ((await useControlStore().detect()) ? true : "/"),
    },
    // Anything else (an old /hub bookmark, a typo) lands on the dashboard.
    { path: "/:pathMatch(.*)*", redirect: "/" },
  ],
});

export default router;
