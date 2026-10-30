<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { openPath } from "@tauri-apps/plugin-opener";
import { Copy, FolderOpen, Globe, Loader2, Play, RefreshCw, Search, Server, Trash2 } from "lucide-vue-next";
import { api, type Instance, type LinkedServer, type ServerEntry, type World } from "@/lib/api";
import { relative } from "@/lib/format";
import { runOf, runs, serverSync } from "@/lib/store";
import { errorText, toast } from "@/lib/toast";
import AppDialog from "./AppDialog.vue";

const props = defineProps<{ instance: Instance }>();
const worlds = ref<World[] | null>(null);
const query = ref("");
const deleting = ref<World | null>(null);
const busy = ref(false);
const run = computed(() => runOf(props.instance.id));
const shown = computed(() => (worlds.value ?? []).filter((w) => w.folder.toLowerCase().includes(query.value.trim().toLowerCase())));
// quick play (straight into a world) exists since 1.20
const quickPlay = computed(() => {
  const [maj, min] = props.instance.game_version.split(".").map(Number);
  return (maj ?? 0) > 1 || (min ?? 0) >= 20;
});

// Servers: the ones made from this instance (CSCM), then the rest of the game's server list.
const linked = ref<LinkedServer[] | null>(null);
const others = ref<ServerEntry[]>([]);

async function loadServers() {
  try {
    linked.value = props.instance.servers?.length ? await api.instances.linkedServers(props.instance.id) : [];
  } catch {
    linked.value = [];
  }
  const mine = new Set(linked.value.flatMap((s) => [s.addresses.public, s.addresses.lan, s.addresses.playit].filter(Boolean)));
  others.value = (await api.instances.serverList(props.instance.id)).filter((e) => !mine.has(e.ip));
}

async function load() {
  worlds.value = await api.instances.worldsDetail(props.instance.id);
}
onMounted(() => {
  void load();
  void loadServers();
});

const running = (s: LinkedServer) => s.runtime_status === "healthy" || s.runtime_status === "running";
const statusText = (s: LinkedServer) => {
  const v = s.runtime_status;
  if (v === null) return "Can't reach the machine";
  if (running(s)) return "Online";
  if (v === "starting" || v === "unhealthy") return "Starting";
  return "Offline";
};
const syncOf = (s: LinkedServer) => serverSync[`${props.instance.id}:${s.server_id}`];
const syncing = ref(false);

async function syncNow() {
  syncing.value = true;
  try {
    await api.instances.syncServers(props.instance.id);
  } finally {
    syncing.value = false;
  }
}

async function join(address: string) {
  runs[props.instance.id] = { state: "preparing", stage: "Preparing", done: 0, total: 0 };
  try {
    await api.instances.launch(props.instance.id, null, address);
  } catch (e) {
    delete runs[props.instance.id];
    toast.error(errorText(e));
  }
}

async function copy(text: string) {
  await navigator.clipboard.writeText(text);
  toast.success("Address copied");
}

async function play(w: World) {
  runs[props.instance.id] = { state: "preparing", stage: "Preparing", done: 0, total: 0 };
  try {
    await api.instances.launch(props.instance.id, w.folder);
  } catch (e) {
    delete runs[props.instance.id];
    toast.error(errorText(e));
  }
}

async function openFolder(w: World) {
  const game = await api.instances.folder(props.instance.id);
  await openPath(`${game}/saves/${w.folder}`);
}

async function remove() {
  if (!deleting.value) return;
  busy.value = true;
  try {
    await api.instances.deleteWorld(props.instance.id, deleting.value.folder);
    toast.success(`${deleting.value.folder} deleted`);
    deleting.value = null;
    await load();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = false;
  }
}

const size = (n: number) => (n > 1e9 ? `${(n / 1e9).toFixed(1)} GB` : n > 1e6 ? `${(n / 1e6).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1e3))} KB`);
</script>

