<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { RouterLink, useRouter } from "vue-router";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowLeft, Copy, ExternalLink, KeyRound, Loader2, PackagePlus, Play, RefreshCw, RotateCw, ShieldCheck, Square, Trash2 } from "lucide-vue-next";
import AppDialog from "@/components/AppDialog.vue";
import { api, type Machine } from "@/lib/api";
import { serverAddresses } from "@/lib/serverAddress";
import { errorText, toast } from "@/lib/toast";

/** The parts of CSCM's GET /api/servers this page uses. */
interface CscmServer {
  id: number;
  name: string;
  type: string;
  version: string;
  port: number;
  runtime_status: string;
  local_only: boolean;
  tunnels?: { address: string | null; external_port: number | null }[];
  dns_records?: { type: string; name: string }[];
}

const TYPE_COLORS: Record<string, string> = {
  paper: "#eeaaaa", purpur: "#c39dff", fabric: "#dbb69b", forge: "#959eef", vanilla: "#7dd87d", bedrock: "#6fd2c3",
};

const props = defineProps<{ id: string }>();
const router = useRouter();
const machine = ref<Machine | null>(null);
const servers = ref<CscmServer[] | null>(null);
const error = ref("");
const busy = ref<number | null>(null);
const password = ref("");
const otp = ref("");
const signingIn = ref(false);
const confirmRemove = ref(false);
const packing = ref<number | null>(null);

/** "Get local pack": an instance with the server's version and mods, with the server in its list. */
async function localPack(s: CscmServer) {
  packing.value = s.id;
  try {
    const { instance: inst, missing } = await api.projects.fromServer(props.id, s.id);
    toast.success(`${inst.name} is in your library, ready to join the server`);
    if (missing.length) toast.error(`Some mods need ${missing.join(", ")}, which isn't on Modrinth for Minecraft ${inst.game_version}. Add it by hand.`);
    void router.push(`/instance/${inst.id}`);
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    packing.value = null;
  }
}
let timer: ReturnType<typeof setInterval> | undefined;

const signedIn = computed(() => machine.value?.signed_in);

async function load() {
  machine.value = (await api.machines.list()).find((m) => m.id === props.id) ?? null;
  if (!machine.value) return router.replace("/servers");
  if (!machine.value.signed_in) return;
  try {
    const res = await api.machines.request<{ servers: CscmServer[] }>(props.id, "GET", "/servers");
    if (res.status === 401) {
      machine.value = { ...machine.value, signed_in: false };
      return;
    }
    if (res.status !== 200) throw new Error(`CSCM answered ${res.status}`);
    servers.value = res.body.servers;
    error.value = "";
  } catch (e) {
    error.value = errorText(e);
  }
}

onMounted(() => {
  void load();
  timer = setInterval(load, 5000);
});
onUnmounted(() => clearInterval(timer));

function state(s: CscmServer) {
  const v = s.runtime_status;
  if (v === "running" || v === "healthy") return { key: "running", label: "Running" };
  if (v === "starting" || v === "restarting") return { key: "preparing", label: "Starting" };
  if (v === "unhealthy") return { key: "error", label: "Unhealthy" };
  return { key: "idle", label: v === "not_created" ? "Not created" : "Stopped" };
}

const addresses = (s: CscmServer) => serverAddresses(s, machine.value?.url ?? "");

async function action(s: CscmServer, what: "start" | "stop" | "restart") {
  busy.value = s.id;
  try {
    const res = await api.machines.request<{ message?: string }>(props.id, "POST", `/servers/${s.id}/${what}`);
    if (res.status >= 400) throw new Error(res.body?.message ?? `CSCM answered ${res.status}`);
    toast.success(`${s.name}: ${what === "start" ? "starting" : what === "restart" ? "restarting" : "stopping"}`);
    await load();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = null;
  }
}

async function signIn() {
  signingIn.value = true;
  try {
    machine.value = await api.machines.signIn(props.id, password.value, otp.value.trim());
    password.value = otp.value = "";
    await load();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    signingIn.value = false;
  }
}

async function remove() {
  await api.machines.remove(props.id);
  router.replace("/servers");
}

async function copy(text: string) {
  await navigator.clipboard.writeText(text);
  toast.success("Address copied");
}
</script>

