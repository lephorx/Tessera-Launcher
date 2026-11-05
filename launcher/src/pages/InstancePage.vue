<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { RouterLink, useRouter } from "vue-router";
import { open } from "@tauri-apps/plugin-dialog";
import { ArrowDownAZ, ArrowLeft, ArrowLeftRight, Clock, Filter, RefreshCw, Compass, Eye, FolderOpen, Globe, Inbox, Lock, Package, PackageCheck, Pencil, Plus, Search, Server, Settings, SquareTerminal, Trash2, Upload, UserPlus, Users, Wrench } from "lucide-vue-next";
import AppDialog from "@/components/AppDialog.vue";
import ExportDialog from "@/components/ExportDialog.vue";
import InstanceSettingsDialog, { type InstanceSection } from "@/components/InstanceSettingsDialog.vue";
import ModpackPanel from "@/components/ModpackPanel.vue";
import PlayerHead from "@/components/PlayerHead.vue";
import SharedContentDialog from "@/components/SharedContentDialog.vue";
import RowMenu, { type RowMenuItem } from "@/components/RowMenu.vue";
import SharePanel from "@/components/SharePanel.vue";
import WorldsPanel from "@/components/WorldsPanel.vue";
import IconEditor from "@/components/IconEditor.vue";
import InstanceIcon from "@/components/InstanceIcon.vue";
import PlayButton from "@/components/PlayButton.vue";
import StateBadge from "@/components/StateBadge.vue";
import { api, type ContentFile, type Icon, type Instance } from "@/lib/api";
import { iconVersion } from "@/lib/iconCache";
import { tabDirection, type PanelDirection } from "@/lib/tabSwitch";
import { LOADERS, horizonBuild, relative } from "@/lib/format";
import { logListeners, runOf, shareSync } from "@/lib/store";
import { errorText, toast } from "@/lib/toast";

