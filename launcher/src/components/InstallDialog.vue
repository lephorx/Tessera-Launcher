<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { Download, Loader2 } from "lucide-vue-next";
import { api, type Instance, type ProjectHit, type ProjectVersion } from "@/lib/api";
import { LOADERS } from "@/lib/format";
import { errorText, toast } from "@/lib/toast";
import AppDialog from "./AppDialog.vue";
import InstanceIcon from "./InstanceIcon.vue";

/** Pick which instance (and for datapacks, which world) a project goes into; with `version`, that version of it. */
const props = defineProps<{
  hit: Pick<ProjectHit, "project_id" | "project_type" | "title" | "versions" | "categories">;
  version?: ProjectVersion | null;
  preselect?: string;
  /** Into `preselect` (a shared pack): only on this computer. */
  local?: boolean;
}>();
const emit = defineEmits<{ close: []; installed: [Instance] }>();

const instances = ref<Instance[]>([]);
const target = ref("");
const worlds = ref<string[]>([]);
const world = ref("");
const busy = ref(false);

const gameVersions = computed(() => props.version?.game_versions ?? props.hit.versions);
function fits(i: Instance) {
  if (gameVersions.value.length && !gameVersions.value.includes(i.game_version)) return false;
  // Horizon runs resource and data packs, not Java mods or Iris/OptiFine shaders
  if (i.loader === "horizon" && (props.hit.project_type === "mod" || props.hit.project_type === "shader")) return false;
  if (props.hit.project_type !== "mod") return true;
  if (i.loader === "vanilla") return false;
  const ok = i.loader === "quilt" ? ["quilt", "fabric"] : [i.loader];
  return (props.version?.loaders ?? props.hit.categories).some((c) => ok.includes(c));
}

const compatible = computed(() => instances.value.filter(fits));
const others = computed(() => instances.value.filter((i) => !fits(i)));

onMounted(async () => {
  instances.value = await api.instances.list();
  target.value = compatible.value.find((i) => i.id === props.preselect)?.id ?? compatible.value[0]?.id ?? "";
});

watch(target, async (id) => {
  worlds.value = props.hit.project_type === "datapack" && id ? await api.instances.worlds(id) : [];
  world.value = worlds.value[0] ?? "";
});

async function install() {
  busy.value = true;
  try {
    const dpWorld = props.hit.project_type === "datapack" ? world.value : null;
    const inst = await api.projects.install(target.value, props.hit.project_id, props.hit.project_type, dpWorld, props.version?.id ?? null, props.local && target.value === props.preselect ? true : null);
    toast.success(`${props.hit.title} added to ${inst.name}`);
    emit("installed", inst);
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <AppDialog :title="`Install ${hit.title}${version ? ` ${version.version_number}` : ''}`" :width="480" @close="emit('close')">
    <div class="stack">
      <p v-if="!compatible.length" class="muted">
        None of your instances can use this. It needs
        {{ hit.project_type === "mod" ? "a matching loader and " : "" }}one of these Minecraft versions: {{ gameVersions.slice(-4).join(", ") }}.
      </p>
      <label v-for="i in compatible" :key="i.id" class="option-row" :style="target === i.id ? 'outline: 2px solid var(--primary)' : ''">
        <input v-model="target" type="radio" :value="i.id" style="display: none" />
        <InstanceIcon :icon="i.icon" :id="i.id" :size="40" />
        <span style="flex: 1">
          <h3>{{ i.name }}</h3>
          <span class="hint">{{ LOADERS[i.loader].label }} {{ i.game_version }}</span>
        </span>
      </label>
      <p v-if="others.length" class="hint">{{ others.length }} other instance{{ others.length === 1 ? "" : "s" }} can't use it (different version or loader).</p>
      <p v-if="hit.project_type === 'shader'" class="hint">Shaders need a shader mod such as Iris (Fabric/Quilt) or Oculus (Forge) in the instance.</p>
      <div v-if="hit.project_type === 'datapack' && target" class="field">
        <label for="dp-world">World</label>
        <select v-if="worlds.length" id="dp-world" v-model="world" class="select">
          <option v-for="w in worlds" :key="w" :value="w">{{ w }}</option>
        </select>
        <p v-else class="error-text">This instance has no worlds yet. Play once and create a world first.</p>
      </div>
      <div class="dialog-footer">
        <button class="btn btn-lg" @click="emit('close')">Cancel</button>
        <button class="btn btn-primary btn-lg" :disabled="busy || !target || (hit.project_type === 'datapack' && !world)" @click="install">
          <Loader2 v-if="busy" class="spin" /><Download v-else /> Install
        </button>
      </div>
    </div>
  </AppDialog>
</template>
