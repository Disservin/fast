import { createRouter, createWebHashHistory } from "vue-router";

const routes = [
  {
    path: "/",
    name: "home",
    component: () => import("../views/HomePage.vue"),
  },
  {
    path: "/analysis",
    name: "analysis",
    component: () => import("../views/AnalysisPage.vue"),
  },
  {
    path: "/engines",
    name: "engines",
    component: () => import("../views/EnginePage.vue"),
  },
  {
    path: "/info",
    name: "info",
    component: () => import("../views/InfoPage.vue"),
  },
  {
    path: "/settings",
    name: "settings",
    component: () => import("../views/SettingsPage.vue"),
  },
];

const router = createRouter({
  // 4. Provide the history implementation to use. We are using the hash history for simplicity here.
  history: createWebHashHistory(),
  routes, // short for `routes: routes`
});

export default router;
