<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { RouterLink, useRouter } from "vue-router";
import { ArrowLeft, Blocks, Box, Hammer, Loader2, Package, Puzzle, Server } from "lucide-vue-next";
import { api, type EasyKind, type GameVersion, type Instance } from "@/lib/api";
import { LEGAL, openLegal } from "@/lib/legal";
import { errorText, toast } from "@/lib/toast";

/** Easy hosting: a server on this computer in a few clicks, run by the launcher (no Docker). */
const props = defineProps<{ instanceId?: string | null }>();
const router = useRouter();

type Source = EasyKind | "instance";
const sources: { id: Source; label: string; hint: string; icon: typeof Box }[] = [
  { id: "vanilla", label: "Vanilla", hint: "Plain Minecraft, nothing added", icon: Box },
  { id: "instance", label: "One of my instances", hint: "Same version and mods as an instance in your library", icon: Package },
  { id: "paper", label: "Paper", hint: "Faster, with plugins. Friends join with plain Minecraft", icon: Puzzle },
  { id: "fabric", label: "Fabric", hint: "For Fabric mods", icon: Blocks },
  { id: "forge", label: "Forge", hint: "For Forge mods", icon: Hammer },
  { id: "neoforge", label: "NeoForge", hint: "For NeoForge mods", icon: Hammer },
];

const source = ref<Source>(props.instanceId ? "instance" : "vanilla");
const name = ref("");
const version = ref("");
const versions = ref<GameVersion[]>([]);
const instances = ref<Instance[]>([]);
const instance = ref(props.instanceId ?? "");
const memGb = ref(4);
const totalMb = ref(16384);
const eula = ref(false);
const creating = ref(false);

const releases = computed(() => versions.value.filter((v) => v.kind === "release"));
const hostable = computed(() => instances.value.filter((i) => i.loader !== "quilt"));
const picked = computed(() => instances.value.find((i) => i.id === instance.value));
const maxGb = computed(() => Math.max(2, Math.floor((totalMb.value / 1024) * 0.6)));

onMounted(async () => {
  const [v, list, mem] = await Promise.all([api.versions.game(), api.instances.list(), api.system.memoryMb().catch(() => 16384)]);
  versions.value = v.versions;
  version.value = v.latest;
  instances.value = list;
  totalMb.value = mem;
  if (!instance.value) instance.value = hostable.value[0]?.id ?? "";
  if (picked.value) memGb.value = picked.value.mods.length > 50 ? 6 : picked.value.loader === "vanilla" ? 2 : 4;
  memGb.value = Math.min(memGb.value, maxGb.value);
});

const placeholder = computed(() => (source.value === "instance" ? picked.value?.name : null) ?? "My server");

async function create() {
  creating.value = true;
  try {
    const fromInstance = source.value === "instance";
    const s = await api.easy.create({
      name: name.value.trim() || placeholder.value,
      kind: fromInstance ? "vanilla" : (source.value as EasyKind),
      game_version: fromInstance ? "" : version.value,
      memory_mb: memGb.value * 1024,
      instance: fromInstance ? instance.value : null,
      eula: eula.value,
    });
    // straight on: it's what everyone does next
    await api.easy.start(s.id).catch((e) => toast.error(errorText(e)));
    toast.success(`${s.name} is starting`);
    void router.replace(`/servers/local/${s.id}`);
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    creating.value = false;
  }
}
</script>

<template>
  <div class="page page-narrow enter">
    <RouterLink to="/servers/host" class="row hint" style="margin-bottom: 14px; color: var(--tint); font-weight: 600"><ArrowLeft :size="14" /> Host a server</RouterLink>
    <div class="page-header">
      <div>
        <h1 class="page-title">New server on this computer</h1>
        <p class="page-sub">The launcher sets everything up and runs it. No Docker, nothing else to install.</p>
      </div>
    </div>

    <form class="stack" style="gap: 20px" @submit.prevent="create">
      <div class="field">
        <span class="label">What kind</span>
        <div class="easy-kinds">
          <button v-for="s in sources" :key="s.id" type="button" class="card option easy-kind" :class="{ selected: source === s.id }" @click="source = s.id">
            <span class="icon"><component :is="s.icon" /></span>
            <span><h3>{{ s.label }}</h3><span class="hint">{{ s.hint }}</span></span>
          </button>
        </div>
      </div>

      <div v-if="source === 'instance'" class="field">
        <label for="e-inst">Instance</label>
        <select v-if="hostable.length" id="e-inst" v-model="instance" class="input">
          <option v-for="i in hostable" :key="i.id" :value="i.id">{{ i.name }} · {{ i.loader === "vanilla" ? "Vanilla" : i.loader }} {{ i.game_version }}</option>
        </select>
        <p v-else class="hint">No instances to host yet. Make one in the Library first.</p>
        <span class="hint">Mods that only run in the game (like minimaps and shaders) stay out; the server gets the rest.</span>
      </div>
      <div v-else class="field">
        <label for="e-ver">Minecraft version</label>
        <select id="e-ver" v-model="version" class="select">
          <option v-for="v in releases" :key="v.id" :value="v.id">{{ v.id }}</option>
        </select>
      </div>

      <div class="field">
        <label for="e-name">Name</label>
        <input id="e-name" v-model="name" class="input" maxlength="48" :placeholder="placeholder" />
      </div>

      <div class="field">
        <label for="e-mem">Memory</label>
        <div class="row">
          <input
            id="e-mem"
            v-model.number="memGb"
            class="range"
            type="range"
            min="1"
            :max="maxGb"
            step="1"
            style="flex: 1"
            :style="{ '--range-fill': `${((memGb - 1) / Math.max(1, maxGb - 1)) * 100}%` }"
          />
          <span class="mono num" style="width: 56px; text-align: right; font-weight: 600">{{ memGb }} GB</span>
        </div>
        <span class="hint">2 GB is fine for vanilla with a few friends; modpacks want 4–6 GB.</span>
      </div>

      <label class="row" style="gap: 8px">
        <input v-model="eula" type="checkbox" required />
        <span>I accept the <a class="link" :href="LEGAL.eula" @click.prevent="openLegal('eula')">Minecraft EULA</a></span>
      </label>

      <div class="row">
        <button class="btn btn-primary btn-lg" :disabled="creating || !eula || (source === 'instance' && !instance)">
          <Loader2 v-if="creating" class="spin" /><Server v-else /> Create and start
        </button>
        <span class="hint">It keeps running when you close the launcher; stop it on its page.</span>
      </div>
    </form>
  </div>
</template>

<style scoped>
.easy-kinds {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
  gap: 10px;
}
.easy-kind h3 {
  font-size: 15px;
}
</style>
