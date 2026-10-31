import { createRouter, createWebHashHistory } from "vue-router";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", component: () => import("./pages/HomePage.vue") },
    { path: "/library", component: () => import("./pages/LibraryPage.vue") },
    { path: "/instance/:id", component: () => import("./pages/InstancePage.vue"), props: true },
    // "Add content": Discover for one instance; everything clicked goes straight in
    { path: "/instance/:id/add", component: () => import("./pages/DiscoverPage.vue"), props: (r) => ({ instanceId: r.params.id }) },
    { path: "/discover", component: () => import("./pages/DiscoverPage.vue") },
    { path: "/servers", component: () => import("./pages/ServersPage.vue") },
    { path: "/servers/host", component: () => import("./pages/HostPage.vue") },
    // easy servers, run by the launcher itself (no Docker)
    { path: "/servers/new", component: () => import("./pages/EasyCreatePage.vue"), props: (r) => ({ instanceId: r.query.instance ?? null }) },
    { path: "/servers/local/:id", component: () => import("./pages/EasyServerPage.vue"), props: true },
    { path: "/servers/:id", component: () => import("./pages/MachinePage.vue"), props: true },
    { path: "/skins", component: () => import("./pages/SkinsPage.vue") },
    // Settings is a popup now (App.vue); old links land on Home
    { path: "/settings", redirect: "/" },
  ],
});
