<script setup lang="ts">
import { onMounted, ref } from "vue";
import { Archive, FolderOpen, Loader2, RotateCcw, Trash2 } from "lucide-vue-next";
import AppDialog from "@/components/AppDialog.vue";
import { api, type EasyBackup, type EasyServer } from "@/lib/api";
import { relative } from "@/lib/format";
import { bytes } from "@/lib/settingsDialog";
import { errorText, toast } from "@/lib/toast";

/** Backups of the world, mods and settings: by hand, on a schedule, restore and delete. */
const props = defineProps<{ server: EasyServer }>();
const emit = defineEmits<{ changed: [] }>();

const list = ref<EasyBackup[] | null>(null);
const making = ref(false);
const busy = ref<string | null>(null);
const confirm = ref<{ kind: "restore" | "delete"; backup: EasyBackup } | null>(null);
const every = ref(props.server.backup_every_hours ?? 0);
const keep = ref(props.server.backup_keep);

async function load() {
  try {
    list.value = await api.easy.backups(props.server.id);
  } catch (e) {
    toast.error(errorText(e));
  }
}
onMounted(load);

async function backup() {
  making.value = true;
  try {
    await api.easy.backup(props.server.id);
    toast.success("Backed up");
    await load();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    making.value = false;
  }
}

async function saveSchedule() {
  try {
    await api.easy.update(props.server.id, { backup_every_hours: every.value, backup_keep: keep.value });
    toast.success(every.value ? "Automatic backups saved" : "Automatic backups are off");
    emit("changed");
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function confirmed() {
  const c = confirm.value;
  if (!c) return;
  confirm.value = null;
  busy.value = c.backup.name;
  try {
    if (c.kind === "restore") {
      await api.easy.restore(props.server.id, c.backup.name);
      toast.success("Restored");
      emit("changed");
    } else {
      await api.easy.deleteBackup(props.server.id, c.backup.name);
    }
    await load();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = null;
  }
}

const reveal = (b: EasyBackup) => api.easy.revealBackup(props.server.id, b.name).catch((e) => toast.error(errorText(e)));
</script>

<template>
  <div class="stack" style="gap: 16px">
    <div class="card stack">
      <div class="row">
        <div class="stack" style="gap: 2px; flex: 1">
          <h3>Backups</h3>
          <span class="hint">The world, mods and settings. A running server keeps going while it's backed up.</span>
        </div>
        <button class="btn btn-primary" :disabled="making" @click="backup"><Loader2 v-if="making" class="spin" /><Archive v-else /> Back up now</button>
      </div>
      <div class="row" style="flex-wrap: wrap; gap: 10px">
        <label class="row hint" style="gap: 8px">
          Automatically
          <select v-model.number="every" class="select" style="width: 160px">
            <option :value="0">Off</option>
            <option :value="1">Every hour</option>
            <option :value="6">Every 6 hours</option>
            <option :value="12">Every 12 hours</option>
            <option :value="24">Every day</option>
            <option :value="168">Every week</option>
          </select>
        </label>
        <label v-if="every" class="row hint" style="gap: 8px">
          keep the newest
          <input v-model.number="keep" class="input num" type="number" min="1" max="100" style="width: 72px" />
        </label>
        <button class="btn" @click="saveSchedule">Save</button>
      </div>
      <p class="hint" style="margin: 0">Automatic backups are made while the launcher is open. Ones you make yourself are never deleted automatically.</p>
    </div>

    <div class="card stack" style="gap: 0; padding: 6px">
      <div v-for="b in list ?? []" :key="b.name" class="row backup">
        <Archive :size="18" style="opacity: 0.7" />
        <div class="stack" style="gap: 2px; flex: 1; min-width: 0">
          <b>{{ new Date(b.created_at).toLocaleString() }}</b>
          <span class="hint">{{ relative(b.created_at) }} · {{ bytes(b.size) }} · {{ b.automatic ? "Automatic" : "Made by hand" }}</span>
        </div>
        <Loader2 v-if="busy === b.name" class="spin" />
        <button class="btn btn-sm" :disabled="!!busy || server.state !== 'stopped'" :title="server.state !== 'stopped' ? 'Stop the server first' : ''" @click="confirm = { kind: 'restore', backup: b }"><RotateCcw /> Restore</button>
        <button class="btn btn-ghost btn-icon btn-sm" title="Show in folder" @click="reveal(b)"><FolderOpen /></button>
        <button class="btn btn-ghost btn-icon btn-sm" title="Delete" style="color: var(--fill-red)" :disabled="!!busy" @click="confirm = { kind: 'delete', backup: b }"><Trash2 /></button>
      </div>
      <p v-if="list && !list.length" class="hint" style="padding: 12px; margin: 0">No backups yet.</p>
      <div v-if="!list" class="empty"><Loader2 class="spin" /></div>
    </div>

    <AppDialog v-if="confirm" :title="confirm.kind === 'restore' ? 'Restore this backup?' : 'Delete this backup?'" :width="440" @close="confirm = null">
      <p class="muted">
        <template v-if="confirm.kind === 'restore'">The server goes back to how it was on {{ new Date(confirm.backup.created_at).toLocaleString() }}. Everything since then is replaced, so back up first if you might want it.</template>
        <template v-else>The backup from {{ new Date(confirm.backup.created_at).toLocaleString() }} is deleted. This can't be undone.</template>
      </p>
      <div class="dialog-footer">
        <button class="btn" @click="confirm = null">Cancel</button>
        <button class="btn" :class="confirm.kind === 'restore' ? 'btn-primary' : 'btn-danger'" @click="confirmed">{{ confirm.kind === "restore" ? "Restore" : "Delete" }}</button>
      </div>
    </AppDialog>
  </div>
</template>

<style scoped>
h3 {
  font-size: 15px;
  font-weight: 700;
}
.backup {
  gap: 10px;
  padding: 10px;
  border-radius: 12px;
}
.backup + .backup {
  border-top: 1px solid var(--border);
}
</style>