const props = defineProps<{ id: string }>();
const router = useRouter();
const inst = ref<Instance | null>(null);
const TAB_ORDER = ["content", "worlds", "logs", "share"] as const;
const tab = ref<(typeof TAB_ORDER)[number]>("content");
const panelDir = ref<PanelDirection>("none");
function goTab(to: (typeof TAB_ORDER)[number]) {
  panelDir.value = tabDirection(TAB_ORDER, tab.value, to);
  tab.value = to;
}
const content = ref<ContentFile[]>([]);
const kindFilter = ref<"all" | ContentFile["kind"]>("all");
const contentQuery = ref("");
const editingIcon = ref(false);
const exporting = ref(false);
/** The settings popup, on this section (null: closed). */
const settingsAt = ref<InstanceSection | null>(null);
const viewingShared = ref(false);
type KindId = "all" | ContentFile["kind"];
const KINDS = computed<{ id: KindId; label: string }[]>(() =>
  isHorizon.value
    ? [
        { id: "all", label: "All" },
        { id: "native-mod", label: "Native mods" },
        { id: "horizon-mod", label: "Content mods" },
        { id: "resourcepack", label: "Resource packs" },
        { id: "shader", label: "Shaders" },
      ]
    : [
        { id: "all", label: "All" },
        { id: "mod", label: "Mods" },
        { id: "resourcepack", label: "Resource packs" },
        { id: "shader", label: "Shaders" },
        { id: "datapack", label: "Data packs" },
      ],
);
const isHorizon = computed(() => inst.value?.loader === "horizon");
const countOf = (kind: KindId) => ownContent.value.filter((c) => kind === "all" || c.kind === kind).length;
// "42 mods · 3 resource packs · 2 turned off" under the filters
const summary = computed(() => {
  const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
  const parts = [
    isHorizon.value ? plural(countOf("native-mod") + countOf("horizon-mod"), "mod", "mods") : plural(countOf("mod"), "mod", "mods"),
    countOf("resourcepack") && plural(countOf("resourcepack"), "resource pack", "resource packs"),
    countOf("shader") && plural(countOf("shader"), "shader", "shaders"),
    countOf("datapack") && plural(countOf("datapack"), "data pack", "data packs"),
  ].filter(Boolean);
  const off = ownContent.value.filter((c) => !c.enabled).length;
  if (off) parts.push(`${off} turned off`);
  return parts.join(" · ");
});
/** A modpack instance whose pack Tessera can update (from Modrinth). */
const fromModpack = computed(() => !!inst.value?.pack?.project_id);
/** Content added since the modpack was installed (the pack's files are known for packs installed since 0.6.22). */
const addedCount = computed(() => (inst.value?.pack?.files.length ? content.value.filter((c) => !c.from_pack && c.kind !== "datapack").length : 0));
async function packChanged(i: Instance) {
  inst.value = i;
  await loadContent();
}
// ── a copy of someone else's shared pack: the pack's content, and this computer's own on top ──
const joined = computed(() => inst.value?.joined ?? null);
const canEdit = computed(() => joined.value?.role === "editor");
/** What the pack brings (data packs live in worlds: always this copy's own). */
const sharedFiles = computed(() => (joined.value ? content.value.filter((c) => !c.local && c.kind !== "datapack") : []));
/** Content listed on the page: on a joined pack only this computer's own. */
const ownContent = computed(() => (joined.value ? content.value.filter((c) => c.local || c.kind === "datapack") : content.value));
const sharedSummary = computed(() => {
  const n = (kinds: ContentFile["kind"][]) => sharedFiles.value.filter((f) => kinds.includes(f.kind)).length;
  const plural = (k: number, one: string, many: string) => k && `${k} ${k === 1 ? one : many}`;
  return [plural(n(["mod", "horizon-mod", "native-mod"]), "mod", "mods"), plural(n(["resourcepack"]), "resource pack", "resource packs"), plural(n(["shader"]), "shader", "shaders")]
    .filter(Boolean)
    .join(" · ") || "Nothing yet";
});
const syncText = computed(() => {
  const s = shareSync[props.id];
  if (s?.state === "error") return "Last sync failed";
  if (s?.state === "waiting") return "An update waits for the game to close";
  if (s?.state === "paused") return "Paused: a new version waits for your answer";
  if (s) return `Synced ${relative(new Date(s.at).toISOString()).toLowerCase()}`;
  return "Live";
});
/** May change the shared pack (owner or editor): what they add goes to everyone unless they pick "Only for me". */
const canShare = computed(() => !!inst.value?.share_code || canEdit.value);
const onlyMe = ref(false);
/** What's added now stays on this computer: always for members who can only play. */
const addsLocally = computed(() => (joined.value && !canEdit.value) || (canShare.value && onlyMe.value));
/** Where "Browse content" adds (?local=1: this computer only). */
const addRoute = computed(() => {
  const i = inst.value!;
  const local = addsLocally.value ? { local: "1" } : {};
  if (i.loader === "horizon") return { path: `/instance/${i.id}/horizon`, query: local };
  return { path: `/instance/${i.id}/add`, query: { type: i.loader === "vanilla" ? "resourcepack" : "mod", ...local } };
});

async function uploadFiles() {
  const picked = await open({ multiple: true, title: "Add mods, resource packs or shaders", filters: [{ name: "Mods, resource packs and shaders", extensions: ["jar", "zip"] }] });
  const files = Array.isArray(picked) ? picked : typeof picked === "string" ? [picked] : [];
  if (!files.length) return;
  try {
    inst.value = await api.instances.addFiles(props.id, files, addsLocally.value ? true : canShare.value ? false : null);
    await loadContent();
    toast.success(files.length === 1 ? "Added" : `${files.length} files added`);
  } catch (e) {
    toast.error(errorText(e));
  }
}

/** An editor puts a file of their own in the pack for everyone. */
async function shareFile(f: ContentFile) {
  try {
    inst.value = await api.instances.setLocal(props.id, f, false);
    await loadContent();
    toast.success(`${f.title} goes to everyone on the pack`);
  } catch (e) {
    toast.error(errorText(e));
  }
}

function openSettings(section: InstanceSection = "general") {
  settingsAt.value = section;
}
/** From the settings popup: it closes first, so Escape only closes what's on top. */
function fromSettings(then: () => void) {
  settingsAt.value = null;
  then();
}

