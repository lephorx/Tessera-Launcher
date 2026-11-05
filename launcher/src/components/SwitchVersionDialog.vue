<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { Download, Eye, EyeOff, FileText, Loader2, Package, Search, TriangleAlert, X } from "lucide-vue-next";
import { api, type ContentFile, type Instance, type ProjectType, type ProjectVersion } from "@/lib/api";
import { errorText, toast } from "@/lib/toast";

/** Another version of an installed project: the project's versions, their changelogs, and the switch. */
const props = defineProps<{ instance: Instance; file: ContentFile }>();
const emit = defineEmits<{ close: []; switched: [Instance] }>();

const versions = ref<ProjectVersion[]>([]);
const loading = ref(true);
const failed = ref("");
const query = ref("");
const showIncompatible = ref(false);
const picked = ref<ProjectVersion | null>(null);
const busy = ref(false);

const LOADERS: Record<string, string[]> = { fabric: ["fabric"], quilt: ["quilt", "fabric"], forge: ["forge"], neoforge: ["neoforge"] };
/** The version this instance has, by Modrinth's id (or its version number for files it doesn't know). */
const currentId = computed(
  () => props.instance.mods.find((m) => m.project_id === props.file.project_id && m.kind === props.file.kind && m.world === props.file.world)?.version_id ?? null,
);
const isCurrent = (v: ProjectVersion) => (currentId.value ? v.id === currentId.value : v.version_number === props.file.version);

/** Why it can't go into this instance, or null when it can. */
function problem(v: ProjectVersion): string | null {
  if (v.game_versions.length && !v.game_versions.includes(props.instance.game_version)) return `Not for Minecraft ${props.instance.game_version}`;
  const need = props.file.kind === "mod" ? LOADERS[props.instance.loader] : props.file.kind === "datapack" ? ["datapack"] : null;
  if (need && !v.loaders.some((l) => need.includes(l))) return `Not for ${props.instance.loader}`;
  return null;
}

const shown = computed(() => {
  const words = query.value.toLowerCase().split(/\s+/).filter(Boolean);
  return versions.value.filter((v) => {
    if (!showIncompatible.value && problem(v) && !isCurrent(v)) return false;
    const text = `${v.version_number} ${v.name} ${v.version_type}`.toLowerCase();
    return words.every((w) => text.includes(w));
  });
});
const incompatible = computed(() => versions.value.filter((v) => problem(v) && !isCurrent(v)).length);

onMounted(async () => {
  window.addEventListener("keydown", onKey);
  try {
    versions.value = await api.projects.versions(props.file.project_id!);
    picked.value = versions.value.find(isCurrent) ?? versions.value.find((v) => !problem(v)) ?? versions.value[0] ?? null;
  } catch (e) {
    failed.value = errorText(e);
  } finally {
    loading.value = false;
  }
});
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") emit("close");
}
onUnmounted(() => window.removeEventListener("keydown", onKey));

const date = (iso: string) => new Date(iso).toLocaleDateString(undefined, { year: "numeric", month: "long", day: "numeric" });
const cap = (s: string) => s.slice(0, 1).toUpperCase() + s.slice(1);
/** "Forge 1.20.1" or "Fabric, Quilt 1.20–1.21.1" */
function targets(v: ProjectVersion) {
  const loaders = v.loaders.filter((l) => l !== "minecraft").map(cap).join(", ");
  const games = v.game_versions.length > 2 ? `${v.game_versions[0]}–${v.game_versions[v.game_versions.length - 1]}` : v.game_versions.join(", ");
  return [loaders, games].filter(Boolean).join(" ");
}

