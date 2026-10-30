<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, CircleCheck, CircleX, Download, ExternalLink, Loader2, Play, Rocket, Settings2, X } from "lucide-vue-next";
import AddMachineForm from "./AddMachineForm.vue";
import { api, type Machine } from "@/lib/api";
import { errorText, toast } from "@/lib/toast";

/**
 * Host on this computer: one click installs Tessera (the server manager) here, then its own
 * setup asks for your account and public address, then the launcher connects to it.
 */
type Status = Awaited<ReturnType<typeof api.hostLocal.status>>;
const status = ref<Status | null>(null);
const machines = ref<Machine[]>([]);
const phase = ref<"idle" | "installing" | "starting" | "failed">("idle");
const step = ref("");
const log = ref<string[]>([]);
const error = ref("");
let unlisten: UnlistenFn | undefined;
let timer: ReturnType<typeof setInterval> | undefined;

const url = computed(() => (status.value?.installed ? `http://localhost:${status.value.installed.port}` : ""));
const connected = computed(() => machines.value.find((m) => m.url === url.value));
/** Where this computer is in the whole flow. */
const stage = computed(() => {
  const s = status.value;
  if (!s) return "loading";
  if (!s.supported) return "unsupported";
  if (phase.value !== "idle") return phase.value;
  if (!s.installed) return "install";
  if (!s.reachable) return "stopped";
  if (s.setup_required) return "setup";
  if (!connected.value) return "connect";
  return "done";
});

async function refresh() {
  status.value = await api.hostLocal.status();
  machines.value = await api.machines.list();
}

onMounted(async () => {
  unlisten = await listen<{ step: string | null; line: string | null }>("host-install", ({ payload }) => {
    if (payload.step) step.value = payload.step;
    if (payload.line) log.value = [...log.value.slice(-7), payload.line];
  });
  await refresh();
  // follows Tessera's setup in its own window
  timer = setInterval(() => {
    if (phase.value === "idle" && (stage.value === "setup" || stage.value === "stopped" || stage.value === "install")) void refresh();
  }, 3000);
});
onUnmounted(() => {
  unlisten?.();
  clearInterval(timer);
});

async function install() {
  phase.value = "installing";
  error.value = "";
  log.value = [];
  step.value = "docker";
  try {
    await api.hostLocal.install();
    phase.value = "idle";
    await refresh();
    toast.success("Tessera is installed. Now set it up.");
    void api.hostLocal.openSetup();
  } catch (e) {
    error.value = errorText(e);
    phase.value = "failed";
  }
}

async function start() {
  phase.value = "starting";
  error.value = "";
  log.value = [];
  try {
    await api.hostLocal.start();
    phase.value = "idle";
    await refresh();
  } catch (e) {
    error.value = errorText(e);
    phase.value = "failed";
  }
}

async function startDocker() {
  try {
    await api.hostLocal.startDocker();
    await refresh();
  } catch (e) {
    toast.error(errorText(e));
  }
}

const STEPS = [
  { id: "docker", label: "Check Docker", hint: "Starts Docker Desktop if it isn't running" },
  { id: "download", label: "Download Tessera", hint: "From github.com/lephorx/Tessera" },
  { id: "start", label: "Build and start", hint: "The first time takes a few minutes" },
  { id: "ready", label: "Wait until it's ready", hint: "" },
];
const order = STEPS.map((s) => s.id);
const stepState = (id: string) => {
  const i = order.indexOf(id);
  const at = order.indexOf(step.value);
  if (i < at) return "done";
  if (i > at) return "todo";
  return phase.value === "failed" ? "failed" : "active";
};
</script>