// the search matches a name, file name, version or world, in any order of words
const shownContent = computed(() => {
  const words = contentQuery.value.toLowerCase().split(/\s+/).filter(Boolean);
  const list = ownContent.value.filter((c) => {
    if (kindFilter.value !== "all" && c.kind !== kindFilter.value) return false;
    const text = `${c.title} ${c.filename} ${c.version ?? ""} ${c.world ?? ""}`.toLowerCase();
    return words.every((w) => text.includes(w));
  });
  const byName = (a: ContentFile, b: ContentFile) => a.title.localeCompare(b.title, undefined, { sensitivity: "base" });
  if (contentSort.value === "za") return list.sort((a, b) => byName(b, a));
  if (contentSort.value === "off") return list.sort((a, b) => Number(a.enabled) - Number(b.enabled) || byName(a, b));
  return list.sort(byName);
});
const contentSort = ref<"az" | "za" | "off">("az");
/** Kinds there is something of (and the one picked, so it can be unpicked). */
const shownKinds = computed(() => KINDS.value.filter((k) => k.id === "all" || countOf(k.id) || kindFilter.value === k.id));

// ── picking several rows ──
const keyOf = (f: ContentFile) => f.kind + (f.world ?? "") + f.filename;
const selected = ref(new Set<string>());
const selectedFiles = computed(() => ownContent.value.filter((f) => selected.value.has(keyOf(f))));
const allSelected = computed(() => shownContent.value.length > 0 && shownContent.value.every((f) => selected.value.has(keyOf(f))));
function selectAll() {
  selected.value = allSelected.value ? new Set() : new Set(shownContent.value.map(keyOf));
}
function select(f: ContentFile) {
  const next = new Set(selected.value);
  if (!next.delete(keyOf(f))) next.add(keyOf(f));
  selected.value = next;
}
const bulkBusy = ref(false);
async function bulk(what: "on" | "off" | "remove") {
  bulkBusy.value = true;
  try {
    for (const f of selectedFiles.value) {
      if (what === "remove") inst.value = await api.instances.removeContent(props.id, f);
      else if ((what === "on") !== f.enabled) await api.instances.toggleContent(props.id, f);
    }
    if (what === "remove") selected.value = new Set();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    bulkBusy.value = false;
    await loadContent();
  }
}

const refreshing = ref(false);
async function refresh() {
  refreshing.value = true;
  try {
    await reloadInstance();
  } finally {
    refreshing.value = false;
  }
}

/** Another version from the project's page. */
const canChangeVersion = (f: ContentFile) => !!f.project_id && !isHorizon.value && f.kind !== "datapack";
function changeVersion(f: ContentFile) {
  router.push({ path: `/project/${f.project_id}`, query: { instance: props.id, ...(f.local && joined.value ? { local: "1" } : {}) } });
}
function rowMenu(f: ContentFile): RowMenuItem[] {
  const items: RowMenuItem[] = [];
  if (canChangeVersion(f)) items.push({ label: "Other versions", icon: ArrowLeftRight, action: () => changeVersion(f) });
  if (f.local && f.kind !== "datapack" && canShare.value) items.push({ label: "Add to the pack for everyone", icon: Users, action: () => shareFile(f) });
  if (!f.local && f.kind !== "datapack" && inst.value?.share_code) {
    items.push({
      label: "Keep only on this computer",
      icon: Lock,
      action: async () => {
        try {
          inst.value = await api.instances.setLocal(props.id, f, true);
          await loadContent();
          toast.success(`${f.title} stays on this computer and leaves the pack for the others`);
        } catch (e) {
          toast.error(errorText(e));
        }
      },
    });
  }
  items.push({ label: "Show in folder", icon: FolderOpen, action: () => openFolder(f.world ?? undefined) });
  items.push({ label: "Remove", icon: Trash2, danger: true, action: () => removeFile(f) });
  return items;
}

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

