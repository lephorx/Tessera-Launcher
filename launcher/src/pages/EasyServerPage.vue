<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { RouterLink, useRouter } from "vue-router";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowLeft, Copy, Cpu, ExternalLink, FolderOpen, Globe, Laptop, Loader2, MemoryStick, Play, RotateCw, Square, Timer, Trash2, Users, Wifi, Zap } from "lucide-vue-next";
import AppDialog from "@/components/AppDialog.vue";
import EasyBackups from "@/components/easy/EasyBackups.vue";
import EasyConsole from "@/components/easy/EasyConsole.vue";
import EasyFiles from "@/components/easy/EasyFiles.vue";
import EasyPlayers from "@/components/easy/EasyPlayers.vue";
import EasyProperties from "@/components/easy/EasyProperties.vue";
import EasySettings from "@/components/easy/EasySettings.vue";
import { api, type EasyServer } from "@/lib/api";
import { duration, head, KIND, STATE } from "@/lib/easy";
import { ensureAccount } from "@/lib/player";
import { bytes } from "@/lib/settingsDialog";
import { runs } from "@/lib/store";
import { errorText, toast } from "@/lib/toast";

/**
 * A server the launcher runs on this computer, with what Tessera's dashboard has: overview
 * (address, CPU, memory, who's on), console, players, backups, files, properties and settings.
 * It keeps running when the launcher quits.
 */
const props = defineProps<{ id: string }>();
const router = useRouter();

type Tab = "overview" | "console" | "players" | "backups" | "files" | "properties" | "settings";
const TABS: { id: Tab; label: string }[] = [
  { id: "overview", label: "Overview" },
  { id: "console", label: "Console" },
  { id: "players", label: "Players" },
  { id: "backups", label: "Backups" },
  { id: "files", label: "Files" },
  { id: "properties", label: "Properties" },
  { id: "settings", label: "Settings" },
];

const server = ref<EasyServer | null>(null);
const icon = ref<string | null>(null);
const busy = ref(false);
const tab = ref<Tab>("overview");
const confirmDelete = ref(false);
const confirmKill = ref(false);
const deleting = ref(false);

const state = computed(() => STATE[server.value?.state ?? "stopped"]!);
const localAddress = computed(() => (server.value ? (server.value.port === 25565 ? "localhost" : `localhost:${server.value.port}`) : ""));

async function load() {
  try {
    server.value = await api.easy.get(props.id);
  } catch (e) {
    if (deleting.value) return;
    toast.error(errorText(e));
    void router.replace("/servers");
  }
}

async function loadIcon() {
  icon.value = await api.easy.icon(props.id).catch(() => null);
}

const now = ref(Date.now());
let unlisten: (() => void) | undefined;
let timer: ReturnType<typeof setInterval> | undefined;
onMounted(async () => {
  await load();
  void loadIcon();
  unlisten = await listen<{ id: string; line: string | null }>("easy-server", (e) => {
    if (e.payload.id === props.id && e.payload.line === null) void load();
  });
  // CPU, memory and uptime
  timer = setInterval(() => {
    now.value = Date.now();
    if (server.value && server.value.state !== "stopped") void load();
  }, 2000);
});
onUnmounted(() => {
  unlisten?.();
  clearInterval(timer);
});

async function run(f: () => Promise<void>) {
  busy.value = true;
  try {
    await f();
    await load();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = false;
  }
}

const start = () => run(() => api.easy.start(props.id));
const stop = () => run(() => api.easy.stop(props.id));
const restart = () => run(() => api.easy.restart(props.id));
function kill() {
  confirmKill.value = false;
  void run(() => api.easy.kill(props.id));
}

async function copy(text: string) {
  await navigator.clipboard.writeText(text);
  toast.success("Address copied");
}

/** Plays the instance the server was made from, straight onto this server. */
async function join() {
  const inst = server.value?.instance;
  if (!inst || !(await ensureAccount())) return;
  runs[inst] = { state: "preparing", stage: "Preparing", done: 0, total: 0 };
  try {
    await api.instances.launch(inst, null, localAddress.value);
  } catch (e) {
    delete runs[inst];
    toast.error(errorText(e));
  }
}

async function remove() {
  deleting.value = true;
  try {
    await api.easy.remove(props.id);
    confirmDelete.value = false;
    toast.success("Server deleted");
    void router.replace("/servers");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    deleting.value = false;
  }
}

