import HomeView from "../views/Home.vue";
import { createRouter, createWebHashHistory } from 'vue-router'

const routes = [
    {
        path: "/",
        name: "home",
        component: HomeView,
    },
    {
        path: "/analysis",
        name: "analysis",
        component: () => import("../views/Analysis.vue"),
    },
    {
        path: "/engines",
        name: "engines",
        component: () => import("../views/Engines.vue"),
    },
    {
        path: "/info",
        name: "info",
        component: () => import("../views/Info.vue"),
    },
    {
        path: "/settings",
        name: "settings",
        component: () => import("../views/Settings.vue"),
    },
];

const router = createRouter({
    // 4. Provide the history implementation to use. We are using the hash history for simplicity here.
    history: createWebHashHistory(),
    routes, // short for `routes: routes`
})

export default router;
