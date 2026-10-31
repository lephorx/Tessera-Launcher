<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { RouterLink, useRouter } from "vue-router";
import { Check, ChevronRight, Laptop, Loader2, Server, X } from "lucide-vue-next";
import { api, type Instance, type Machine } from "@/lib/api";
import { LEGAL, openLegal } from "@/lib/legal";
import { packProgress } from "@/lib/store";
import { errorText, toast } from "@/lib/toast";
import AppDialog from "./AppDialog.vue";

/**
 * Creates a server from this instance on one of your CSCM machines, then follows it until
 * it runs: pack built here, uploaded, set up by CSCM (container, tunnel, DNS), and the
 * modpack installed and started by the server itself.
 */
const props = defineProps<{ instance: Instance }>();
const emit = defineEmits<{ close: [] }>();
const router = useRouter();

const machines = ref<Machine[] | null>(null);
const machineId = ref("");
const name = ref(props.instance.name);
const memGb = ref(4);
// Tessera only creates a server once you accept the Minecraft EULA for it
const eula = ref(false);
const error = ref("");
const usable = computed(() => (machines.value ?? []).filter((m) => m.signed_in));
const machine = computed(() => usable.value.find((m) => m.id === machineId.value));

onMounted(async () => {
  machines.value = await api.machines.list();
  machineId.value = usable.value[0]?.id ?? "";
  // modpacks need more memory than vanilla
  memGb.value = props.instance.mods.length > 50 ? 6 : props.instance.loader === "vanilla" ? 2 : 4;
});

// ── progress ──

type StepKey = "build" | "upload" | "setup" | "start";
const phase = ref<"form" | "working" | "done" | "failed">("form");
const step = ref<StepKey>("build");
const serverId = ref<number | null>(null);
const setup = ref<{ percent: number; step: string }>({ percent: 0, step: "" });
const runtime = ref("");
const logTail = ref<string[]>([]);
const summary = ref("");
const startedAt = ref(0);
const now = ref(Date.now());
let timer: ReturnType<typeof setInterval> | undefined;
let clock: ReturnType<typeof setInterval> | undefined;
let stoppedPolls = 0;

const steps = computed(() => [
  { key: "build" as const, label: "Building the server pack", hint: "Server-side mods and configs from this instance" },
  { key: "upload" as const, label: `Uploading to ${machine.value?.name ?? "the machine"}`, hint: "" },
  { key: "setup" as const, label: `Setting up on ${machine.value?.name ?? "the machine"}`, hint: "Container, ports and address" },
  { key: "start" as const, label: "Installing the modpack and starting", hint: "The server downloads Minecraft, the loader and the mods" },
]);
const order: StepKey[] = ["build", "upload", "setup", "start"];
const stateOf = (key: StepKey) => {
  if (phase.value === "done") return "done";
  const i = order.indexOf(key);
  const at = order.indexOf(step.value);
  if (i < at) return "done";
  if (i > at) return "todo";
  return phase.value === "failed" ? "failed" : "active";
};

/** 0–1 for the active step's bar, or null for "busy, no numbers". */
const fraction = computed<number | null>(() => {
  if (step.value === "build" || step.value === "upload") {
    return packProgress.total ? packProgress.done / packProgress.total : null;
  }
  if (step.value === "setup") return setup.value.percent / 100;
  return null;
});
const detail = computed(() => {
  if (step.value === "build") return packProgress.stage && packProgress.stage !== "Uploading" ? packProgress.stage : "";
  if (step.value === "upload") {
    const mb = (kb: number) => (kb / 1024).toFixed(1);
    return packProgress.total ? `${mb(packProgress.done)} of ${mb(packProgress.total)} MB` : "";
  }
  if (step.value === "setup") return setup.value.step;
  if (step.value === "start") {
    const s = Math.round((now.value - startedAt.value) / 1000);
    const time = s < 60 ? `${s}s` : `${Math.floor(s / 60)} min ${s % 60}s`;
    return `${runtime.value === "starting" ? "Starting" : runtime.value === "unhealthy" ? "Still starting" : "Waiting for the server"} · ${time}`;
  }
  return "";
});

// the bar shows "Uploading" as soon as the first chunk goes out
const uploading = computed(() => packProgress.stage === "Uploading");

async function create() {
  phase.value = "working";
  step.value = "build";
  error.value = "";
  const watch = setInterval(() => {
    if (step.value === "build" && uploading.value) step.value = "upload";
  }, 100);
  try {
    const r = await api.createServer(props.instance.id, machineId.value, name.value.trim() || props.instance.name, memGb.value, eula.value);
    serverId.value = r.server_id;
    summary.value = `${r.linked} mod${r.linked === 1 ? "" : "s"} download on the server${r.client_only ? `, ${r.client_only} client-only left out` : ""}.`;
    step.value = "setup";
    timer = setInterval(poll, 1500);
    void poll();
  } catch (e) {
    error.value = errorText(e);
    phase.value = "failed";
  } finally {
    clearInterval(watch);
  }
}

