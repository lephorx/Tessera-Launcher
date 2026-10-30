<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { save } from "@tauri-apps/plugin-dialog";
import { File, Folder, Loader2, PackageCheck } from "lucide-vue-next";
import { api, type Instance, type PackEntry } from "@/lib/api";
import { errorText, toast } from "@/lib/toast";
import AppDialog from "./AppDialog.vue";

/** Export an instance as a Tessera pack (.tapack) or a Modrinth pack (.mrpack). */
const props = defineProps<{ instance: Instance }>();
const emit = defineEmits<{ close: [] }>();

const name = ref(props.instance.pack?.name ?? props.instance.name);
const version = ref(props.instance.pack?.version ?? "1.0.0");
const author = ref("");
const summary = ref("");
const entries = ref<PackEntry[] | null>(null);
const include = ref(new Set<string>());
const busy = ref(false);
const format = ref<"tapack" | "mrpack">("tapack");
const error = ref("");

const size = (n: number) => (n > 1e9 ? `${(n / 1e9).toFixed(1)} GB` : n > 1e6 ? `${(n / 1e6).toFixed(1)} MB` : n > 1e3 ? `${Math.round(n / 1e3)} KB` : `${n} B`);
const selectedSize = computed(() => (entries.value ?? []).filter((e) => include.value.has(e.name)).reduce((a, e) => a + e.size, 0));

onMounted(async () => {
  try {
    entries.value = await api.projects.entries(props.instance.id);
    include.value = new Set(entries.value.filter((e) => e.suggested).map((e) => e.name));
  } catch (e) {
    error.value = errorText(e);
  }
});

function toggle(n: string) {
  const s = new Set(include.value);
  if (s.has(n)) s.delete(n);
  else s.add(n);
  include.value = s;
}

async function exportPack() {
  const slug = name.value.trim().replace(/[^\w.-]+/g, "-") || "pack";
  const ext = format.value;
  const path = await save({
    defaultPath: `${slug}-${version.value.trim() || "1.0.0"}.${ext}`,
    filters: [{ name: ext === "mrpack" ? "Modrinth pack" : "Tessera pack", extensions: [ext] }],
  });
  if (!path) return;
  busy.value = true;
  error.value = "";
  try {
    const r = await api.projects.export(props.instance.id, path, {
      name: name.value,
      version: version.value,
      author: author.value,
      summary: summary.value,
      include: [...include.value],
      format: format.value,
    });
    toast.success(`Exported ${size(r.bytes)}: ${r.linked} from Modrinth, ${r.embedded} files included`);
    emit("close");
  } catch (e) {
    error.value = errorText(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <AppDialog title="Export pack" :width="560" @close="emit('close')">
    <div class="stack" style="gap: 14px">
      <div class="choices">
        <button type="button" class="choice" :class="{ on: format === 'tapack' }" @click="format = 'tapack'">Tessera (.tapack)</button>
        <button type="button" class="choice" :class="{ on: format === 'mrpack' }" @click="format = 'mrpack'">Modrinth (.mrpack)</button>
      </div>
      <p v-if="format === 'tapack'" class="hint">
        Opens in Tessera Launcher with its icon, memory setting and disabled mods. Content from Modrinth is linked, not
        copied, so the file stays small; everything else you tick is included.
      </p>
      <p v-else class="hint">
        Opens in the Modrinth App, Prism Launcher, Tessera and most other launchers. Mods from Modrinth are linked; everything
        else you tick goes in as files. The icon isn't part of the format.
      </p>
      <div class="row" style="gap: 12px">
        <div class="field" style="flex: 2"><label for="ex-name">Name</label><input id="ex-name" v-model="name" class="input" /></div>
        <div class="field" style="flex: 1"><label for="ex-version">Version</label><input id="ex-version" v-model="version" class="input mono" /></div>
      </div>
      <div class="field"><label for="ex-author">Author <span class="hint">(optional)</span></label><input id="ex-author" v-model="author" class="input" /></div>
      <div class="field"><label for="ex-summary">Description <span class="hint">(optional)</span></label><input id="ex-summary" v-model="summary" class="input" /></div>
      <div class="field">
        <span class="label">Include</span>
        <div class="inset stack" style="gap: 2px; max-height: 220px; overflow-y: auto">
          <p v-if="entries === null" class="hint row"><Loader2 class="spin" :size="13" /> Reading the instance…</p>
          <p v-else-if="!entries.length" class="hint">The instance is empty. Play once or add content first.</p>
          <label v-for="e in entries ?? []" :key="e.name" class="row" style="cursor: pointer; padding: 4px 2px">
            <input type="checkbox" :checked="include.has(e.name)" @change="toggle(e.name)" />
            <Folder v-if="e.is_dir" :size="15" class="muted" /><File v-else :size="15" class="muted" />
            <span class="mono" style="flex: 1; font-size: 13px">{{ e.name }}</span>
            <span class="hint num">{{ size(e.size) }}</span>
          </label>
        </div>
        <span class="hint num">Selected: {{ size(selectedSize) }} before linking. Worlds, screenshots and the server list are off by default; logs are never included.</span>
      </div>
      <p v-if="error" class="error-text">{{ error }}</p>
      <div class="dialog-footer" style="margin-top: 0">
        <button class="btn btn-lg" @click="emit('close')">Cancel</button>
        <button class="btn btn-primary btn-lg" :disabled="busy || !name.trim() || entries === null" @click="exportPack">
          <Loader2 v-if="busy" class="spin" /><PackageCheck v-else /> Export .{{ format }}
        </button>
      </div>
    </div>
  </AppDialog>
</template>
