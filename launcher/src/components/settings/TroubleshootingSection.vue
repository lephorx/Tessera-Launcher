<script setup lang="ts">
import { ref } from "vue";
import { save } from "@tauri-apps/plugin-dialog";
import { Download, FolderOpen, Loader2, Trash2 } from "lucide-vue-next";
import { api } from "@/lib/api";
import { bytes } from "@/lib/settingsDialog";
import { errorText, toast } from "@/lib/toast";

const exporting = ref(false);

async function clearCache() {
  try {
    const freed = await api.system.clearCache();
    toast.success(freed ? `Cleared ${bytes(freed)} of cached data` : "The cache is already empty");
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function exportDebug() {
  const path = await save({ defaultPath: `tessera-debug-${new Date().toISOString().slice(0, 10)}.zip`, filters: [{ name: "Zip", extensions: ["zip"] }] });
  if (!path) return;
  exporting.value = true;
  try {
    await api.system.exportDebug(path);
    toast.success("Debug info saved");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    exporting.value = false;
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
    <h3 class="set-title">App cache</h3>
    <button class="btn" style="align-self: flex-start" @click="clearCache"><Trash2 /> Clear cache</button>
    <p class="set-hint">Clears cached pictures and web data. They're downloaded again when needed, so pages may load a little slower at first.</p>
  </div>
  <div class="set-block">
    <h3 class="set-title">App files</h3>
    <button class="btn" style="align-self: flex-start" @click="openDir"><FolderOpen /> Open app folder</button>
    <p class="set-hint">Your instances, game files and settings live here.</p>
  </div>
  <div class="set-block">
    <h3 class="set-title">Debug info</h3>
    <button class="btn" style="align-self: flex-start" :disabled="exporting" @click="exportDebug"><Loader2 v-if="exporting" class="spin" /><Download v-else /> Export debug info</button>
    <p class="set-hint">Saves a ZIP with the launcher's version, your system, your instances and their latest game logs, to send along with a bug report. Sign-ins are never included.</p>
  </div>
</template>