async function get<T>(path: string): Promise<T | null> {
  const res = await api.machines.request<T>(machineId.value, "GET", path);
  return res.status < 400 ? res.body : null;
}

let polling = false;
async function poll() {
  if (polling || serverId.value === null) return;
  polling = true;
  try {
    const id = serverId.value;
    if (step.value === "setup") {
      const p = await get<{ status: string; percent: number; step: string; message: string | null }>(`/servers/${id}/progress`);
      if (!p) return;
      if (p.status === "failed") return fail(p.message || p.step || "CSCM couldn't set up the server.");
      setup.value = { percent: p.percent, step: p.step };
      if (p.status === "completed" || p.status === "idle") {
        step.value = "start";
        startedAt.value = Date.now();
        clock = setInterval(() => (now.value = Date.now()), 1000);
      }
      return;
    }
    if (step.value === "start") {
      const [server, logs] = await Promise.all([
        get<{ server?: { runtime_status: string }; runtime_status?: string }>(`/servers/${id}`),
        get<{ data: string[] }>(`/servers/${id}/logs?tail=6`),
      ]);
      if (logs) logTail.value = logs.data.filter((l) => l.trim()).slice(-6);
      // CSCM answers { success, server: {...} }
      const status = server?.server?.runtime_status ?? server?.runtime_status;
      if (!status) return;
      runtime.value = status;
      if (status === "healthy" || status === "running") return finish();
      // stopped for a while after it had started: it crashed while installing or starting
      stoppedPolls = status === "stopped" ? stoppedPolls + 1 : 0;
      if (stoppedPolls >= 4) fail("The server stopped while starting. Open it to read the console.");
    }
  } catch {
    // the machine may be busy; keep polling
  } finally {
    polling = false;
  }
}

function stopPolling() {
  clearInterval(timer);
  clearInterval(clock);
}

function finish() {
  stopPolling();
  phase.value = "done";
  // adds it to the instance's server list (Worlds tab, Multiplayer) with its address
  void api.instances.linkedServers(props.instance.id).catch(() => {});
  toast.success(`${name.value} is running`);
}

function fail(message: string) {
  stopPolling();
  error.value = message;
  phase.value = "failed";
}

function openServer() {
  emit("close");
  router.push(`/servers/${machineId.value}`);
}

function close() {
  // setting up continues on the machine; this only stops watching
  if (phase.value === "working" && serverId.value !== null) toast.info(`${name.value} keeps setting up on ${machine.value?.name ?? "the machine"}.`);
  emit("close");
}

onUnmounted(stopPolling);
</script>

