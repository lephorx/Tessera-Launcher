<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { FolderOpen, Loader2, Trash2 } from "lucide-vue-next";
import AppDialog from "../AppDialog.vue";
import { api, type StorageUsage } from "@/lib/api";
import { bytes, saveSettings } from "@/lib/settingsDialog";
import { errorText, toast } from "@/lib/toast";

const usage = ref<StorageUsage | null>(null);
const dataDir = ref("");
const downloads = ref<number | null>(null);
const clearing = ref(false);
const confirming = ref(false);

const total = computed(() => (usage.value ? usage.value.unique + usage.value.shared + usage.value.unused : 0));
const pct = (n: number) => (total.value ? `${(n / total.value) * 100}%` : "0%");

async function load() {
  usage.value = await api.system.storage().catch(() => null);
}

onMounted(async () => {
  void load();
  dataDir.value = (await api.system.info()).data_dir;
  downloads.value = (await api.settings.get()).max_downloads;
});

async function clearUnused() {
  confirming.value = false;
  clearing.value = true;
  try {
    const freed = await api.system.clearUnused();
    toast.success(freed ? `Freed ${bytes(freed)}` : "Nothing to clear");
    await load();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    clearing.value = false;
  }
}

async function saveDownloads() {
  if (downloads.value === null) return;
  try {
    await saveSettings({ max_downloads: downloads.value });
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function openDir() {
  try {
    await api.system.openDataDir();
  } catch (e) {
    toast.error(errorText(e));
  }
}
</script>

<template>
  <div class="set-block">
    <div class="row">
      <h3 class="set-title">Content storage</h3>
      <span class="spacer" />
      <strong v-if="usage" class="num">{{ bytes(total) }} stored</strong>
    </div>
    <p class="set-hint">Minecraft versions, libraries and Java are shared by your instances, so each is only downloaded once.</p>
    <template v-if="usage">
      <div class="storage-bar" role="img" :aria-label="`${bytes(usage.unique)} in instances, ${bytes(usage.shared)} shared, ${bytes(usage.unused)} unused`">
        <span class="storage-unique" :style="{ width: pct(usage.unique) }" />
        <span class="storage-shared" :style="{ width: pct(usage.shared) }" />
        <span class="storage-unused" :style="{ width: pct(usage.unused) }" />
      </div>
      <div class="storage-legend num">
        <span><i class="storage-unique" /> Instances <strong>{{ bytes(usage.unique) }}</strong></span>
        <span><i class="storage-shared" /> Shared <strong>{{ bytes(usage.shared) }}</strong></span>
        <span><i class="storage-unused" /> Unused <strong>{{ bytes(usage.unused) }}</strong></span>
      </div>
      <button class="btn btn-danger" style="align-self: flex-start" :disabled="clearing || !usage.unused" @click="confirming = true">
        <Loader2 v-if="clearing" class="spin" /><Trash2 v-else /> Clear unused
      </button>
    </template>
    <p v-else class="hint row"><Loader2 class="spin" :size="14" /> Adding it up…</p>
  </div>

  <div class="set-block">
    <h3 class="set-title">App directory</h3>
    <div class="set-path">
      <span class="mono ellipsis">{{ dataDir }}</span>
      <button class="btn btn-ghost btn-icon btn-sm" title="Open folder" aria-label="Open folder" @click="openDir"><FolderOpen /></button>
    </div>
    <p class="set-hint">Where the launcher keeps your instances, game files and settings.</p>
  </div>

  <div v-if="downloads !== null" class="set-block">
    <h3 class="set-title">Maximum concurrent downloads</h3>
    <div class="row">
      <span class="hint">1</span>
      <input
        v-model.number="downloads"
        class="range"
        type="range"
        min="1"
        max="16"
        step="1"
        aria-label="Maximum concurrent downloads"
        style="flex: 1"
        :style="{ '--range-fill': `${((downloads - 1) / 15) * 100}%` }"
        @change="saveDownloads"
      />
      <span class="hint">16</span>
      <span class="mono num" style="width: 36px; text-align: right; font-weight: 600">{{ downloads }}</span>
    </div>
    <p class="set-hint">How many files the launcher downloads at once. Lower it if downloads fail on your connection.</p>
  </div>

  <AppDialog v-if="confirming" title="Clear unused files?" :width="420" @close="confirming = false">
    <p class="muted">This deletes {{ usage ? bytes(usage.unused) : "" }} of Minecraft versions, Java and game assets that none of your instances use. They're downloaded again if an instance needs them.</p>
    <div class="dialog-footer">
      <button class="btn" @click="confirming = false">Cancel</button>
      <button class="btn btn-danger" @click="clearUnused">Clear unused</button>
    </div>
  </AppDialog>
</template>
