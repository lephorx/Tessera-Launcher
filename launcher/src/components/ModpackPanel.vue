<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { RouterLink } from "vue-router";
import { ArrowUpCircle, Boxes, Check, History, Layers, Loader2, RotateCcw } from "lucide-vue-next";
import AppDialog from "./AppDialog.vue";
import { api, type Instance, type ProjectVersion } from "@/lib/api";
import { relative } from "@/lib/format";
import { errorText, toast } from "@/lib/toast";

/** A modpack instance: update it to the pack's newest version, pick another, or reset it to how the pack came. */
const props = defineProps<{ instance: Instance; /** content added since the pack was installed */ added: number }>();
const emit = defineEmits<{ changed: [Instance] }>();

const pack = computed(() => props.instance.pack!);
const versions = ref<ProjectVersion[]>([]);
const loading = ref(true);
const busy = ref(false);
const picking = ref(false);
const confirm = ref<{ version: ProjectVersion | null; reset: boolean } | null>(null);

onMounted(async () => {
  try {
    versions.value = await api.projects.versions(pack.value.project_id!);
  } catch {
    /* offline: no update check */
  } finally {
    loading.value = false;
  }
});

const current = computed(() => versions.value.find((v) => v.id === pack.value.version_id) ?? null);
/** Releases (betas and alphas only when the pack has none); Modrinth lists newest first. */
const releases = computed(() => {
  const r = versions.value.filter((v) => v.version_type === "release");
  return r.length ? r : versions.value;
});
const at = computed(() => versions.value.findIndex((v) => v.id === pack.value.version_id));
const newer = (v: ProjectVersion) => at.value === -1 || versions.value.indexOf(v) < at.value;
/** Update: the newest release for the instance's Minecraft version. */
const update = computed(() => {
  const v = releases.value.find((v) => v.game_versions.includes(props.instance.game_version));
  return v && v.id !== pack.value.version_id && newer(v) ? v : null;
});
/** A newer release for another Minecraft version: offered, never applied by "Update". */
const newerGame = computed(() => {
  const v = releases.value[0];
  return v && v.id !== update.value?.id && v.id !== pack.value.version_id && newer(v) && newGame(v) ? v : null;
});
/** The Minecraft version a pack version moves to, when it isn't the instance's. */
function newGame(v: ProjectVersion | null) {
  const gv = v?.game_versions[v.game_versions.length - 1];
  return gv && !v!.game_versions.includes(props.instance.game_version) ? gv : null;
}

async function apply() {
  const c = confirm.value;
  if (!c) return;
  confirm.value = null;
  picking.value = false;
  busy.value = true;
  try {
    const inst = await api.projects.modpackApply(props.instance.id, c.reset ? null : (c.version?.id ?? null), c.reset);
    toast.success(c.reset ? `${inst.name} is back to how the modpack came` : `${inst.name} is on ${inst.pack?.version ?? "the new version"}`);
    emit("changed", inst);
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="inset stack modpack" style="gap: 10px">
    <div class="row" style="gap: 12px; flex-wrap: wrap">
      <span class="tile tile-sm" style="--type: #959eef"><Boxes /></span>
      <div style="flex: 1; min-width: 200px">
        <div style="font-weight: 700">{{ pack.name }}<span class="hint mono" style="font-weight: 400; margin-left: 8px">{{ pack.version ?? current?.version_number ?? "" }}</span></div>
        <div class="hint">
          <Loader2 v-if="loading" class="spin" :size="12" />
          <template v-else-if="update">Version {{ update.version_number }} is out ({{ relative(update.date_published) }})</template>
          <template v-else-if="versions.length"><Check :size="12" /> Up to date for Minecraft {{ instance.game_version }}</template>
          <template v-else>Couldn't check for updates</template>
          <template v-if="newerGame"> · {{ newerGame.version_number }} for Minecraft {{ newGame(newerGame) }} is in Versions</template>
        </div>
      </div>
      <button v-if="update" class="btn btn-primary btn-sm" :disabled="busy" @click="confirm = { version: update, reset: false }">
        <Loader2 v-if="busy" class="spin" /><ArrowUpCircle v-else /> Update
      </button>
      <button class="btn btn-sm" :disabled="busy || !versions.length" @click="picking = true"><History /> Versions</button>
      <button class="btn btn-sm" :disabled="busy || !pack.version_id" @click="confirm = { version: current, reset: true }"><RotateCcw /> Reset</button>
      <RouterLink :to="`/project/${pack.project_id}`" class="btn btn-sm btn-ghost"><Layers /> Modpack page</RouterLink>
    </div>
    <p class="hint" style="margin: 0">
      Add any mod you like: updates keep what you added<template v-if="added"> ({{ added }} so far)</template> and what you turned off.
    </p>

    <AppDialog v-if="picking" title="Modpack versions" :width="560" @close="picking = false">
      <div class="stack" style="gap: 8px; max-height: 60vh; overflow: auto">
        <div v-for="v in versions" :key="v.id" class="option-row" style="cursor: default">
          <span style="flex: 1; min-width: 0">
            <h3>{{ v.name || v.version_number }}</h3>
            <span class="hint">{{ v.version_type }} · Minecraft {{ v.game_versions.join(", ") }} · {{ relative(v.date_published) }}</span>
          </span>
          <span v-if="v.id === pack.version_id" class="chip"><Check :size="12" /> Installed</span>
          <button v-else class="btn btn-sm" @click="confirm = { version: v, reset: false }">Switch</button>
        </div>
      </div>
    </AppDialog>

    <AppDialog v-if="confirm" :title="confirm.reset ? 'Reset to the original modpack?' : `Switch to ${confirm.version?.version_number}?`" @close="confirm = null">
      <div class="stack">
        <p v-if="confirm.reset" class="muted">
          {{ pack.name }} {{ pack.version ?? "" }} is installed again as it came. Mods, resource packs and shaders you added, and changed
          settings of the pack's mods, are removed; turned-off mods are turned on again. Your worlds stay.
        </p>
        <template v-else>
          <p class="muted">
            The modpack's own mods and files are replaced by {{ confirm.version?.version_number }}'s.
            What you added<template v-if="added"> ({{ added }})</template> and what you turned off stays. Your worlds stay.
          </p>
          <p v-if="newGame(confirm.version)" class="error-text">
            This version is for Minecraft {{ newGame(confirm.version) }} (the instance is on {{ instance.game_version }}).
            <template v-if="added">Mods you added may not work with it.</template> Back up your worlds first if you care about them.
          </p>
        </template>
        <div class="dialog-footer">
          <button class="btn" @click="confirm = null">Cancel</button>
          <button class="btn" :class="confirm.reset ? 'btn-danger' : 'btn-primary'" @click="apply">{{ confirm.reset ? "Reset" : "Switch" }}</button>
        </div>
      </div>
    </AppDialog>
  </div>
</template>