<template>
  <AppDialog :title="phase === 'form' ? 'Create a server from this instance' : name" :width="520" @close="close">
    <div v-if="phase === 'form'" class="stack" style="gap: 16px">
      <p class="hint">
        The server gets the same Minecraft version, loader and server-side mods. Client-only mods (shaders, minimaps,
        performance mods like Sodium) and your options stay out. Configs come along.
      </p>
      <RouterLink :to="`/servers/new?instance=${instance.id}`" class="inset row" style="gap: 10px; color: inherit" @click="emit('close')">
        <Laptop :size="18" />
        <span style="flex: 1"><b>Run it on this computer, the easy way</b><br /><span class="hint">No Docker or Tessera needed: the launcher runs it.</span></span>
        <ChevronRight :size="18" class="muted" />
      </RouterLink>
      <div v-if="machines && !usable.length" class="inset stack" style="gap: 8px">
        <span>No signed-in machine to create it on.</span>
        <RouterLink to="/servers/host" class="btn btn-primary" style="align-self: flex-start" @click="emit('close')"><Server /> Host a server</RouterLink>
      </div>
      <template v-else>
        <div class="field">
          <label for="cs-machine">Machine</label>
          <select id="cs-machine" v-model="machineId" class="select">
            <option v-for="m in usable" :key="m.id" :value="m.id">{{ m.name }} ({{ m.url }})</option>
          </select>
        </div>
        <div class="field">
          <label for="cs-name">Server name</label>
          <input id="cs-name" v-model="name" class="input" maxlength="64" />
        </div>
        <div class="field">
          <span class="label">Memory</span>
          <div class="row">
            <input v-model.number="memGb" class="range" type="range" min="2" max="16" step="1" :style="{ '--range-fill': `${((memGb - 2) / 14) * 100}%` }" />
            <span class="mono num" style="width: 56px; text-align: right; font-weight: 600">{{ memGb }} GB</span>
          </div>
        </div>
        <label class="row" style="gap: 8px; font-size: 13px; cursor: pointer">
          <input v-model="eula" type="checkbox" required />
          <span>I accept the <a class="link" :href="LEGAL.eula" @click.prevent="openLegal('eula')">Minecraft EULA</a></span>
        </label>
        <div class="dialog-footer" style="margin-top: 0">
          <button class="btn btn-lg" @click="emit('close')">Cancel</button>
          <button class="btn btn-primary btn-lg" :disabled="!machineId || !eula" @click="create"><Server /> Create server</button>
        </div>
      </template>
    </div>

    <div v-else class="stack" style="gap: 18px">
      <ol class="cs-steps">
        <li v-for="s in steps" :key="s.key" class="cs-step" :class="stateOf(s.key)">
          <span class="cs-dot">
            <Check v-if="stateOf(s.key) === 'done'" :size="14" :stroke-width="3" />
            <Loader2 v-else-if="stateOf(s.key) === 'active'" :size="14" class="spin" />
            <X v-else-if="stateOf(s.key) === 'failed'" :size="14" :stroke-width="3" />
          </span>
          <div class="cs-step-body">
            <span class="cs-step-label">{{ s.label }}</span>
            <template v-if="stateOf(s.key) === 'active'">
              <span class="hint">{{ detail || s.hint }}</span>
              <div class="progress cs-bar" :class="{ busy: fraction === null }">
                <div :style="{ width: fraction === null ? '30%' : `${Math.max(fraction * 100, 3)}%` }" />
              </div>
              <pre v-if="s.key === 'start' && logTail.length" class="cs-log">{{ logTail.join("\n") }}</pre>
            </template>
            <span v-else-if="stateOf(s.key) === 'todo'" class="hint">{{ s.hint }}</span>
          </div>
        </li>
      </ol>

      <div v-if="phase === 'done'" class="inset stack" style="gap: 4px">
        <strong>Your server is running</strong>
        <span class="hint">{{ summary }} It's in this instance's server list (Worlds tab and Multiplayer), and gets the instance's server-side mods whenever you change them.</span>
      </div>
      <p v-if="phase === 'failed'" class="error-text">{{ error }}</p>

      <div class="dialog-footer" style="margin-top: 0">
        <template v-if="phase === 'working'">
          <span class="hint" style="margin-right: auto">You can close this; the server keeps setting up.</span>
          <button class="btn btn-lg" @click="close">Hide</button>
        </template>
        <template v-else-if="phase === 'failed' && serverId === null">
          <button class="btn btn-lg" @click="emit('close')">Close</button>
          <button class="btn btn-primary btn-lg" @click="phase = 'form'">Try again</button>
        </template>
        <template v-else>
          <button class="btn btn-lg" @click="emit('close')">Close</button>
          <button class="btn btn-primary btn-lg" @click="openServer"><Server /> Open server</button>
        </template>
      </div>
    </div>
  </AppDialog>
</template>

<style scoped>
.cs-steps {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
}
.cs-step {
  position: relative;
  display: flex;
  gap: 14px;
  padding-bottom: 18px;
}
.cs-step:last-child {
  padding-bottom: 0;
}
/* the line joining the dots */
.cs-step:not(:last-child)::before {
  content: "";
  position: absolute;
  left: 11px;
  top: 26px;
  bottom: 2px;
  width: 2px;
  border-radius: 2px;
  background: var(--border);
}
.cs-step.done:not(:last-child)::before {
  background: var(--tint);
}
.cs-dot {
  display: grid;
  place-items: center;
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  border-radius: 999px;
  border: 2px solid var(--border);
  color: var(--muted-foreground);
  background: var(--popover);
}
.cs-step.done .cs-dot {
  border-color: var(--tint);
  background: var(--tint);
  color: #fff;
}
.cs-step.active .cs-dot {
  border-color: var(--tint);
  color: var(--tint);
}
.cs-step.failed .cs-dot {
  border-color: var(--fill-red);
  background: var(--fill-red);
  color: #fff;
}
.cs-step-body {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding-top: 2px;
}
.cs-step-label {
  font-weight: 600;
}
.cs-step.todo .cs-step-label {
  color: var(--muted-foreground);
}
.cs-bar {
  margin-top: 4px;
}
.cs-bar > div {
  transition: width 300ms ease;
}
.cs-bar.busy {
  overflow: hidden;
}
.cs-bar.busy > div {
  animation: cs-busy 1.2s ease-in-out infinite;
}
@keyframes cs-busy {
  from {
    transform: translateX(-100%);
  }
  to {
    transform: translateX(340%);
  }
}
.cs-log {
  margin: 6px 0 0;
  padding: 10px 12px;
  max-height: 120px;
  overflow: hidden;
  font: 11px/1.5 var(--font-mono, ui-monospace, monospace);
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--muted-foreground);
  background: var(--muted);
  border-radius: 10px;
}
</style>
