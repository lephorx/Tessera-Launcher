<script setup lang="ts">
import { onMounted, ref } from "vue";
import { File, Folder, Loader2, UserPlus } from "lucide-vue-next";
import { api, type SharePreview } from "@/lib/api";
import { bytes } from "@/lib/settingsDialog";
import { errorText } from "@/lib/toast";
import AppDialog from "./AppDialog.vue";

/** Before an instance is shared the first time: what goes up, and what stays on this computer. */
const props = defineProps<{ id: string; name: string; busy: boolean }>();
const emit = defineEmits<{ close: []; share: [] }>();

const preview = ref<SharePreview | null>(null);
const error = ref("");

onMounted(async () => {
  try {
    preview.value = await api.share.preview(props.id);
  } catch (e) {
    error.value = errorText(e);
  }
});
</script>

<template>
  <AppDialog :title="`Share ${name}?`" :width="480" @close="emit('close')">
    <div class="stack" style="gap: 14px">
      <p class="hint" style="margin: 0">
        This goes up to your Tessera account, for the friends you add. Mods, resource packs and shaders that are on Modrinth go as links, not as files.
      </p>
      <div class="inset stack list">
        <div v-if="!preview && !error" class="empty"><Loader2 class="spin" /></div>
        <p v-else-if="preview && !preview.shared.length" class="hint" style="text-align: center; margin: 12px 0">Nothing to share yet: no mods, configs or settings.</p>
        <div v-for="e in preview?.shared ?? []" :key="e.name" class="row entry">
          <component :is="e.is_dir ? Folder : File" :size="16" class="muted" />
          <span style="flex: 1; font-weight: 600" class="ellipsis">{{ e.name }}</span>
          <span class="hint num">{{ e.is_dir ? `${e.files} file${e.files === 1 ? "" : "s"} · ` : "" }}{{ bytes(e.size) }}</span>
        </div>
      </div>
      <p v-if="preview?.shared.some((e) => e.private)" class="hint" style="margin: 0">
        Files that look like they hold a login (accounts, tokens, sessions) are left out.
      </p>
      <p v-if="preview?.left_out.length" class="hint" style="margin: 0">
        Stays on this computer: {{ preview.left_out.join(", ") }}.
      </p>
      <p v-if="error" class="error-text" style="margin: 0">{{ error }}</p>
      <div class="dialog-footer" style="margin-top: 0">
        <button class="btn" @click="emit('close')">Cancel</button>
        <button class="btn btn-primary" :disabled="busy || !preview" @click="emit('share')"><Loader2 v-if="busy" class="spin" /><UserPlus v-else /> Share</button>
      </div>
    </div>
  </AppDialog>
</template>

<style scoped>
.list {
  gap: 2px;
  max-height: 280px;
  overflow-y: auto;
  min-height: 60px;
}
.entry {
  gap: 10px;
  padding: 5px 4px;
}
</style>
