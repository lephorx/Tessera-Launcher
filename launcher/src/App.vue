<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { RouterLink, RouterView, useRouter } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Blocks, ChevronDown, Compass, Home, Loader2, LogIn, LogOut, RefreshCw, Server, Settings as SettingsIcon, Shirt, UserRound } from "lucide-vue-next";
import { checkForUpdate, installUpdate, startUpdateChecks, updater } from "./lib/updater";
import markUrl from "./assets/mark.svg";
import markWhiteUrl from "./assets/mark-white.svg";
import Toast from "./components/AppToast.vue";
import AppDialog from "./components/AppDialog.vue";
import AccountCard from "./components/AccountCard.vue";
import ConsentDialog from "./components/ConsentDialog.vue";
import HomeSidebar from "./components/HomeSidebar.vue";
import SettingsDialog from "./components/SettingsDialog.vue";
import { openSettings, settingsDialog } from "./lib/settingsDialog";
import { api } from "./lib/api";
import { account, loadAccount, signOut } from "./lib/account";
import { TERMS_VERSION } from "./lib/legal";
import { packProgress, runs, startEvents } from "./lib/store";
import { loadPlayers } from "./lib/player";
import { errorText, toast } from "./lib/toast";

const scrolled = ref(false);
const router = useRouter();
const dropping = ref(false);
let unlistenDrop: (() => void) | undefined;

/** Drop a .mrpack or .tapack anywhere on the window to install it as a new instance. */
async function importDropped(paths: string[]) {
  // on the Skins page, dropped PNGs become saved skins
  const pngs = paths.filter((p) => /\.png$/i.test(p));
  if (pngs.length && router.currentRoute.value.path === "/skins") {
    window.dispatchEvent(new CustomEvent("tessera-skin-files", { detail: pngs }));
    return;
  }
  const packs = paths.filter((p) => /\.(mrpack|tapack)$/i.test(p));
  if (!packs.length) {
    toast.error("Drop a .mrpack or .tapack file to install it.");
    return;
  }
  for (const path of packs) {
    try {
      const inst = await api.projects.importPack(path);
      toast.success(`${inst.name} is ready`);
      router.push(`/instance/${inst.id}`);
    } catch (e) {
      toast.error(errorText(e));
    }
  }
}

const running = computed(() => Object.values(runs).filter((r) => r.state !== "idle").length);
const menuOpen = ref(false);

function closeMenu(e: MouseEvent) {
  if (!(e.target as HTMLElement).closest(".account-menu-wrap")) menuOpen.value = false;
}

async function doSignOut() {
  menuOpen.value = false;
  await signOut();
}

onMounted(async () => {
  window.addEventListener("mousedown", closeMenu);
  void loadAccount();
  void startUpdateChecks();
  await startEvents();
  // live sync of shared instances (see start_live_sync in lib.rs for why it starts here)
  void invoke("start_live_sync").catch(() => {});
  await loadPlayers();
  unlistenDrop = await getCurrentWebview().onDragDropEvent((e) => {
    if (e.payload.type === "enter" || e.payload.type === "over") dropping.value = true;
    else if (e.payload.type === "leave") dropping.value = false;
    else if (e.payload.type === "drop") {
      dropping.value = false;
      void importDropped(e.payload.paths);
    }
  });
});
onUnmounted(() => {
  unlistenDrop?.();
  window.removeEventListener("mousedown", closeMenu);
});

const nav = [
  { to: "/", label: "Home", icon: Home, exact: true },
  { to: "/library", label: "Library", icon: Blocks },
  { to: "/discover", label: "Discover", icon: Compass },
  { to: "/skins", label: "Skins", icon: Shirt },
  { to: "/servers", label: "Servers", icon: Server },
];
</script>

