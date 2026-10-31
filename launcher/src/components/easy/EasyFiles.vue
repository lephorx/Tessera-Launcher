<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { ChevronRight, ExternalLink, File, FileText, Folder, FolderPlus, Loader2, Pencil, Save, Trash2, Upload, X } from "lucide-vue-next";
import AppDialog from "@/components/AppDialog.vue";
import { api, type EasyFile, type EasyServer } from "@/lib/api";
import { relative } from "@/lib/format";
import { bytes } from "@/lib/settingsDialog";
import { errorText, toast } from "@/lib/toast";

/** The server's folder: browse, edit text files, add files, rename and delete. */
const props = defineProps<{ server: EasyServer }>();

const path = ref("");
const entries = ref<EasyFile[] | null>(null);
const selected = ref(new Set<string>());
const editing = ref<{ path: string; text: string; original: string } | null>(null);
const saving = ref(false);
const naming = ref<{ kind: "rename" | "folder"; from: string; name: string } | null>(null);
const confirmDelete = ref(false);

const crumbs = computed(() => {
  const parts = path.value ? path.value.split("/") : [];
  return parts.map((p, i) => ({ name: p, path: parts.slice(0, i + 1).join("/") }));
});
const join = (name: string) => (path.value ? `${path.value}/${name}` : name);
const TEXT = /\.(txt|properties|json|json5|toml|yml|yaml|cfg|conf|ini|log|md|mcmeta|snbt|js|sh|bat|csv|xml|lang|secret)$/i;

async function load(to = path.value) {
  try {
    entries.value = await api.easy.files(props.server.id, to);
    path.value = to;
    selected.value = new Set();
  } catch (e) {
    toast.error(errorText(e));
  }
}
onMounted(() => load(""));

async function openEntry(f: EasyFile) {
  if (f.dir) return load(join(f.name));
  if (TEXT.test(f.name) || f.size < 64 * 1024) {
    try {
      const text = await api.easy.readFile(props.server.id, join(f.name));
      editing.value = { path: join(f.name), text, original: text };
      return;
    } catch {
      // not text: the system opens it
    }
  }
  await api.easy.openFile(props.server.id, join(f.name)).catch((e) => toast.error(errorText(e)));
}

async function save() {
  if (!editing.value) return;
  saving.value = true;
  try {
    await api.easy.writeFile(props.server.id, editing.value.path, editing.value.text);
    editing.value.original = editing.value.text;
    toast.success(props.server.state === "stopped" ? "Saved" : "Saved. Most files are read when the server starts.");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    saving.value = false;
  }
}

function closeEditor() {
  if (editing.value && editing.value.text !== editing.value.original && !window.confirm("Close without saving your changes?")) return;
  editing.value = null;
}

function keys(e: KeyboardEvent) {
  if ((e.metaKey || e.ctrlKey) && e.key === "s") {
    e.preventDefault();
    void save();
  }
}

