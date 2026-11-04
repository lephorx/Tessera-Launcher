<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { Loader2, Server } from "lucide-vue-next";
import AppDialog from "./AppDialog.vue";
import { api, type GameVersion } from "@/lib/api";
import { LEGAL, openLegal } from "@/lib/legal";
import { errorText, toast } from "@/lib/toast";

/**
 * A new server on a Tessera machine, like its dashboard's form: the types that Tessera
 * offers (Horizon from 1.18), the Minecraft version, memory and the EULA. Tessera sets it
 * up in the background; the machine's page shows it coming up.
 */
const props = defineProps<{ machineId: string; machineName: string }>();
const emit = defineEmits<{ close: []; created: [id: number] }>();

const LABELS: Record<string, string> = {
  vanilla: "Vanilla",
  paper: "Paper",
  purpur: "Purpur",
  fabric: "Fabric",
  forge: "Forge",
  bedrock: "Bedrock",
  horizon: "Horizon (Rust)",
};
const HINTS: Record<string, string> = {
  vanilla: "Plain Minecraft",
  paper: "Faster, with plugins",
  purpur: "Paper with more settings",
  fabric: "For Fabric mods",
  forge: "For Forge mods",
  bedrock: "For phones, consoles and Windows' Bedrock Edition",
  horizon: "Minecraft 26.3 rewritten in Rust: fast, little memory. The first one downloads Minecraft's files and prepares Horizon's (a few minutes)",
};

const types = ref<string[]>([]);
const type = ref("vanilla");
const versions = ref<GameVersion[]>([]);
const version = ref("");
const name = ref("");
const memGb = ref(4);
const eula = ref(false);
const localOnly = ref(false);
const creating = ref(false);
const error = ref("");

const releases = computed(() => versions.value.filter((v) => v.kind === "release"));
const noJvm = computed(() => type.value === "bedrock" || type.value === "horizon");
const noVersion = computed(() => noJvm.value);

onMounted(async () => {
  try {
    const [t, v, setup] = await Promise.all([
      api.machines.request<{ server_types: string[] }>(props.machineId, "GET", "/server-types"),
      api.versions.game(),
      api.machines.request<{ public_available?: boolean }>(props.machineId, "GET", "/setup").catch(() => null),
    ]);
    types.value = (t.body?.server_types ?? Object.keys(LABELS)).filter((x) => x !== "modpack");
    versions.value = v.versions;
    version.value = v.latest;
    // no Playit set up there: servers stay on the local network
    localOnly.value = setup?.status === 200 && setup.body?.public_available === false;
  } catch (e) {
    error.value = errorText(e);
  }
});

function pickType(t: string) {
  type.value = t;
  if (t === "horizon") memGb.value = Math.min(memGb.value, 2);
}

async function create() {
  creating.value = true;
  error.value = "";
  try {
    const body: Record<string, unknown> = {
      eula: true,
      name: name.value.trim() || `${LABELS[type.value] ?? type.value} server`,
      type: type.value,
      mem_min: noJvm.value ? 1 : Math.min(2, memGb.value),
      mem_max: memGb.value,
      local_only: localOnly.value,
    };
    if (!noVersion.value) body.version = version.value;
    const res = await api.machines.request<{ server_id?: number; message?: string; error?: string }>(props.machineId, "POST", "/servers", body);
    if (res.status >= 400 || !res.body?.server_id) throw new Error(res.body?.error ?? res.body?.message ?? `Tessera answered ${res.status}`);
    toast.success(`${body.name} is being set up on ${props.machineName}`);
    emit("created", res.body.server_id);
  } catch (e) {
    error.value = errorText(e);
  } finally {
    creating.value = false;
  }
}
</script>

<template>
  <AppDialog :title="`New server on ${machineName}`" :width="560" @close="emit('close')">
    <form class="stack" style="gap: 16px" @submit.prevent="create">
      <div class="field">
        <span class="label">What kind</span>
        <div class="nms-types">
          <button v-for="t in types" :key="t" type="button" class="card option nms-type" :class="{ selected: type === t }" @click="pickType(t)">
            <span><h3>{{ LABELS[t] ?? t }}</h3><span class="hint">{{ HINTS[t] ?? "" }}</span></span>
          </button>
        </div>
        <span v-if="types.length && !types.includes('horizon')" class="hint">Update Tessera on {{ machineName }} to 1.18 or newer for Horizon servers.</span>
      </div>
      <div v-if="!noVersion" class="field">
        <label for="nms-ver">Minecraft version</label>
        <select id="nms-ver" v-model="version" class="select">
          <option v-for="v in releases" :key="v.id" :value="v.id">{{ v.id }}</option>
        </select>
      </div>
      <div class="field">
        <label for="nms-name">Name</label>
        <input id="nms-name" v-model="name" class="input" maxlength="64" :placeholder="`${LABELS[type] ?? type} server`" />
      </div>
      <div class="field">
        <span class="label">{{ noJvm ? "Memory limit" : "Memory" }}</span>
        <div class="row">
          <input v-model.number="memGb" class="range" type="range" min="1" max="16" step="1" :style="{ '--range-fill': `${((memGb - 1) / 15) * 100}%` }" />
          <span class="mono num" style="width: 56px; text-align: right; font-weight: 600">{{ memGb }} GB</span>
        </div>
      </div>
      <label class="row" style="gap: 8px; font-size: 13px; cursor: pointer">
        <input v-model="localOnly" type="checkbox" />
        <span>Only on the local network (no public Playit address)</span>
      </label>
      <label class="row" style="gap: 8px; font-size: 13px; cursor: pointer">
        <input v-model="eula" type="checkbox" required />
        <span>I accept the <a class="link" :href="LEGAL.eula" @click.prevent="openLegal('eula')">Minecraft EULA</a></span>
      </label>
      <p v-if="error" class="error-text">{{ error }}</p>
      <div class="dialog-footer" style="margin-top: 0">
        <button type="button" class="btn btn-lg" @click="emit('close')">Cancel</button>
        <button class="btn btn-primary btn-lg" :disabled="creating || !eula || !types.length">
          <Loader2 v-if="creating" class="spin" /><Server v-else /> Create server
        </button>
      </div>
    </form>
  </AppDialog>
</template>

<style scoped>
.nms-types {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 8px;
}
.nms-type {
  padding: 12px;
  text-align: left;
}
.nms-type h3 {
  font-size: 14px;
}
.nms-type .hint {
  font-size: 12px;
}
</style>