<template>
  <div class="card stack" style="gap: 16px">
    <h2 style="font-size: 17px; font-weight: 700">Host on this computer</h2>

    <p v-if="stage === 'loading'" class="hint row"><Loader2 class="spin" :size="14" /> Checking this computer…</p>

    <p v-else-if="stage === 'unsupported'" class="muted">
      Hosting on this computer works on macOS and Linux for now. On Windows, install Tessera in WSL 2 and add it as
      <b>another machine</b>.
    </p>

    <!-- 1. one click -->
    <template v-else-if="stage === 'install'">
      <p class="muted">
        Installs <b>Tessera</b>, the server manager, on this computer. Your Minecraft servers run here while the computer is on;
        friends join through a public address you set up next.
      </p>
      <div class="inset stack" style="gap: 8px">
        <div class="row">
          <CircleCheck v-if="status!.docker_running" :size="16" style="color: var(--status-green)" />
          <CircleX v-else :size="16" style="color: var(--status-red)" />
          <span v-if="status!.docker_running" style="flex: 1">Docker {{ status!.docker_version }} is running</span>
          <span v-else-if="status!.docker_installed" style="flex: 1">Docker is installed but not running</span>
          <span v-else style="flex: 1">Docker Desktop isn't installed</span>
          <button v-if="status!.docker_installed && !status!.docker_running" class="btn btn-sm" @click="startDocker"><Play /> Start Docker</button>
          <button v-if="!status!.docker_installed" class="btn btn-sm" @click="openUrl('https://www.docker.com/products/docker-desktop/')">
            <Download /> Get Docker Desktop <ExternalLink :size="12" />
          </button>
        </div>
        <div class="row">
          <CircleCheck v-if="status!.git" :size="16" style="color: var(--status-green)" />
          <CircleX v-else :size="16" style="color: var(--status-red)" />
          <span v-if="status!.git">git is installed</span>
          <span v-else>git is missing: in Terminal, run <span class="mono">xcode-select --install</span> once</span>
        </div>
      </div>
      <button class="btn btn-cta" style="align-self: flex-start" :disabled="!status!.docker_installed || !status!.git" @click="install">
        <Rocket /> Install Tessera
      </button>
      <p v-if="!status!.docker_installed" class="hint">Install Docker Desktop and open it once; this page notices when it's ready.</p>
    </template>

    <!-- installing / starting -->
    <template v-else-if="stage === 'installing' || stage === 'starting' || stage === 'failed'">
      <ol v-if="stage !== 'starting'" class="hl-steps">
        <li v-for="s in STEPS" :key="s.id" class="hl-step" :class="stepState(s.id)">
          <span class="hl-dot">
            <Check v-if="stepState(s.id) === 'done'" :size="13" :stroke-width="3" />
            <Loader2 v-else-if="stepState(s.id) === 'active'" :size="13" class="spin" />
            <X v-else-if="stepState(s.id) === 'failed'" :size="13" :stroke-width="3" />
          </span>
          <span class="stack" style="gap: 2px">
            <span style="font-weight: 600">{{ s.label }}</span>
            <span v-if="s.hint && stepState(s.id) !== 'done'" class="hint">{{ s.hint }}</span>
          </span>
        </li>
      </ol>
      <p v-else class="hint row"><Loader2 class="spin" :size="14" /> Starting Tessera…</p>
      <pre v-if="log.length" class="hl-log">{{ log.join("\n") }}</pre>
      <template v-if="stage === 'failed'">
        <p class="error-text" style="white-space: pre-wrap">{{ error }}</p>
        <div class="row">
          <button v-if="status?.installed" class="btn btn-primary" @click="start"><Play /> Try starting again</button>
          <button v-else class="btn btn-primary" @click="(phase = 'idle'), refresh()">Try again</button>
        </div>
      </template>
    </template>

    <!-- installed but not running -->
    <template v-else-if="stage === 'stopped'">
      <p class="muted">Tessera is installed in <span class="mono">{{ status!.installed!.dir }}</span> but isn't running.</p>
      <button class="btn btn-primary" style="align-self: flex-start" @click="start"><Play /> Start Tessera</button>
    </template>

    <!-- 2. Tessera's own setup -->
    <template v-else-if="stage === 'setup'">
      <div class="hl-done"><CircleCheck :size="18" /> Tessera is installed and running.</div>
      <p class="muted">
        Now set it up in Tessera: create your admin account (with an authenticator app for the one-time codes), then choose how
        friends reach your servers (Playit, optionally your own domain). This page moves on by itself when you're done.
      </p>
      <button class="btn btn-cta" style="align-self: flex-start" @click="api.hostLocal.openSetup()"><Settings2 /> Open Tessera setup</button>
      <p class="hint row"><Loader2 class="spin" :size="13" /> Waiting for the setup…</p>
    </template>

    <!-- 3. connect the launcher -->
    <template v-else-if="stage === 'connect'">
      <div class="hl-done"><CircleCheck :size="18" /> Tessera is set up.</div>
      <p class="muted">Last step: sign in with the Tessera account you just made, so the launcher can manage your servers.</p>
      <AddMachineForm :url="url" name="This computer" hide-url />
    </template>

    <template v-else-if="stage === 'done'">
      <div class="hl-done"><CircleCheck :size="18" /> Tessera runs on this computer and is connected.</div>
      <div class="row">
        <RouterLink :to="`/servers/${connected!.id}`" class="btn btn-primary"><Rocket /> Open your servers</RouterLink>
        <button class="btn" @click="api.hostLocal.openSetup()"><Settings2 /> Tessera dashboard</button>
      </div>
    </template>
  </div>
</template>

<style scoped>
.hl-steps {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.hl-step {
  display: flex;
  gap: 12px;
  align-items: flex-start;
}
.hl-step.todo {
  color: var(--muted-foreground);
}
.hl-dot {
  display: grid;
  place-items: center;
  flex-shrink: 0;
  width: 22px;
  height: 22px;
  border-radius: 999px;
  border: 2px solid var(--border);
  color: var(--tint);
}
.hl-step.done .hl-dot {
  background: var(--tint);
  border-color: var(--tint);
  color: #fff;
}
.hl-step.active .hl-dot {
  border-color: var(--tint);
}
.hl-step.failed .hl-dot {
  background: var(--fill-red);
  border-color: var(--fill-red);
  color: #fff;
}
.hl-log {
  margin: 0;
  padding: 10px 12px;
  max-height: 150px;
  overflow: hidden;
  font: 11px/1.5 var(--font-mono, ui-monospace, monospace);
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--muted-foreground);
  background: var(--muted);
  border-radius: 10px;
}
.hl-done {
  display: flex;
  align-items: center;
  gap: 8px;
  font-weight: 600;
  color: var(--fill-green);
}
</style>