<template>
  <div class="shell">
    <header class="topbar" :class="{ scrolled }" data-tauri-drag-region>
      <div data-tauri-drag-region>
        <RouterLink to="/" class="brand" title="Home" aria-label="Tessera, go to Home">
          <span class="brand-tile"><img :src="markWhiteUrl" alt="" draggable="false" /></span>
          <img class="brand-mark" :src="markUrl" alt="" draggable="false" />
          tessera
        </RouterLink>
      </div>
      <nav class="nav">
        <RouterLink
          v-for="item in nav"
          :key="item.to"
          :to="item.to"
          :class="{ active: item.exact ? $route.path === item.to : $route.path.startsWith(item.to) }"
        >
          <component :is="item.icon" :size="18" />
          {{ item.label }}
        </RouterLink>
      </nav>
      <div class="topbar-right" data-tauri-drag-region>
        <button
          v-if="updater.available"
          class="update-badge"
          :disabled="updater.installing"
          :title="`Version ${updater.available.version} is ready`"
          @click="installUpdate"
        >
          <Loader2 v-if="updater.installing" class="spin" :size="16" /><RefreshCw v-else :size="16" />
          {{ updater.installing ? (updater.progress && !updater.ready ? `Updating ${Math.round(updater.progress * 100)}%` : "Updating…") : "Reload to update" }}
        </button>
        <button
          v-else
          class="btn btn-ghost btn-icon"
          :title="updater.current ? `Check for updates (you have ${updater.current})` : 'Check for updates'"
          aria-label="Check for updates"
          :disabled="updater.checking"
          @click="checkForUpdate(true)"
        >
          <RefreshCw :class="{ spin: updater.checking }" />
        </button>
        <RouterLink to="/library" class="run-status" :class="{ on: running }">
          <span class="run-dot" /><span class="run-text">{{ running ? `${running} running` : "Nothing running" }}</span>
        </RouterLink>
        <button class="btn btn-ghost btn-icon" title="Settings" aria-label="Settings" @click="openSettings()">
          <SettingsIcon />
        </button>
        <button v-if="account.loaded && !account.me" class="btn btn-primary" @click="account.dialog = true"><LogIn /> Sign in</button>
        <div v-else-if="account.me" class="account-menu-wrap">
          <button class="account-button" :aria-expanded="menuOpen" @click="menuOpen = !menuOpen">
            <span class="avatar">{{ (account.me.username ?? account.me.email).slice(0, 1) }}</span>
            <span class="ellipsis">{{ account.me.username ?? account.me.email }}</span>
            <ChevronDown :size="16" class="muted" />
          </button>
          <div v-if="menuOpen" class="account-menu" role="menu">
            <div class="account-menu-head">
              <strong class="ellipsis">{{ account.me.username ?? "No username yet" }}</strong>
              <span class="hint ellipsis">{{ account.me.email }}</span>
            </div>
            <button class="account-menu-item" role="menuitem" @click="(menuOpen = false), openSettings('profile')"><UserRound /> Account settings</button>
            <button class="account-menu-item" role="menuitem" @click="doSignOut"><LogOut /> Sign out</button>
          </div>
        </div>
      </div>
    </header>
    <div class="body">
      <main class="content" @scroll="scrolled = ($event.target as HTMLElement).scrollTop > 4">
        <RouterView v-slot="{ Component, route }">
          <component :is="Component" :key="route.fullPath" />
        </RouterView>
      </main>
      <HomeSidebar v-if="$route.path === '/'" />
    </div>
    <SettingsDialog v-if="settingsDialog.open" />
    <AppDialog v-if="account.dialog" title="Tessera account" :width="440" @close="account.dialog = false">
      <AccountCard bare @signed-in="account.dialog = false" />
    </AppDialog>
    <!-- members from before the current Terms agree once (or sign out) -->
    <ConsentDialog v-if="account.me?.member && account.me.terms_version !== TERMS_VERSION" />
    <div v-if="packProgress.stage" class="pack-progress stack" style="gap: 8px" role="status">
      <div class="row"><span style="font-weight: 600">{{ packProgress.stage }}</span><span class="spacer" /><span class="hint num">{{ packProgress.total ? `${packProgress.done} / ${packProgress.total}` : "" }}</span></div>
      <div class="progress"><div :style="{ width: packProgress.total ? `${(packProgress.done / packProgress.total) * 100}%` : '8%' }" /></div>
    </div>
    <div v-if="dropping" class="drop-zone" aria-hidden="true">
      <div class="card stack" style="align-items: center; gap: 6px; padding: 28px 36px">
        <template v-if="$route.path === '/skins'">
          <span style="font-weight: 800; font-size: 18px">Drop to add skins</span>
          <span class="hint">64×64 PNG skin files</span>
        </template>
        <template v-else>
          <span style="font-weight: 800; font-size: 18px">Drop to install</span>
          <span class="hint">.mrpack (Modrinth) or .tapack (Tessera) modpacks</span>
        </template>
      </div>
    </div>
    <Toast />
  </div>
</template>
