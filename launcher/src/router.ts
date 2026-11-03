import { createRouter, createWebHashHistory } from "vue-router";

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", component: () => import("./pages/HomePage.vue") },
    { path: "/library", component: () => import("./pages/LibraryPage.vue") },
    { path: "/instance/:id", component: () => import("./pages/InstancePage.vue"), props: true },
    // "Add content": Discover for one instance; everything clicked goes straight in
    { path: "/instance/:id/add", component: () => import("./pages/DiscoverPage.vue"), props: (r) => ({ instanceId: r.params.id }) },
    // a Horizon instance's mods, from Horizon's own registry (Nightly)
    { path: "/instance/:id/horizon", component: () => import("./pages/HorizonModsPage.vue"), props: (r) => ({ instanceId: r.params.id }) },
    { path: "/discover", component: () => import("./pages/DiscoverPage.vue") },
    // a Modrinth project (id or slug): versions, what's in a modpack; ?instance= from "Add content"
    { path: "/project/:id", component: () => import("./pages/ProjectPage.vue"), props: (r) => ({ id: r.params.id, instanceId: r.query.instance ?? null }) },
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