async function openFolder(world?: string) {
  try {
    await api.instances.openFolder(props.id, world);
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
              {{ LOADERS[inst.loader].label }}{{ isHorizon ? ` · ${horizonBuild(inst.loader_version)}` : inst.loader_version ? ` ${inst.loader_version}` : "" }}
            </span>
            <span class="chip num">{{ inst.game_version }}</span>
            <span class="hint">{{ relative(inst.last_played) }}</span>
          </div>
        </div>
      </div>
      <div class="row">
        <button class="btn btn-lg" title="Open folder" @click="openFolder()"><FolderOpen /> Folder</button>
        <button v-if="!isHorizon" class="btn btn-lg" title="Export as .tapack" @click="exporting = true"><PackageCheck /> Export</button>
        <button class="btn btn-lg" :title="isHorizon ? 'Create a Horizon server' : 'Create a server with these mods'" @click="router.push(`/servers/host?instance=${inst.id}`)"><Server /> Create server</button>
        <PlayButton :id="inst.id" large />
        <button class="btn btn-lg btn-icon" title="Settings" aria-label="Settings" @click="openSettings()"><Settings /></button>
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
    </div>

    <div class="panel-host">
    <Transition :name="`panel-${panelDir}`">
    <section v-if="tab === 'content'" class="stack" style="gap: 16px">
    <div v-if="joined" class="card shared-card">
      <div class="row shared-card-head">
        <div style="flex: 1; min-width: 0">
          <h3 class="shared-card-title">Shared content</h3>
          <div class="hint row" style="gap: 6px; margin-top: 4px"><Package :size="16" /> {{ sharedSummary }}</div>
        </div>
        <button class="btn" @click="viewingShared = true"><Eye /> View content</button>
        <button class="btn btn-icon" title="Installation settings" aria-label="Installation settings" @click="openSettings('installation')"><Wrench /></button>
      </div>
      <div class="row shared-card-foot hint">
        <span class="row" style="gap: 8px">
          <Lock :size="16" /> Provided by
          <PlayerHead v-if="joined.owner" :name="joined.owner" :size="22" />
          <b class="ellipsis">{{ joined.owner ?? "a friend" }}</b>
        </span>
        <span class="sep-dot" />
        <span class="row" style="gap: 6px"><Clock :size="16" /> {{ syncText }}</span>
        <template v-if="canEdit"><span class="sep-dot" /><span class="row" style="gap: 6px"><Users :size="16" /> You can add content for everyone</span></template>
      </div>
    </div>

    <div v-if="joined && !ownContent.length" class="empty extras-empty">
      <Inbox />
      <h3>No extra content installed</h3>
      <p v-if="canEdit">Add content to the pack for everyone, or on top of it only for you.</p>
      <p v-else>Add content on top of this modpack. It stays on this computer; the others don't get it.</p>
      <div v-if="canEdit" class="choices" role="radiogroup" aria-label="Who gets what you add" style="margin-top: 4px">
        <button class="choice" role="radio" :aria-checked="!onlyMe" :class="{ on: !onlyMe }" @click="onlyMe = false"><Users :size="14" /> For everyone</button>
        <button class="choice" role="radio" :aria-checked="onlyMe" :class="{ on: onlyMe }" @click="onlyMe = true"><Lock :size="14" /> Only for me</button>
      </div>
      <div class="row" style="margin-top: 8px">
        <button v-if="!isHorizon" class="btn btn-lg" @click="uploadFiles"><Upload /> Upload files</button>
        <RouterLink :to="addRoute" class="btn btn-primary btn-lg"><Compass /> Browse content</RouterLink>
      </div>
    </div>

    <div v-else class="stack" style="gap: 14px">
      <h3 v-if="joined" class="extras-title">Additional content</h3>
      <ModpackPanel v-if="fromModpack" :key="inst.pack?.version_id ?? ''" :instance="inst" :added="addedCount" @changed="packChanged" />
      <div class="row" style="gap: 10px">
        <div class="input-icon" style="flex: 1; min-width: 180px">
          <Search />
          <input v-model="contentQuery" class="input content-search" :placeholder="`Search ${ownContent.length} project${ownContent.length === 1 ? '' : 's'}…`" aria-label="Search content" @keydown.esc="contentQuery = ''" />
        </div>
        <div v-if="canShare" class="input-icon add-for" title="Who gets what you add: everyone on the shared pack, or only this computer">
          <component :is="onlyMe ? Lock : Users" />
          <select v-model="onlyMe" class="select" aria-label="Who gets what you add">
            <option :value="false">For everyone</option>
            <option :value="true">Only for me</option>
          </select>
        </div>
        <button v-if="!isHorizon" class="btn btn-lg" title="Add .jar mods and .zip resource packs or shaders from this computer" @click="uploadFiles"><FolderOpen /> Upload files</button>
        <RouterLink :to="addRoute" class="btn btn-primary btn-lg"><Compass /> {{ isHorizon && !joined ? "Browse Horizon mods" : "Browse content" }}</RouterLink>
      </div>
      <div class="row" style="flex-wrap: wrap; gap: 10px">
        <div class="input-icon content-sort">
          <ArrowDownAZ />
          <select v-model="contentSort" class="select" aria-label="Sort">
            <option value="az">Name (A-Z)</option>
            <option value="za">Name (Z-A)</option>
            <option value="off">Turned off first</option>
          </select>
        </div>
        <span class="vsep" />
        <Filter :size="16" class="muted" />
        <div class="choices">
          <button v-for="k in shownKinds" :key="k.id" class="choice" :class="{ on: kindFilter === k.id }" @click="kindFilter = k.id">
            {{ k.label }}<span v-if="k.id !== 'all' && countOf(k.id)" class="choice-count num">{{ countOf(k.id) }}</span>
          </button>
        </div>
        <span class="spacer" />
        <button class="btn btn-ghost" :disabled="refreshing" title="Read the folder again" @click="refresh"><RefreshCw :class="{ spin: refreshing }" /> Refresh</button>
      </div>

      <div v-if="selectedFiles.length" class="row bulk-bar">
        <b class="num">{{ selectedFiles.length }} selected</b>
        <span class="spacer" />
        <button class="btn btn-sm" :disabled="bulkBusy" @click="bulk('on')">Turn on</button>
        <button class="btn btn-sm" :disabled="bulkBusy" @click="bulk('off')">Turn off</button>
        <button class="btn btn-sm btn-danger" :disabled="bulkBusy" @click="bulk('remove')"><Trash2 /> Remove</button>
        <button class="btn btn-sm btn-ghost" @click="selected = new Set()">Clear</button>
      </div>

      <div class="content-table">
        <div class="ct-row ct-head">
          <input type="checkbox" :checked="allSelected" :disabled="!shownContent.length" aria-label="Select all" @change="selectAll" />
          <span>Project</span><span>Version</span><span style="text-align: right">Actions</span>
        </div>
        <p v-if="!ownContent.length" class="hint ct-empty">
          Nothing here yet.
          {{
            isHorizon
              ? "Add mods, resource packs and shaders from Horizon's registry. Native mods are compiled into the game the next time you press Play."
              : inst.loader === "vanilla"
                ? "Vanilla can use resource packs, data packs and shaders."
                : "Add mods, resource packs and more with Browse content."
          }}
        </p>
        <p v-else-if="!shownContent.length" class="hint ct-empty">
          Nothing{{ contentQuery.trim() ? ` matches “${contentQuery.trim()}”` : " of this kind" }}.
          <button v-if="contentQuery" class="btn btn-ghost btn-sm" @click="contentQuery = ''">Clear search</button>
        </p>
        <div v-for="f in shownContent" :key="keyOf(f)" class="ct-row" :class="{ off: !f.enabled, picked: selected.has(keyOf(f)) }">
          <input type="checkbox" :checked="selected.has(keyOf(f))" :aria-label="`Select ${f.title}`" @change="select(f)" />
          <div class="row" style="gap: 14px; min-width: 0">
            <img v-if="f.icon_url" :src="f.icon_url" alt="" width="48" height="48" class="ct-icon" />
            <span v-else class="tile ct-icon" style="--type: #a2a9b4"><Package /></span>
            <div style="min-width: 0">
              <div class="row" style="gap: 6px; min-width: 0">
                <Package :size="15" class="muted" style="flex-shrink: 0" />
                <RouterLink v-if="canChangeVersion(f)" :to="{ path: `/project/${f.project_id}`, query: { instance: inst.id } }" class="content-title">{{ f.title }}</RouterLink>
                <span v-else class="content-title">{{ f.title }}</span>
              </div>
              <div class="row hint ct-sub">
                <span>{{ KINDS.find((k) => k.id === f.kind)?.label.replace(/s$/, "") }}{{ f.world ? ` · ${f.world}` : "" }}</span>
                <span v-if="inst.pack?.files.length && !f.from_pack && f.kind !== 'datapack'" class="chip" title="Not part of the modpack: updates keep it">Added</span>
                <span v-if="!joined && f.local" class="chip" title="Not in the shared pack: the others don't get it"><Lock /> Only for me</span>
              </div>
            </div>
          </div>
          <div style="min-width: 0">
            <div class="ellipsis" style="font-weight: 600">{{ f.version ?? "—" }}</div>
            <div class="hint ellipsis">{{ f.filename }}</div>
          </div>
          <div class="row" style="justify-content: flex-end; gap: 6px">
            <button v-if="canChangeVersion(f)" class="btn btn-ghost btn-icon" :title="`Other versions of ${f.title}`" :aria-label="`Other versions of ${f.title}`" @click="changeVersion(f)"><ArrowLeftRight /></button>
            <label class="switch" :title="f.enabled ? 'Turn off' : 'Turn on'">
              <input type="checkbox" :checked="f.enabled" :aria-label="`${f.title} turned on`" @change="toggle(f)" /><span />
            </label>
            <button class="btn btn-ghost btn-icon" :title="`Remove ${f.title}`" :aria-label="`Remove ${f.title}`" @click="removeFile(f)"><Trash2 /></button>
            <RowMenu :items="rowMenu(f)" :label="`${f.title}: more`" />
          </div>
        </div>
      </div>
      <p v-if="ownContent.length" class="hint" style="margin: 0">{{ summary }}</p>
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

    <section v-else>
      <SharePanel :key="`${inst.joined?.code ?? ''}${inst.share_code ?? ''}`" :instance="inst" @changed="reloadInstance" />
    </section>
    </Transition>
    </div>

    <ExportDialog v-if="exporting" :instance="inst" @close="exporting = false" />
    <InstanceSettingsDialog
      v-if="settingsAt"
      :instance="inst"
      :section="settingsAt"
      @close="settingsAt = null"
      @saved="(i) => (inst = i, loadContent())"
      @icon="fromSettings(() => (editingIcon = true))"
      @delete="fromSettings(() => (confirmDelete = true))"
      @share="fromSettings(() => goTab('share'))"
    />
    <SharedContentDialog
      v-if="viewingShared && joined"
      :instance="inst"
      :files="sharedFiles"
      :owner="joined.owner"
      :can-edit="canEdit"
      @close="viewingShared = false"
      @changed="reloadInstance"
    />
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
.content-title {
  display: block;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: inherit;
  text-decoration: none;
}
.content-title:hover {
  color: var(--tint);
}
html .card.shared-card {
  padding: 0;
  overflow: hidden;
}
.shared-card-head {
  gap: 10px;
  padding: 18px 20px;
}
.shared-card-title {
  font-size: 17px;
  font-weight: 700;
}
.shared-card-foot {
  flex-wrap: wrap;
  gap: 10px;
  padding: 12px 20px;
  border-top: 1px solid var(--border);
  background: var(--fill);
  font-weight: 600;
}
.sep-dot {
  width: 4px;
  height: 4px;
  border-radius: 999px;
  background: currentColor;
  opacity: 0.5;
}
.extras-empty {
  padding: 48px 24px 56px;
}
.extras-empty > svg {
  width: 56px;
  height: 56px;
  opacity: 0.5;
}
.extras-empty .btn svg {
  width: 16px;
  height: 16px;
}
.extras-empty h3 {
  font-size: 20px;
}
.extras-title {
  margin-top: 6px;
  font-size: 20px;
  font-weight: 700;
}
.content-search {
  height: 44px;
}
.content-sort {
  width: 200px;
}
.content-sort .select {
  padding-left: 34px;
}
.add-for {
  width: 160px;
}
.add-for .select {
  height: 40px;
  padding-left: 34px;
}
.vsep {
  width: 1px;
  height: 24px;
  background: var(--border);
}
.bulk-bar {
  gap: 8px;
  padding: 8px 12px;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--fill);
}
.content-table {
  border: 1px solid var(--border);
  border-radius: 16px;
  background: var(--card);
  overflow: hidden;
}
.ct-row {
  display: grid;
  grid-template-columns: 20px minmax(0, 1.3fr) minmax(0, 1fr) 176px;
  align-items: center;
  gap: 18px;
  padding: 12px 18px;
}
.ct-row + .ct-row {
  border-top: 1px solid var(--border);
}
.ct-head {
  padding-block: 14px;
  background: var(--fill);
  font-weight: 700;
  color: var(--muted-foreground);
}
.ct-row.picked {
  background: color-mix(in srgb, var(--tint) 7%, transparent);
}
.ct-row.off > :nth-child(2),
.ct-row.off > :nth-child(3) {
  opacity: 0.55;
}
.ct-icon {
  flex-shrink: 0;
  width: 48px;
  height: 48px;
  border-radius: 10px;
}
.ct-sub {
  gap: 6px;
  margin-top: 2px;
}
.ct-sub .chip {
  height: 20px;
  padding: 0 6px;
  font-size: 11px;
}
.ct-sub .chip svg {
  width: 11px;
  height: 11px;
}
.ct-empty {
  margin: 0;
  padding: 28px 18px;
  text-align: center;
  border-top: 1px solid var(--border);
}
.choice-count {
  margin-left: 6px;
  opacity: 0.6;
  font-size: 12px;
}
</style>
