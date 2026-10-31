<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";
import { RouterLink, useRouter } from "vue-router";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowLeft, Copy, ExternalLink, FolderOpen, Globe, Loader2, Play, Send, Square, Trash2, Users, Wifi } from "lucide-vue-next";
import AppDialog from "@/components/AppDialog.vue";
import { api, type EasyServer } from "@/lib/api";
import { ensureAccount } from "@/lib/player";
import { runs } from "@/lib/store";
import { errorText, toast } from "@/lib/toast";

/** A server the launcher runs itself: start and stop, who's on, the console and its settings. */
const props = defineProps<{ id: string }>();
const router = useRouter();

const server = ref<EasyServer | null>(null);
const lines = ref<string[]>([]);
const command = ref("");
const busy = ref(false);
const tab = ref<"console" | "settings">("console");
const confirmDelete = ref(false);
const consoleEl = ref<HTMLElement | null>(null);

const KIND: Record<string, string> = { vanilla: "Vanilla", paper: "Paper", fabric: "Fabric", forge: "Forge", neoforge: "NeoForge" };
const STATE: Record<string, { key: string; label: string }> = {
  stopped: { key: "idle", label: "Stopped" },
  starting: { key: "preparing", label: "Starting…" },
  running: { key: "running", label: "Online" },
  stopping: { key: "preparing", label: "Stopping…" },
};
const state = computed(() => STATE[server.value?.state ?? "stopped"]!);
const localAddress = computed(() => (server.value ? (server.value.port === 25565 ? "localhost" : `localhost:${server.value.port}`) : ""));

async function load() {
  try {
    server.value = await api.easy.get(props.id);
  } catch (e) {
    toast.error(errorText(e));
    void router.replace("/servers");
  }
}

async function scrollDown() {
  await nextTick();
  const el = consoleEl.value;
  if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 80) el.scrollTop = el.scrollHeight;
}

let unlisten: (() => void) | undefined;
onMounted(async () => {
  await load();
  lines.value = await api.easy.log(props.id);
  void nextTick(() => consoleEl.value && (consoleEl.value.scrollTop = consoleEl.value.scrollHeight));
  unlisten = await listen<{ id: string; line: string | null }>("easy-server", (e) => {
    if (e.payload.id !== props.id) return;
    if (e.payload.line === null) void load();
    else {
      lines.value.push(e.payload.line);
      if (lines.value.length > 3000) lines.value.splice(0, lines.value.length - 3000);
      void scrollDown();
    }
  });
  void loadSettings();
});
onUnmounted(() => unlisten?.());

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

const start = () =>
  run(async () => {
    lines.value = [];
    await api.easy.start(props.id);
  });
const stop = () => run(() => api.easy.stop(props.id));

