<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { RouterLink, useRouter } from "vue-router";
import { ArrowLeft, FolderOpen, Globe, Package, PackageCheck, Pencil, Plus, Server, Settings2, SquareTerminal, Trash2, UserPlus } from "lucide-vue-next";
import AppDialog from "@/components/AppDialog.vue";
import CreateServerDialog from "@/components/CreateServerDialog.vue";
import ExportDialog from "@/components/ExportDialog.vue";
import InstanceOptions from "@/components/InstanceOptions.vue";
import SharePanel from "@/components/SharePanel.vue";
import WorldsPanel from "@/components/WorldsPanel.vue";
import IconEditor from "@/components/IconEditor.vue";
import InstanceIcon from "@/components/InstanceIcon.vue";
import PlayButton from "@/components/PlayButton.vue";
import StateBadge from "@/components/StateBadge.vue";
import { api, type ContentFile, type Icon, type Instance } from "@/lib/api";
import { iconVersion } from "@/lib/iconCache";
import { tabDirection, type PanelDirection } from "@/lib/tabSwitch";
import { LOADERS, relative } from "@/lib/format";
import { logListeners, runOf } from "@/lib/store";
import { errorText, toast } from "@/lib/toast";

const props = defineProps<{ id: string }>();
const router = useRouter();
const inst = ref<Instance | null>(null);
const TAB_ORDER = ["content", "worlds", "logs", "share", "options"] as const;
const tab = ref<(typeof TAB_ORDER)[number]>("content");
const panelDir = ref<PanelDirection>("none");
function goTab(to: (typeof TAB_ORDER)[number]) {
  panelDir.value = tabDirection(TAB_ORDER, tab.value, to);
  tab.value = to;
}
const content = ref<ContentFile[]>([]);
const kindFilter = ref<"all" | ContentFile["kind"]>("all");
const editingIcon = ref(false);
const exporting = ref(false);
const creatingServer = ref(false);
const KINDS = [
  { id: "all", label: "All" },
  { id: "mod", label: "Mods" },
  { id: "resourcepack", label: "Resource packs" },
  { id: "shader", label: "Shaders" },
  { id: "datapack", label: "Data packs" },
] as const;
const countOf = (kind: (typeof KINDS)[number]["id"]) => content.value.filter((c) => kind === "all" || c.kind === kind).length;
// "42 mods · 3 resource packs · 2 turned off" under the filters
const summary = computed(() => {
  const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
  const parts = [
    plural(countOf("mod"), "mod", "mods"),
    countOf("resourcepack") && plural(countOf("resourcepack"), "resource pack", "resource packs"),
    countOf("shader") && plural(countOf("shader"), "shader", "shaders"),
    countOf("datapack") && plural(countOf("datapack"), "data pack", "data packs"),
  ].filter(Boolean);
  const off = content.value.filter((c) => !c.enabled).length;
  if (off) parts.push(`${off} turned off`);
  return parts.join(" · ");
});
const shownContent = computed(() => content.value.filter((c) => kindFilter.value === "all" || c.kind === kindFilter.value));

/** After a share update, leaving a share, or an ownership change. */
async function reloadInstance() {
  inst.value = await api.instances.get(props.id);
  await loadContent();
}

async function loadContent() {
  content.value = await api.instances.content(props.id);
}