async function add() {
  const picked = await open({ multiple: true, title: "Add files to the server" });
  const list = Array.isArray(picked) ? picked : picked ? [picked] : [];
  if (!list.length) return;
  try {
    const n = await api.easy.addFiles(props.server.id, path.value, list);
    toast.success(`Added ${n} file${n === 1 ? "" : "s"}`);
    await load();
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function named() {
  const n = naming.value;
  if (!n || !n.name.trim()) return;
  try {
    if (n.kind === "rename") await api.easy.renameFile(props.server.id, n.from, n.name.trim());
    else await api.easy.mkdir(props.server.id, path.value, n.name.trim());
    naming.value = null;
    await load();
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function remove() {
  confirmDelete.value = false;
  try {
    await api.easy.deleteFiles(props.server.id, [...selected.value].map(join));
    await load();
  } catch (e) {
    toast.error(errorText(e));
  }
}

function openHere() {
  api.easy.openFile(props.server.id, path.value).catch((e) => toast.error(errorText(e)));
}

function toggle(name: string) {
  const s = new Set(selected.value);
  if (s.has(name)) s.delete(name);
  else s.add(name);
  selected.value = s;
}
</script>

<template>
  <div v-if="editing" class="card stack" style="gap: 10px" @keydown="keys">
    <div class="row">
      <FileText :size="18" />
      <b class="mono ellipsis" style="flex: 1">{{ editing.path }}</b>
      <span v-if="editing.text !== editing.original" class="hint">Not saved</span>
      <button class="btn btn-primary btn-sm" :disabled="saving || editing.text === editing.original" @click="save"><Loader2 v-if="saving" class="spin" /><Save v-else /> Save</button>
      <button class="btn btn-ghost btn-icon btn-sm" title="Close" @click="closeEditor"><X /></button>
    </div>
    <textarea v-model="editing.text" class="input mono editor" spellcheck="false" />
  </div>

  <div v-else class="card stack" style="gap: 10px">
    <div class="row" style="flex-wrap: wrap">
      <nav class="row crumbs">
        <button @click="load('')">{{ server.name }}</button>
        <template v-for="c in crumbs" :key="c.path"><ChevronRight :size="14" /><button @click="load(c.path)">{{ c.name }}</button></template>
      </nav>
      <span class="spacer" />
      <button v-if="selected.size" class="btn btn-sm" style="color: var(--fill-red)" @click="confirmDelete = true"><Trash2 /> Delete {{ selected.size }}</button>
      <button v-if="selected.size === 1" class="btn btn-sm" @click="naming = { kind: 'rename', from: join([...selected][0]!), name: [...selected][0]! }"><Pencil /> Rename</button>
      <button class="btn btn-sm" @click="naming = { kind: 'folder', from: '', name: '' }"><FolderPlus /> New folder</button>
      <button class="btn btn-sm" @click="add"><Upload /> Add files</button>
      <button class="btn btn-ghost btn-icon btn-sm" title="Open in your file manager" @click="openHere"><ExternalLink /></button>
    </div>
    <div v-if="entries" class="files">
      <div v-for="f in entries" :key="f.name" class="file row" :class="{ picked: selected.has(f.name) }">
        <input type="checkbox" :checked="selected.has(f.name)" :aria-label="`Select ${f.name}`" @change="toggle(f.name)" />
        <button class="file-name row" @click="openEntry(f)">
          <Folder v-if="f.dir" :size="16" style="color: var(--tint)" /><File v-else :size="16" style="opacity: 0.6" />
          <span class="ellipsis">{{ f.name }}</span>
        </button>
        <span class="hint num" style="width: 90px; text-align: right">{{ f.dir ? "" : bytes(f.size) }}</span>
        <span class="hint" style="width: 110px; text-align: right">{{ f.modified ? relative(f.modified) : "" }}</span>
      </div>
      <p v-if="!entries.length" class="hint" style="padding: 10px; margin: 0">This folder is empty.</p>
    </div>
    <div v-else class="empty"><Loader2 class="spin" /></div>
  </div>

  <AppDialog v-if="naming" :title="naming.kind === 'rename' ? 'Rename' : 'New folder'" :width="400" @close="naming = null">
    <form class="stack" @submit.prevent="named">
      <input v-model="naming.name" class="input" placeholder="Name" />
      <div class="dialog-footer">
        <button type="button" class="btn" @click="naming = null">Cancel</button>
        <button class="btn btn-primary" :disabled="!naming.name.trim()">{{ naming.kind === "rename" ? "Rename" : "Create" }}</button>
      </div>
    </form>
  </AppDialog>

  <AppDialog v-if="confirmDelete" :title="`Delete ${selected.size} item${selected.size === 1 ? '' : 's'}?`" :width="420" @close="confirmDelete = false">
    <p class="muted">{{ [...selected].join(", ") }} {{ selected.size === 1 ? "is" : "are" }} deleted from the server's folder. Folders are deleted with everything in them. This can't be undone.</p>
    <div class="dialog-footer">
      <button class="btn" @click="confirmDelete = false">Cancel</button>
      <button class="btn btn-danger" @click="remove">Delete</button>
    </div>
  </AppDialog>
</template>

<style scoped>
.crumbs {
  gap: 4px;
  flex-wrap: wrap;
  font-weight: 600;
}
.crumbs button {
  border: 0;
  background: none;
  color: var(--tint);
  font: inherit;
  cursor: pointer;
  padding: 2px 4px;
  border-radius: 6px;
}
.crumbs button:last-child {
  color: inherit;
}
.files {
  display: flex;
  flex-direction: column;
}
.file {
  gap: 10px;
  padding: 4px 8px;
  border-radius: 10px;
}
.file:hover,
.file.picked {
  background: var(--muted);
}
.file-name {
  flex: 1;
  min-width: 0;
  gap: 8px;
  border: 0;
  background: none;
  color: inherit;
  font: inherit;
  cursor: pointer;
  padding: 6px 0;
  text-align: left;
}
.editor {
  min-height: 460px;
  resize: vertical;
  font-size: 12px;
  line-height: 1.5;
  white-space: pre;
  tab-size: 4;
}
</style>