async function send() {
  const line = command.value.trim();
  if (!line) return;
  try {
    await api.easy.send(props.id, line);
    command.value = "";
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function openFolder() {
  try {
    await api.easy.openFolder(props.id);
  } catch (e) {
    toast.error(errorText(e));
  }
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

const lineClass = (l: string) => (/\b(ERROR|FATAL|Exception)\b/.test(l) ? "error" : /\bWARN\b/.test(l) ? "warn" : "");

// ── settings ──

const form = ref({ name: "", memGb: 4, port: 25565, maxPlayers: 20, difficulty: "easy", gamemode: "survival", pvp: true, whitelist: false });
const saving = ref(false);

async function loadSettings() {
  if (!server.value) return;
  const p = await api.easy.properties(props.id).catch(() => ({}) as Record<string, string>);
  form.value = {
    name: server.value.name,
    memGb: Math.round(server.value.memory_mb / 1024),
    port: server.value.port,
    maxPlayers: Number(p["max-players"] ?? 20),
    difficulty: p["difficulty"] ?? "easy",
    gamemode: p["gamemode"] ?? "survival",
    pvp: (p["pvp"] ?? "true") === "true",
    whitelist: (p["white-list"] ?? "false") === "true",
  };
}

async function saveSettings() {
  saving.value = true;
  try {
    const f = form.value;
    server.value = await api.easy.update(props.id, {
      name: f.name,
      memory_mb: f.memGb * 1024,
      port: f.port,
      max_players: f.maxPlayers,
      difficulty: f.difficulty,
      gamemode: f.gamemode,
      pvp: f.pvp,
      whitelist: f.whitelist,
    });
    toast.success(server.value.state === "stopped" ? "Saved" : "Saved. Restart the server to use the new settings.");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    saving.value = false;
  }
}

async function remove() {
  confirmDelete.value = false;
  try {
    await api.easy.remove(props.id);
    toast.success("Server deleted");
    void router.replace("/servers");
  } catch (e) {
    toast.error(errorText(e));
  }
}
</script>

<template>
  <div v-if="server" class="page enter">
    <RouterLink to="/servers" class="row hint" style="margin-bottom: 14px; color: var(--tint); font-weight: 600"><ArrowLeft :size="14" /> Servers</RouterLink>
    <div class="page-header">
      <div style="min-width: 0">
        <h1 class="page-title ellipsis">{{ server.name }}</h1>
        <div class="row" style="flex-wrap: wrap; margin-top: 6px">
          <span class="badge" :data-state="state.key"><span class="dot" />{{ state.label }}</span>
          <span class="chip">{{ KIND[server.kind] }}</span>
          <span class="chip num">{{ server.game_version }}</span>
          <span class="chip">On this computer</span>
        </div>
      </div>
      <div class="row">
        <button class="btn btn-lg" title="Open the server's folder" @click="openFolder"><FolderOpen /></button>
        <button v-if="server.instance && server.state === 'running'" class="btn btn-lg" @click="join"><Play /> Join</button>
        <button v-if="server.state === 'stopped'" class="btn btn-play btn-lg" :disabled="busy" @click="start"><Loader2 v-if="busy" class="spin" /><Play v-else /> Start</button>
        <button v-else class="btn btn-stop btn-lg" :disabled="busy || server.state === 'stopping'" @click="stop"><Loader2 v-if="busy || server.state === 'stopping'" class="spin" /><Square v-else /> Stop</button>
      </div>
    </div>

    <div class="easy-grid">
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
        <p v-if="!server.players.length" class="hint" style="margin: 0">{{ server.state === "running" ? "Nobody's on right now." : "Start the server to play." }}</p>
        <span v-for="p in server.players" :key="p" class="row" style="gap: 8px; font-weight: 600"><img :src="`https://mc-heads.net/avatar/${p}/24`" width="24" height="24" alt="" /> {{ p }}</span>
      </div>
    </div>

    <div class="tabs" style="margin: 20px 0 12px">
      <button :class="{ active: tab === 'console' }" @click="tab = 'console'">Console</button>
      <button :class="{ active: tab === 'settings' }" @click="tab = 'settings'">Settings</button>
    </div>

    <template v-if="tab === 'console'">
      <div ref="consoleEl" class="console">
        <div v-for="(l, i) in lines" :key="i" :class="lineClass(l)">{{ l }}</div>
        <span v-if="!lines.length" style="opacity: 0.6">{{ server.state === "stopped" ? "The server isn't running." : "Starting…" }}</span>
      </div>
      <form class="row" style="margin-top: 10px" @submit.prevent="send">
        <input v-model="command" class="input mono" :disabled="server.state === 'stopped'" placeholder="Command, e.g. op YourName" style="flex: 1" />
        <button class="btn" :disabled="server.state === 'stopped' || !command.trim()"><Send /> Send</button>
      </form>
    </template>

    <form v-else class="card stack" style="gap: 16px" @submit.prevent="saveSettings">
      <div class="easy-form">
        <div class="field"><label for="s-name">Name</label><input id="s-name" v-model="form.name" class="input" maxlength="48" /></div>
        <div class="field"><label for="s-port">Port</label><input id="s-port" v-model.number="form.port" class="input num" type="number" min="1024" max="65535" /></div>
        <div class="field"><label for="s-max">Max players</label><input id="s-max" v-model.number="form.maxPlayers" class="input num" type="number" min="1" max="1000" /></div>
        <div class="field">
          <label for="s-diff">Difficulty</label>
          <select id="s-diff" v-model="form.difficulty" class="select">
            <option value="peaceful">Peaceful</option><option value="easy">Easy</option><option value="normal">Normal</option><option value="hard">Hard</option>
          </select>
        </div>
        <div class="field">
          <label for="s-mode">Game mode</label>
          <select id="s-mode" v-model="form.gamemode" class="select">
            <option value="survival">Survival</option><option value="creative">Creative</option><option value="adventure">Adventure</option><option value="spectator">Spectator</option>
          </select>
        </div>
        <div class="field">
          <label for="s-mem">Memory</label>
          <div class="row">
            <input id="s-mem" v-model.number="form.memGb" class="range" type="range" min="1" max="16" step="1" style="flex: 1" :style="{ '--range-fill': `${((form.memGb - 1) / 15) * 100}%` }" />
            <span class="mono num" style="width: 48px; text-align: right; font-weight: 600">{{ form.memGb }} GB</span>
          </div>
        </div>
      </div>
      <label class="row" style="gap: 8px"><input v-model="form.pvp" type="checkbox" /> Players can hurt each other (PvP)</label>
      <label class="row" style="gap: 8px"><input v-model="form.whitelist" type="checkbox" /> Only players on the whitelist can join (add them with <span class="mono">whitelist add Name</span>)</label>
      <div class="row">
        <button class="btn btn-primary" :disabled="saving"><Loader2 v-if="saving" class="spin" /> Save</button>
        <span class="hint">Changes apply the next time the server starts.</span>
        <span class="spacer" />
        <button type="button" class="btn btn-ghost" style="color: var(--fill-red)" @click="confirmDelete = true"><Trash2 /> Delete server</button>
      </div>
    </form>

    <AppDialog v-if="confirmDelete" :title="`Delete ${server.name}?`" :width="420" @close="confirmDelete = false">
      <p class="muted">The server and its world are deleted from this computer. This can't be undone.</p>
      <div class="dialog-footer">
        <button class="btn" @click="confirmDelete = false">Cancel</button>
        <button class="btn btn-danger" :disabled="server.state !== 'stopped'" :title="server.state !== 'stopped' ? 'Stop the server first' : ''" @click="remove">Delete</button>
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
.easy-form {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 14px;
}
</style>