async function toggle(f: ContentFile) {
  try {
    await api.instances.toggleContent(props.id, f);
    await loadContent();
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function removeFile(f: ContentFile) {
  try {
    inst.value = await api.instances.removeContent(props.id, f);
    await loadContent();
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function saveIcon(icon: Icon, file: string | null) {
  try {
    inst.value = await api.instances.setIcon(props.id, file ? null : icon, file);
    iconVersion.value++;
    editingIcon.value = false;
  } catch (e) {
    toast.error(errorText(e));
  }
}
const logs = ref<string[]>([]);
const LOG_TAIL = 500;
const allLogs = ref(false);
/** A long game log is thousands of lines: draw the end unless asked for all of it. */
const shownLogs = computed(() => (allLogs.value ? logs.value : logs.value.slice(-LOG_TAIL)));
const consoleEl = ref<HTMLElement | null>(null);
const confirmDelete = ref(false);
const run = computed(() => runOf(props.id));

function onLog(instance: string, line: string) {
  if (instance !== props.id) return;
  logs.value.push(line);
  if (logs.value.length > 5000) logs.value.shift();
  const el = consoleEl.value;
  if (el && el.scrollHeight - el.scrollTop - el.clientHeight < 40) nextTick(() => (el.scrollTop = el.scrollHeight));
}

onMounted(async () => {
  try {
    inst.value = await api.instances.get(props.id);
    // keeps the game's server list in step with the servers' addresses (custom domain, Playit)
    if (inst.value.servers?.length) void api.instances.linkedServers(props.id).catch(() => {});
    logs.value = await api.instances.logs(props.id);
    await loadContent();
  } catch (e) {
    toast.error(errorText(e));
    router.replace("/library");
  }
  logListeners.add(onLog);
  window.addEventListener("tessera-instance-changed", onSynced);
});

// live share sync brought new content: show it
const onSynced = (e: Event) => {
  if ((e as CustomEvent<string>).detail === props.id) void reloadInstance();
};
onUnmounted(() => {
  logListeners.delete(onLog);
  window.removeEventListener("tessera-instance-changed", onSynced);
});

// "last played" is set when the game starts
watch(
  () => run.value.state,
  async (state) => {
    if (state === "running") inst.value = await api.instances.get(props.id);
    if (state === "idle") await loadContent();
  },
);

function lineClass(line: string) {
  return /\bERROR\b|Exception/.test(line) ? "error" : /\bWARN\b/.test(line) ? "warn" : "";
}

async function remove() {
  try {
    await api.instances.remove(props.id);
    router.replace("/library");
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function openFolder() {
  try {
    await api.instances.openFolder(props.id);
  } catch (e) {
    toast.error(errorText(e));
  }
}
</script>

<template>
  <div v-if="inst" class="page enter">
    <RouterLink to="/library" class="row hint" style="margin-bottom: 14px; color: var(--tint); font-weight: 600">
      <ArrowLeft :size="14" /> Library
    </RouterLink>
    <div class="page-header" style="align-items: center">
      <div class="row" style="gap: 16px">
        <button class="icon-edit" title="Change icon" aria-label="Change icon" @click="editingIcon = true">
          <InstanceIcon :icon="inst.icon" :id="inst.id" :size="72" />
          <span class="icon-edit-badge"><Pencil :size="14" /></span>
        </button>
        <div>
          <h1 class="page-title">{{ inst.name }}</h1>
          <div class="row" style="margin-top: 8px; flex-wrap: wrap">
            <StateBadge :id="inst.id" />
            <span class="chip" :style="{ color: LOADERS[inst.loader].color }">
              {{ LOADERS[inst.loader].label }}{{ inst.loader_version ? ` ${inst.loader_version}` : "" }}
            </span>
            <span class="chip num">{{ inst.game_version }}</span>
            <span class="hint">{{ relative(inst.last_played) }}</span>
          </div>
        </div>
      </div>
      <div class="row">
        <button class="btn btn-lg" title="Open folder" @click="openFolder"><FolderOpen /> Folder</button>
        <button class="btn btn-lg" title="Export as .tapack" @click="exporting = true"><PackageCheck /> Export</button>
        <button class="btn btn-lg" title="Create a server with these mods" @click="creatingServer = true"><Server /> Create server</button>
        <PlayButton :id="inst.id" large />
      </div>
    </div>

    <div v-if="run.state === 'preparing'" class="card stack" style="margin-bottom: 20px; gap: 8px">
      <div class="row"><span style="font-weight: 600">{{ run.stage }}</span><span class="spacer" /><span class="hint num">{{ run.total ? `${run.done} / ${run.total}` : "" }}</span></div>
      <div class="progress"><div :style="{ width: run.total ? `${(run.done / run.total) * 100}%` : '6%' }" /></div>
    </div>

    <div v-slider class="tabs">
      <button :class="{ active: tab === 'content' }" @click="goTab('content')"><Package />Content</button>
      <button :class="{ active: tab === 'worlds' }" @click="goTab('worlds')"><Globe />Worlds</button>
      <button :class="{ active: tab === 'logs' }" @click="goTab('logs')"><SquareTerminal />Logs</button>
      <button :class="{ active: tab === 'share' }" @click="goTab('share')"><UserPlus />Share</button>
      <button :class="{ active: tab === 'options' }" @click="goTab('options')"><Settings2 />Options</button>
    </div>

    <div class="panel-host">
    <Transition :name="`panel-${panelDir}`">
    <section v-if="tab === 'content'" class="card stack">
      <div class="row" style="flex-wrap: wrap">
        <div class="choices">
          <button v-for="k in KINDS" :key="k.id" class="choice" style="height: 30px; padding: 0 10px" :class="{ on: kindFilter === k.id }" @click="kindFilter = k.id">
            {{ k.label }}<span v-if="countOf(k.id)" class="choice-count num">{{ countOf(k.id) }}</span>
          </button>
        </div>
        <span class="spacer" />
        <RouterLink
          :to="{ path: `/instance/${inst.id}/add`, query: { type: inst.loader === 'vanilla' ? 'resourcepack' : 'mod' } }"
          class="btn btn-primary btn-sm"
        ><Plus /> Add content</RouterLink>
      </div>
      <p v-if="content.length" class="hint" style="margin: 0">{{ summary }}</p>
      <p v-if="!content.length" class="hint">
        Nothing here yet. {{ inst.loader === "vanilla" ? "Vanilla can use resource packs, data packs and shaders." : "Add mods, resource packs and more from Discover." }}
      </p>
      <div v-for="f in shownContent" :key="f.kind + (f.world ?? '') + f.filename" class="inset row" style="gap: 12px" :style="f.enabled ? '' : 'opacity: .55'">
        <img v-if="f.icon_url" :src="f.icon_url" alt="" width="36" height="36" style="border-radius: 9px" />
        <span v-else class="tile tile-sm" style="--type: #a2a9b4"><Package /></span>
        <div style="flex: 1; min-width: 0">
          <div style="font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap">{{ f.title }}</div>
          <div class="hint mono" style="overflow: hidden; text-overflow: ellipsis; white-space: nowrap">
            {{ f.version ?? f.filename }}{{ f.world ? ` · ${f.world}` : "" }}
          </div>
        </div>
        <span class="chip">{{ KINDS.find((k) => k.id === f.kind)?.label.replace(/s$/, "") }}</span>
        <label class="switch" :title="f.enabled ? 'Turn off' : 'Turn on'">
          <input type="checkbox" :checked="f.enabled" :aria-label="`${f.title} turned on`" @change="toggle(f)" /><span />
        </label>
        <button class="btn btn-ghost btn-icon" :title="`Remove ${f.title}`" :aria-label="`Remove ${f.title}`" @click="removeFile(f)"><Trash2 /></button>
      </div>
    </section>

    <section v-else-if="tab === 'logs'">
      <div ref="consoleEl" class="console">
        <div v-if="!logs.length" style="opacity: 0.6">Press Play; the game's output shows up here.</div>
        <button v-if="!allLogs && logs.length > LOG_TAIL" class="btn btn-sm" style="margin-bottom: 8px" @click="allLogs = true">
          Show all {{ logs.length }} lines
        </button>
        <div v-for="(line, i) in shownLogs" :key="i" :class="lineClass(line)">{{ line }}</div>
      </div>
    </section>

    <section v-else-if="tab === 'worlds'">
      <WorldsPanel :instance="inst" />
    </section>

    <section v-else-if="tab === 'share'">
      <SharePanel :key="`${inst.joined?.code ?? ''}${inst.share_code ?? ''}`" :instance="inst" @changed="reloadInstance" />
    </section>

    <section v-else>
      <InstanceOptions :instance="inst" @saved="(i) => (inst = i)" @delete="confirmDelete = true" />
    </section>
    </Transition>
    </div>

    <CreateServerDialog v-if="creatingServer" :instance="inst" @close="creatingServer = false" />
    <ExportDialog v-if="exporting" :instance="inst" @close="exporting = false" />
    <IconEditor v-if="editingIcon" :icon="inst.icon" @close="editingIcon = false" @save="saveIcon" />

    <AppDialog v-if="confirmDelete" title="Delete this instance?" @close="confirmDelete = false">
      <p class="muted">{{ inst.name }} and its worlds, mods and settings are deleted from this computer. This can't be undone.</p>
      <div class="dialog-footer">
        <button class="btn" @click="confirmDelete = false">Cancel</button>
        <button class="btn btn-danger" @click="remove">Delete</button>
      </div>
    </AppDialog>
  </div>
</template>

<style scoped>
.choice-count {
  margin-left: 6px;
  opacity: 0.6;
  font-size: 12px;
}
</style>