<template>
  <div v-if="machine" class="page enter">
    <RouterLink to="/servers" class="row hint" style="margin-bottom: 14px; color: var(--tint); font-weight: 600"><ArrowLeft :size="14" /> Servers</RouterLink>
    <div class="page-header">
      <div>
        <h1 class="page-title">{{ machine.name }}</h1>
        <p class="page-sub mono">{{ machine.url }} · {{ machine.username }}</p>
      </div>
      <div class="row">
        <button class="btn btn-lg" title="Refresh" aria-label="Refresh" @click="load"><RefreshCw /></button>
        <button class="btn btn-lg" @click="openUrl(machine.url)"><ExternalLink /> Dashboard</button>
        <button class="btn btn-lg btn-danger" title="Remove from launcher" aria-label="Remove from launcher" @click="confirmRemove = true"><Trash2 /></button>
      </div>
    </div>

    <form v-if="!signedIn" class="card stack" style="max-width: 440px" @submit.prevent="signIn">
      <h2 style="font-size: 17px; font-weight: 700">Sign in to {{ machine.name }}</h2>
      <p class="hint">Your CSCM login has expired. Sign in again as {{ machine.username }}.</p>
      <div class="input-icon"><KeyRound /><input v-model="password" class="input" type="password" placeholder="Password" aria-label="Password" required /></div>
      <div class="input-icon"><ShieldCheck /><input v-model="otp" class="input num" maxlength="6" inputmode="numeric" placeholder="One-time code" aria-label="One-time code" required /></div>
      <div class="row"><button class="btn btn-primary" :disabled="signingIn"><Loader2 v-if="signingIn" class="spin" /> Sign in</button></div>
    </form>

    <template v-else>
      <p v-if="error" class="card error-text" style="margin-bottom: 16px">{{ error }}</p>
      <div v-if="servers && servers.length" class="grid">
        <div v-for="s in servers" :key="s.id" class="card stack">
          <div class="row" style="gap: 12px">
            <span class="tile" :style="{ '--type': TYPE_COLORS[s.type] ?? '#a2a9b4' }" style="font-weight: 800; font-size: 18px">
              {{ s.name.slice(0, 1).toUpperCase() }}
            </span>
            <div style="min-width: 0; flex: 1">
              <div style="font-weight: 700; font-size: 16px">{{ s.name }}</div>
              <span class="badge" :data-state="state(s).key" style="margin-top: 4px"><span class="dot" />{{ state(s).label }}</span>
            </div>
          </div>
          <div class="row" style="flex-wrap: wrap">
            <span class="chip" :style="{ color: TYPE_COLORS[s.type] }">{{ s.type[0]?.toUpperCase() + s.type.slice(1) }}</span>
            <span class="chip num">{{ s.version === "LATEST" ? "Latest" : s.version }}</span>
            <span class="chip num">Port {{ s.port }}</span>
          </div>
          <div v-if="addresses(s).public" class="inset stack" style="gap: 2px">
            <div class="row">
              <span class="mono selectable" style="flex: 1; font-size: 13px; font-weight: 600; overflow: hidden; text-overflow: ellipsis">{{ addresses(s).public }}</span>
              <button class="btn btn-ghost btn-icon btn-sm" title="Copy address" aria-label="Copy public address" @click="copy(addresses(s).public!)"><Copy /></button>
            </div>
            <span v-if="addresses(s).playit" class="hint mono selectable" style="font-size: 12px">Playit: {{ addresses(s).playit }}</span>
            <span v-if="addresses(s).lan" class="hint mono selectable" style="font-size: 12px">On your network: {{ addresses(s).lan }}</span>
          </div>
          <div v-else-if="addresses(s).lan" class="inset row">
            <span class="mono selectable" style="flex: 1; font-size: 13px; font-weight: 600">{{ addresses(s).lan }}</span>
            <span class="hint" style="white-space: nowrap">{{ s.local_only ? "Local network only" : "No public address yet" }}</span>
            <button class="btn btn-ghost btn-icon btn-sm" title="Copy address" aria-label="Copy LAN address" @click="copy(addresses(s).lan!)"><Copy /></button>
          </div>
          <div v-else class="inset hint">{{ s.local_only ? "Only on the local network" : "No public address yet" }}</div>
          <div class="row">
            <button v-if="state(s).key === 'idle'" class="btn btn-play" :disabled="busy === s.id" @click="action(s, 'start')">
              <Loader2 v-if="busy === s.id" class="spin" /><Play v-else /> Start
            </button>
            <template v-else>
              <button class="btn btn-stop" :disabled="busy === s.id" @click="action(s, 'stop')">
                <Loader2 v-if="busy === s.id" class="spin" /><Square v-else /> Stop
              </button>
              <button class="btn btn-icon" :disabled="busy === s.id" title="Restart (installs new mods)" aria-label="Restart" @click="action(s, 'restart')"><RotateCw /></button>
            </template>
            <span class="spacer" />
            <button
              v-if="s.type !== 'bedrock'"
              class="btn"
              :disabled="packing !== null"
              title="Make an instance with this server's Minecraft version and mods"
              @click="localPack(s)"
            >
              <Loader2 v-if="packing === s.id" class="spin" /><PackagePlus v-else /> Get local pack
            </button>
          </div>
        </div>
      </div>
      <div v-else-if="servers" class="card empty">
        <h3>No servers on this machine</h3>
        <p>Create one in its dashboard. Creating servers from the launcher comes with modpack sync.</p>
      </div>
      <div v-else class="empty"><Loader2 class="spin" /></div>
    </template>

    <AppDialog v-if="confirmRemove" title="Remove from the launcher?" @close="confirmRemove = false">
      <p class="muted">{{ machine.name }} and its servers keep running. You can add it again any time.</p>
      <div class="dialog-footer">
        <button class="btn" @click="confirmRemove = false">Cancel</button>
        <button class="btn btn-danger" @click="remove">Remove</button>
      </div>
    </AppDialog>
  </div>
</template>