<template>
  <div class="stack">
    <template v-if="(linked && linked.length) || others.length">
      <h3 class="wp-title">Servers</h3>
      <div v-for="s in linked ?? []" :key="`${s.machine_id}:${s.server_id}`" class="card row" style="gap: 14px">
        <span class="tile" style="--type: #959eef"><Server /></span>
        <div style="flex: 1; min-width: 0" class="stack wp-server">
          <div class="row" style="gap: 8px">
            <span class="wp-name">{{ s.name }}</span>
            <span class="badge" :data-state="running(s) ? 'running' : s.runtime_status === 'starting' ? 'preparing' : 'idle'"><span class="dot" />{{ statusText(s) }}</span>
          </div>
          <div v-if="s.addresses.public || s.addresses.lan" class="row" style="gap: 6px">
            <span class="mono selectable wp-address">{{ s.addresses.public ?? s.addresses.lan }}</span>
            <button class="btn btn-ghost btn-icon btn-sm" title="Copy address" @click="copy((s.addresses.public ?? s.addresses.lan)!)"><Copy /></button>
          </div>
          <span class="hint">
            Your server on {{ s.machine_name }}<template v-if="s.addresses.playit"> · also {{ s.addresses.playit }}</template
            ><template v-if="s.addresses.public && s.addresses.lan"> · on your network {{ s.addresses.lan }}</template>
          </span>
          <span v-if="syncOf(s)?.state === 'syncing'" class="hint row" style="gap: 6px"><Loader2 class="spin" :size="12" /> Sending the new mods…</span>
          <span v-else-if="syncOf(s)?.state === 'failed'" class="error-text">{{ syncOf(s)?.message }}</span>
          <span v-else class="hint">Gets this instance's server-side mods automatically when you change them.</span>
        </div>
        <button class="btn btn-ghost btn-icon" title="Send the mods to the server now" :disabled="syncing || syncOf(s)?.state === 'syncing'" @click="syncNow">
          <RefreshCw :class="{ spin: syncing }" />
        </button>
        <button
          class="btn btn-play"
          :disabled="run.state !== 'idle' || !(s.addresses.public ?? s.addresses.lan)"
          :title="`Join ${s.name}`"
          @click="join((s.addresses.public ?? s.addresses.lan)!)"
        >
          <Play /> Join
        </button>
      </div>
      <div v-for="e in others" :key="e.ip" class="card row" style="gap: 14px">
        <span class="tile" style="--type: #7dd3fc"><Server /></span>
        <div style="flex: 1; min-width: 0">
          <div class="wp-name">{{ e.name || e.ip }}</div>
          <div class="hint mono">{{ e.ip }}</div>
        </div>
        <button class="btn btn-play" :disabled="run.state !== 'idle'" :title="`Join ${e.name || e.ip}`" @click="join(e.ip)"><Play /> Join</button>
      </div>
      <h3 class="wp-title">Worlds</h3>
    </template>

    <div v-if="worlds && worlds.length" class="input-icon">
      <Search />
      <input v-model="query" class="input" placeholder="Search worlds…" />
    </div>
    <div v-if="worlds === null" class="empty"><Loader2 class="spin" /></div>
    <div v-else-if="!worlds.length" class="card empty">
      <Globe />
      <h3>No worlds yet</h3>
      <p>Play the instance and create a world; it shows up here.</p>
    </div>
    <div v-for="w in shown" :key="w.folder" class="card row" style="gap: 14px">
      <img v-if="w.icon" :src="w.icon" alt="" width="56" height="56" style="border-radius: 12px; image-rendering: pixelated" />
      <span v-else class="tile" style="--type: #7dd87d"><Globe /></span>
      <div style="flex: 1; min-width: 0">
        <div style="font-weight: 700; font-size: 16px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap">{{ w.folder }}</div>
        <div class="hint num">{{ w.last_played ? `Played ${relative(w.last_played).toLowerCase()}` : "Never played" }} · {{ size(w.size) }}</div>
      </div>
      <button class="btn" title="Open folder" @click="openFolder(w)"><FolderOpen /></button>
      <button class="btn btn-ghost btn-icon" :title="`Delete ${w.folder}`" :disabled="run.state !== 'idle'" @click="deleting = w"><Trash2 /></button>
      <button
        class="btn btn-play"
        :disabled="run.state !== 'idle' || !quickPlay"
        :title="quickPlay ? `Play ${w.folder}` : 'Opening a world directly needs Minecraft 1.20 or newer'"
        @click="play(w)"
      >
        <Play /> Play
      </button>
    </div>

    <AppDialog v-if="deleting" :title="`Delete ${deleting.folder}?`" @close="deleting = null">
      <p class="muted">The world and everything built in it is deleted from this computer. This can't be undone.</p>
      <div class="dialog-footer">
        <button class="btn" @click="deleting = null">Cancel</button>
        <button class="btn btn-danger" :disabled="busy" @click="remove">Delete world</button>
      </div>
    </AppDialog>
  </div>
</template>

<style scoped>
.wp-title {
  margin: 8px 0 0;
  font-size: 15px;
  font-weight: 700;
}
.wp-title:first-child {
  margin-top: 0;
}
.wp-server {
  gap: 2px;
}
.wp-name {
  font-weight: 700;
  font-size: 16px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.wp-address {
  font-size: 13px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
