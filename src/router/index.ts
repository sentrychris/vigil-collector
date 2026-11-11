import { createRouter, createWebHistory } from "vue-router";
import { config } from "@/config";
import DashboardView from "../views/DashboardView.vue";

const router = createRouter({
  history: createWebHistory(config.app.base_url),
  routes: [
    {
      path: "/",
      name: "dashboard",
      component: DashboardView,
    },
    // Anything else (an old /hub bookmark, a typo) lands on the dashboard.
    { path: "/:pathMatch(.*)*", redirect: "/" },
  ],
});

export default router;