function openFolder() {
  api.easy.openFolder(props.id).catch((e) => toast.error(errorText(e)));
}

function changed() {
  void load();
  void loadIcon();
}

const uptime = computed(() => (server.value?.started_at ? duration((now.value - new Date(server.value.started_at).getTime()) / 1000) : "–"));
</script>

<template>
  <div v-if="server" class="page enter">
    <RouterLink to="/servers" class="row hint" style="margin-bottom: 14px; color: var(--tint); font-weight: 600"><ArrowLeft :size="14" /> Servers</RouterLink>
    <div class="page-header">
      <div class="row" style="gap: 14px; min-width: 0">
        <span class="server-icon">
          <img v-if="icon" :src="icon" alt="" />
          <Laptop v-else :size="26" />
        </span>
        <div style="min-width: 0">
          <h1 class="page-title ellipsis">{{ server.name }}</h1>
          <div class="row" style="flex-wrap: wrap; margin-top: 6px">
            <span class="badge" :data-state="state.key"><span class="dot" />{{ state.label }}</span>
            <span class="chip">{{ KIND[server.kind] }}</span>
            <span class="chip num">{{ server.game_version }}</span>
            <span v-if="server.loader_version && server.kind !== 'vanilla'" class="chip num">{{ server.loader_version }}</span>
            <span class="chip">On this computer</span>
          </div>
        </div>
      </div>
      <div class="row" style="flex-wrap: wrap; justify-content: flex-end">
        <button class="btn btn-lg" title="Open the server's folder" @click="openFolder"><FolderOpen /></button>
        <button class="btn btn-lg" title="Delete the server" @click="confirmDelete = true"><Trash2 /></button>
        <button v-if="server.instance && server.state === 'running'" class="btn btn-lg" @click="join"><Play /> Join</button>
        <template v-if="server.state === 'stopped'">
          <button class="btn btn-play btn-lg" :disabled="busy" @click="start"><Loader2 v-if="busy" class="spin" /><Play v-else /> Start</button>
        </template>
        <template v-else>
          <button class="btn btn-lg" :disabled="busy || server.state !== 'running'" title="Restart" @click="restart"><RotateCw /></button>
          <button class="btn btn-lg" title="Stop at once, without saving" @click="confirmKill = true"><Zap /></button>
          <button class="btn btn-stop btn-lg" :disabled="busy || server.state === 'stopping'" @click="stop"><Loader2 v-if="busy || server.state === 'stopping'" class="spin" /><Square v-else /> Stop</button>
        </template>
      </div>
    </div>

    <div class="tabs" style="margin: 4px 0 16px">
      <button v-for="t in TABS" :key="t.id" :class="{ active: tab === t.id }" @click="tab = t.id">{{ t.label }}</button>
    </div>

    <template v-if="tab === 'overview'">
      <div class="stat-grid">
        <div class="card stat">
          <span class="hint row" style="gap: 6px"><Users :size="14" /> Players</span>
          <b class="num">{{ server.players.length }} / {{ server.max_players }}</b>
        </div>
        <div class="card stat">
          <span class="hint row" style="gap: 6px"><Cpu :size="14" /> CPU</span>
          <b class="num">{{ server.state === "stopped" ? "–" : `${Math.round(server.cpu)} %` }}</b>
        </div>
        <div class="card stat">
          <span class="hint row" style="gap: 6px"><MemoryStick :size="14" /> Memory</span>
          <b class="num">{{ server.state === "stopped" ? "–" : bytes(server.memory_used) }}</b>
          <div class="progress" style="height: 4px"><div :style="{ width: `${Math.min(100, (server.memory_used / (server.memory_mb * 1048576)) * 100)}%` }" /></div>
          <span class="hint">of {{ server.memory_mb / 1024 }} GB</span>
        </div>
        <div class="card stat">
          <span class="hint row" style="gap: 6px"><Timer :size="14" /> Running for</span>
          <b class="num">{{ server.state === "stopped" ? "–" : uptime }}</b>
        </div>
      </div>

      <div class="easy-grid" style="margin-top: 14px">
        <div class="card stack">
          <h3 class="row" style="gap: 8px"><Wifi :size="18" /> Join</h3>
          <div class="inset row">
            <span class="mono selectable" style="flex: 1; font-weight: 600">{{ localAddress }}</span>
            <span class="hint">You, on this computer</span>
            <button class="btn btn-ghost btn-icon btn-sm" title="Copy address" @click="copy(localAddress)"><Copy /></button>
          </div>
          <div v-if="server.lan_address" class="inset row">
            <span class="mono selectable" style="flex: 1; font-weight: 600">{{ server.lan_address }}</span>
            <span class="hint">Same Wi-Fi</span>
            <button class="btn btn-ghost btn-icon btn-sm" title="Copy address" @click="copy(server.lan_address)"><Copy /></button>
          </div>
          <p class="hint">In Minecraft: Multiplayer → Add Server, and paste the address. The first time it starts, Windows may ask whether Java may use the network: allow it.</p>
        </div>

        <div class="card stack">
          <h3 class="row" style="gap: 8px"><Globe :size="18" /> Friends somewhere else</h3>
          <p class="hint" style="margin: 0">
            Give the server an address that works from anywhere with Playit (free): install the Playit app, sign in, and add a
            <b>Minecraft Java</b> tunnel to port <b class="mono">{{ server.port }}</b>. Playit shows the address to send your friends.
          </p>
          <button class="btn" style="align-self: flex-start" @click="openUrl('https://playit.gg/download')"><ExternalLink /> Get Playit</button>
        </div>

        <div class="card stack">
          <h3 class="row" style="gap: 8px"><Users :size="18" /> Online <span class="hint num">{{ server.players.length }}</span></h3>
          <p v-if="!server.players.length" class="hint" style="margin: 0">{{ server.state === "running" ? "Nobody's on right now." : server.state === "stopped" ? "Start the server to play." : "Starting…" }}</p>
          <button v-for="p in server.players" :key="p" class="row player" @click="tab = 'players'"><img :src="head(p)" width="24" height="24" alt="" /> {{ p }}</button>
          <p class="hint" style="margin: 0">The server keeps running when you close the launcher; stop it here.</p>
        </div>
      </div>
    </template>

    <EasyConsole v-else-if="tab === 'console'" :server="server" />
    <EasyPlayers v-else-if="tab === 'players'" :server="server" @changed="changed" />
    <EasyBackups v-else-if="tab === 'backups'" :server="server" @changed="changed" />
    <EasyFiles v-else-if="tab === 'files'" :server="server" />
    <EasyProperties v-else-if="tab === 'properties'" :server="server" />
    <EasySettings v-else :key="`${server.kind}-${server.game_version}-${server.loader_version}`" :server="server" :icon="icon" @changed="changed" @delete="confirmDelete = true" />

    <AppDialog v-if="confirmDelete" :title="`Delete ${server.name}?`" :width="420" @close="!deleting && (confirmDelete = false)">
      <p class="muted">
        The server, its world and its backups are deleted from this computer.
        <template v-if="server.state !== 'stopped'"> It's running: it's closed first, without saving.</template>
        This can't be undone.
      </p>
      <div class="dialog-footer">
        <button class="btn" :disabled="deleting" @click="confirmDelete = false">Cancel</button>
        <button class="btn btn-danger" :disabled="deleting" @click="remove"><Loader2 v-if="deleting" class="spin" /> {{ deleting ? "Deleting…" : "Delete" }}</button>
      </div>
    </AppDialog>

    <AppDialog v-if="confirmKill" title="Stop at once?" :width="420" @close="confirmKill = false">
      <p class="muted">The server ends right away, without saving. Anything since the last save (every few minutes) is lost. Use Stop to save first.</p>
      <div class="dialog-footer">
        <button class="btn" @click="confirmKill = false">Cancel</button>
        <button class="btn btn-danger" @click="kill">Stop at once</button>
      </div>
    </AppDialog>
  </div>
  <div v-else class="empty"><Loader2 class="spin" /></div>
</template>

<style scoped>
.easy-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 14px;
}
.easy-grid h3 {
  font-size: 15px;
  font-weight: 700;
}
.stat-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
  gap: 12px;
}
.stat {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.stat b {
  font-size: 22px;
}
.server-icon {
  width: 56px;
  height: 56px;
  flex-shrink: 0;
  border-radius: 14px;
  background: var(--muted);
  display: grid;
  place-items: center;
  overflow: hidden;
}
.server-icon img {
  width: 56px;
  height: 56px;
  image-rendering: pixelated;
}
.player {
  gap: 8px;
  font-weight: 600;
  border: 0;
  background: none;
  color: inherit;
  font: inherit;
  cursor: pointer;
  padding: 2px 0;
}
</style>