async function switchTo() {
  const v = picked.value;
  if (!v || busy.value) return;
  busy.value = true;
  try {
    const kind = (props.file.kind === "horizon-mod" || props.file.kind === "native-mod" ? "mod" : props.file.kind) as ProjectType;
    // a file this copy keeps to itself stays its own in the new version
    const inst = await api.projects.install(props.instance.id, props.file.project_id!, kind, props.file.world, v.id, props.file.local ? true : null);
    toast.success(`${props.file.title} is on ${v.version_number} now`);
    emit("switched", inst);
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <Teleport to="body">
    <div class="backdrop" @mousedown.self="emit('close')">
      <div class="dialog sv-dialog" role="dialog" aria-label="Switch version">
        <div class="dialog-head">
          <div class="row" style="gap: 14px; min-width: 0">
            <img v-if="file.icon_url" :src="file.icon_url" alt="" width="44" height="44" style="border-radius: 10px" />
            <span v-else class="tile" style="--type: #a2a9b4"><Package /></span>
            <div style="min-width: 0">
              <h2>Switch version</h2>
              <div class="hint ellipsis">{{ file.title }}</div>
            </div>
          </div>
          <button class="dialog-close" aria-label="Close" title="Close" @click="emit('close')"><X /></button>
        </div>

        <div class="sv-body">
          <aside class="sv-list">
            <div class="input-icon">
              <Search />
              <input v-model="query" class="input" placeholder="Search version…" aria-label="Search version" />
            </div>
            <div class="sv-scroll">
              <div v-if="loading" class="empty"><Loader2 class="spin" /></div>
              <p v-else-if="failed" class="error-text" style="padding: 8px">{{ failed }}</p>
              <p v-else-if="!shown.length" class="hint" style="padding: 8px">
                {{ query ? "No version matches." : `No version for Minecraft ${instance.game_version} on ${cap(instance.loader)}.` }}
              </p>
              <button
                v-for="v in shown"
                :key="v.id"
                class="sv-item"
                :class="{ on: picked?.id === v.id, bad: !!problem(v) }"
                :title="problem(v) ?? v.name"
                @click="picked = v"
              >
                <span class="sv-type" :data-type="v.version_type">{{ v.version_type.slice(0, 1).toUpperCase() }}</span>
                <span class="ellipsis" style="flex: 1; text-align: left">{{ v.version_number }}</span>
                <span v-if="isCurrent(v)" class="sv-current">Current</span>
              </button>
            </div>
            <button v-if="incompatible" class="btn btn-ghost sv-toggle" @click="showIncompatible = !showIncompatible">
              <component :is="showIncompatible ? EyeOff : Eye" /> {{ showIncompatible ? "Hide incompatible" : `Show incompatible (${incompatible})` }}
            </button>
          </aside>

          <section class="sv-detail">
            <template v-if="picked">
              <div class="sv-detail-head">
                <div class="row" style="flex-wrap: wrap; gap: 10px">
                  <h3 class="sv-version">{{ picked.version_number }}</h3>
                  <span class="sv-badge" :data-type="picked.version_type">{{ cap(picked.version_type) }}</span>
                  <span class="spacer" />
                  <span class="muted">{{ date(picked.date_published) }}</span>
                </div>
                <div class="row muted" style="gap: 8px; margin-top: 6px; flex-wrap: wrap">
                  <FileText :size="18" /> Changelog
                  <template v-if="targets(picked)"><span class="sv-dot" />{{ targets(picked) }}</template>
                </div>
                <p v-if="problem(picked) && !isCurrent(picked)" class="error-text" style="margin: 8px 0 0">{{ problem(picked) }}: it can't go into this instance.</p>
              </div>
              <div class="sv-changelog">
                <template v-if="picked.changelog?.trim()">{{ picked.changelog.trim() }}</template>
                <i v-else class="muted">No changelog provided for this version.</i>
              </div>
            </template>
          </section>
        </div>

        <div class="sv-foot">
          <span class="sv-warn"><TriangleAlert :size="22" /> Updating can break your instance. Review version changelogs and back up first.</span>
          <span class="spacer" />
          <button class="btn btn-lg" @click="emit('close')"><X /> Cancel</button>
          <button class="btn btn-primary btn-lg" :disabled="!picked || isCurrent(picked) || !!problem(picked) || busy" @click="switchTo">
            <Loader2 v-if="busy" class="spin" /><Download v-else /> {{ picked ? `Switch to ${picked.version_number}` : "Switch" }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.sv-dialog {
  max-width: 980px;
  height: min(760px, calc(100vh - 48px));
}
.sv-body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 300px 1fr;
}
.sv-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  padding: 16px;
  border-right: 1px solid var(--border);
}
.sv-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.sv-item {
  display: flex;
  align-items: center;
  gap: 12px;
  min-height: 46px;
  padding: 6px 10px;
  border: 0;
  border-radius: 12px;
  background: none;
  color: var(--foreground);
  font: inherit;
  font-weight: 600;
  cursor: pointer;
}
.sv-item:hover {
  background: var(--fill);
}
.sv-item.on {
  background: var(--tint-bg);
}
.sv-item.bad {
  opacity: 0.55;
}
.sv-type {
  display: grid;
  place-items: center;
  flex-shrink: 0;
  width: 28px;
  height: 28px;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 800;
  color: var(--status-green);
  background: color-mix(in srgb, var(--status-green) 18%, transparent);
}
.sv-type[data-type="beta"],
.sv-badge[data-type="beta"] {
  color: var(--status-orange);
  background: color-mix(in srgb, var(--status-orange) 18%, transparent);
}
.sv-type[data-type="alpha"],
.sv-badge[data-type="alpha"] {
  color: var(--destructive);
  background: color-mix(in srgb, var(--destructive) 18%, transparent);
}
.sv-current {
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--fill);
  font-size: 12px;
  color: var(--muted-foreground);
}
.sv-toggle {
  align-self: center;
}
.sv-detail {
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.sv-detail-head {
  padding: 18px 24px;
  border-bottom: 1px solid var(--border);
}
.sv-version {
  font-size: 22px;
  font-weight: 700;
}
.sv-badge {
  padding: 2px 10px;
  border-radius: 999px;
  font-size: 13px;
  font-weight: 700;
  color: var(--status-green);
  background: color-mix(in srgb, var(--status-green) 16%, transparent);
}
.sv-dot {
  width: 4px;
  height: 4px;
  border-radius: 999px;
  background: currentColor;
  opacity: 0.5;
}
.sv-changelog {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 18px 24px;
  white-space: pre-wrap;
  line-height: 1.5;
}
.sv-foot {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 14px 20px;
  border-top: 1px solid var(--border);
}
.sv-warn {
  display: flex;
  align-items: center;
  gap: 10px;
  max-width: 460px;
  font-size: 13px;
  font-weight: 600;
  color: var(--status-orange);
}
.sv-warn svg {
  flex-shrink: 0;
}
</style>
