<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useRouter } from "vue-router";
import { ArrowDownUp, Blocks, Download, Filter, FolderPlus, LayoutGrid, Loader2, Plus, Search, Trash2, X } from "lucide-vue-next";
import AppDialog from "@/components/AppDialog.vue";
import CreateInstanceDialog from "@/components/CreateInstanceDialog.vue";
import LibraryTile from "@/components/LibraryTile.vue";
import { api, type Instance, type Loader, type Share } from "@/lib/api";
import { account, loadAccount } from "@/lib/account";
import { LOADERS, LOADER_ORDER } from "@/lib/format";
import { errorText, toast } from "@/lib/toast";

const router = useRouter();
const instances = ref<Instance[] | null>(null);
const groups = ref<string[]>([]);
const creating = ref(false);

// ── toolbar state (kept for the session) ──
type Sort = "name" | "played" | "created" | "version";
type GroupBy = "custom" | "loader" | "version" | "none";
const query = ref("");
const sort = ref<Sort>((sessionStorage.getItem("lib-sort") as Sort) || "name");
const groupBy = ref<GroupBy>((sessionStorage.getItem("lib-group") as GroupBy) || "custom");
const loaderFilter = ref<Loader[]>([]);
const versionFilter = ref<string[]>([]);
const filterMenu = ref(false);

// ── selection ──
const selected = ref(new Set<string>());
const lastClicked = ref<string | null>(null);
const selecting = computed(() => selected.value.size > 0);

// ── dialogs ──
const groupDialog = ref<"new" | "add" | null>(null);
const groupName = ref("");
const confirmDelete = ref(false);
const busy = ref(false);

async function load() {
  const [list, settings] = await Promise.all([api.instances.list(), api.settings.get()]);
  instances.value = list;
  groups.value = settings.groups;
  // drop selections of instances that are gone
  selected.value = new Set([...selected.value].filter((id) => list.some((i) => i.id === id)));
}

const versionKey = (v: string) => v.split(/\D+/).map(Number);
function compareVersions(a: string, b: string) {
  const x = versionKey(a), y = versionKey(b);
  for (let i = 0; i < Math.max(x.length, y.length); i++) if ((x[i] ?? 0) !== (y[i] ?? 0)) return (y[i] ?? 0) - (x[i] ?? 0);
  return 0;
}

const visible = computed(() => {
  const q = query.value.trim().toLowerCase();
  const list = (instances.value ?? []).filter(
    (i) =>
      (!q || i.name.toLowerCase().includes(q) || i.game_version.includes(q) || LOADERS[i.loader].label.toLowerCase().includes(q)) &&
      (!loaderFilter.value.length || loaderFilter.value.includes(i.loader)) &&
      (!versionFilter.value.length || versionFilter.value.includes(i.game_version)),
  );
  const by: Record<Sort, (a: Instance, b: Instance) => number> = {
    name: (a, b) => a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" }),
    played: (a, b) => (b.last_played ?? "").localeCompare(a.last_played ?? ""),
    created: (a, b) => b.created_at.localeCompare(a.created_at),
    version: (a, b) => compareVersions(a.game_version, b.game_version),
  };
  return [...list].sort(by[sort.value]);
});

/** Sections for the chosen grouping, in display order. */
const sections = computed<{ key: string; title: string; items: Instance[]; group?: string }[]>(() => {
  const list = visible.value;
  if (groupBy.value === "none") return [{ key: "all", title: "", items: list }];
  if (groupBy.value === "loader") {
    return LOADER_ORDER.map((l) => ({ key: l, title: LOADERS[l].label, items: list.filter((i) => i.loader === l) })).filter((s) => s.items.length);
  }
  if (groupBy.value === "version") {
    const versions = [...new Set(list.map((i) => i.game_version))].sort(compareVersions);
    return versions.map((v) => ({ key: v, title: `Minecraft ${v}`, items: list.filter((i) => i.game_version === v) }));
  }
  const out = groups.value.map((g) => ({ key: `g:${g}`, title: g, group: g, items: list.filter((i) => i.groups.includes(g)) }));
  const ungrouped = list.filter((i) => !i.groups.some((g) => groups.value.includes(g)));
  // empty groups still show (so a new group is visible), unless a search/filter is on
  const filtering = query.value.trim() || loaderFilter.value.length || versionFilter.value.length;
  return [...out.filter((s) => s.items.length || !filtering), ...(ungrouped.length ? [{ key: "ungrouped", title: groups.value.length ? "Ungrouped" : "", items: ungrouped }] : [])];
});

/** Flat order on screen, for shift-click ranges. */
const order = computed(() => sections.value.flatMap((s) => s.items.map((i) => i.id)));

function toggle(id: string, shift: boolean) {
  const next = new Set(selected.value);
  if (shift && lastClicked.value) {
    const a = order.value.indexOf(lastClicked.value), b = order.value.indexOf(id);
    if (a !== -1 && b !== -1) for (const x of order.value.slice(Math.min(a, b), Math.max(a, b) + 1)) next.add(x);
  } else if (next.has(id)) next.delete(id);
  else next.add(id);
  lastClicked.value = id;
  selected.value = next;
}

function clearSelection() {
  selected.value = new Set();
  lastClicked.value = null;
}

function onKey(e: KeyboardEvent) {
  const typing = (e.target as HTMLElement)?.closest("input, textarea, select");
  if (e.key === "Escape" && selecting.value && !groupDialog.value && !confirmDelete.value) clearSelection();
  if ((e.metaKey || e.ctrlKey) && e.key === "a" && !typing && instances.value) {
    e.preventDefault();
    selected.value = new Set(order.value);
  }
}

const allVersions = computed(() => [...new Set((instances.value ?? []).map((i) => i.game_version))].sort(compareVersions));
const usedLoaders = computed(() => LOADER_ORDER.filter((l) => (instances.value ?? []).some((i) => i.loader === l)));
const filterCount = computed(() => loaderFilter.value.length + versionFilter.value.length);

function toggleIn<T>(list: T[], v: T): T[] {
  return list.includes(v) ? list.filter((x) => x !== v) : [...list, v];
}

function setSort(v: Sort) {
  sort.value = v;
  sessionStorage.setItem("lib-sort", v);
}
function setGroupBy(v: GroupBy) {
  groupBy.value = v;
  sessionStorage.setItem("lib-group", v);
}

function openGroupDialog(kind: "new" | "add") {
  groupName.value = kind === "add" && groups.value.length ? groups.value[0]! : "";
  groupDialog.value = kind;
}

async function saveGroup() {
  const name = groupName.value.trim();
  if (!name) return;
  busy.value = true;
  try {
    if (selecting.value) {
      await api.instances.setGroup([...selected.value], name, true);
      toast.success(`${selected.value.size} added to ${name}`);
      clearSelection();
    } else {
      await api.groups.create(name);
    }
    if (groupBy.value !== "custom") setGroupBy("custom");
    groupDialog.value = null;
    await load();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = false;
  }
}

async function removeGroup(name: string) {
  await api.groups.remove(name).catch((e) => toast.error(errorText(e)));
  toast.success(`Group "${name}" removed; its instances are still in your library`);
  await load();
}

async function deleteSelected() {
  busy.value = true;
  try {
    const n = await api.instances.removeMany([...selected.value]);
    const skipped = selected.value.size - n;
    toast.success(`Deleted ${n} instance${n === 1 ? "" : "s"}${skipped ? `; ${skipped} running, left alone` : ""}`);
    clearSelection();
    confirmDelete.value = false;
    await load();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = false;
  }
}

// ── shared with me ──
const received = ref<Share[]>([]);
const joining = ref<string | null>(null);

async function join(s: Share) {
  joining.value = s.code;
  try {
    const inst = await api.share.join(s.code);
    toast.success(`${inst.name} is ready`);
    router.push(`/instance/${inst.id}`);
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    joining.value = null;
  }
}

function closeMenus(e: MouseEvent) {
  if (!(e.target as HTMLElement).closest(".lib-filter")) filterMenu.value = false;
}

onMounted(async () => {
  await load();
  window.addEventListener("keydown", onKey);
  document.addEventListener("mousedown", closeMenus);
  if (!account.loaded) await loadAccount();
  if (account.me) received.value = await api.share.received().catch(() => []);
});
onUnmounted(() => {
  window.removeEventListener("keydown", onKey);
  document.removeEventListener("mousedown", closeMenus);
});
</script>

<template>
  <div class="page enter" :style="{ paddingBottom: selecting ? '120px' : undefined }">
    <div class="page-header" style="margin-bottom: 18px">
      <div>
        <h1 class="page-title">Library</h1>
        <p class="page-sub num">{{ instances ? `${instances.length} instance${instances.length === 1 ? "" : "s"}` : "…" }}</p>
      </div>
    </div>

    <div class="row" style="gap: 10px; margin-bottom: 12px">
      <div class="input-icon" style="flex: 1">
        <Search />
        <input v-model="query" class="input" placeholder="Search" />
      </div>
      <button class="btn btn-lg" @click="openGroupDialog('new')"><FolderPlus /> New group</button>
      <button class="btn btn-primary btn-lg" @click="creating = true"><Plus /> New instance</button>
    </div>

    <div class="row" style="gap: 8px; margin-bottom: 22px; flex-wrap: wrap">
      <label class="lib-select" title="Sort by">
        <ArrowDownUp :size="16" />
        <select :value="sort" @change="setSort(($event.target as HTMLSelectElement).value as Sort)">
          <option value="name">Name</option>
          <option value="played">Last played</option>
          <option value="created">Newest</option>
          <option value="version">Game version</option>
        </select>
      </label>
      <label class="lib-select" title="Group by">
        <LayoutGrid :size="16" />
        <select :value="groupBy" @change="setGroupBy(($event.target as HTMLSelectElement).value as GroupBy)">
          <option value="custom">Custom groups</option>
          <option value="loader">Loader</option>
          <option value="version">Game version</option>
          <option value="none">No groups</option>
        </select>
      </label>
      <span style="width: 1px; height: 24px; background: var(--border); margin: 0 4px" />
      <div class="lib-filter" style="position: relative">
        <button class="btn" :class="{ 'btn-tint': filterCount }" @click="filterMenu = !filterMenu">
          <Filter /> {{ filterCount ? `Filters (${filterCount})` : "Add filter" }}
        </button>
        <div v-if="filterMenu" class="lib-menu">
          <span class="label">Loader</span>
          <div class="choices">
            <button v-for="l in usedLoaders" :key="l" class="choice" :class="{ on: loaderFilter.includes(l) }" @click="loaderFilter = toggleIn(loaderFilter, l)">
              {{ LOADERS[l].label }}
            </button>
          </div>
          <span class="label" style="margin-top: 6px">Game version</span>
          <div class="choices" style="max-height: 160px; overflow-y: auto">
            <button v-for="v in allVersions" :key="v" class="choice num" :class="{ on: versionFilter.includes(v) }" @click="versionFilter = toggleIn(versionFilter, v)">
              {{ v }}
            </button>
          </div>
        </div>
      </div>
      <button v-for="l in loaderFilter" :key="l" class="chip lib-active" @click="loaderFilter = toggleIn(loaderFilter, l)">{{ LOADERS[l].label }} <X :size="13" /></button>
      <button v-for="v in versionFilter" :key="v" class="chip lib-active num" @click="versionFilter = toggleIn(versionFilter, v)">{{ v }} <X :size="13" /></button>
    </div>

    <div v-if="instances && !instances.length" class="card empty">
      <Blocks />
      <h3>Nothing here yet</h3>
      <p>Create an instance to start playing.</p>
      <button class="btn btn-primary" @click="creating = true"><Plus /> New instance</button>
    </div>
    <div v-else-if="instances && !visible.length" class="card empty">
      <h3>No instance matches</h3>
      <p>Try another search or fewer filters.</p>
    </div>

    <template v-else>
      <section v-for="s in sections" :key="s.key" style="margin-bottom: 26px">
        <div v-if="s.title" class="row lib-section">
          <h2>{{ s.title }}</h2>
          <span class="hint num">{{ s.items.length }}</span>
          <span class="spacer" />
          <button v-if="s.group" class="btn btn-ghost btn-sm" :title="`Remove the group ${s.title}`" @click="removeGroup(s.group)"><X /> Remove group</button>
        </div>
        <div v-if="s.items.length" class="lib-grid">
          <LibraryTile
            v-for="inst in s.items"
            :key="inst.id"
            :instance="inst"
            :selected="selected.has(inst.id)"
            :selecting="selecting"
            @toggle="(shift) => toggle(inst.id, shift)"
          />
        </div>
        <p v-else class="hint">Empty. Select instances and choose <b>Add to group</b>.</p>
      </section>
    </template>

    <template v-if="received.length">
      <h2 class="section-title">Shared with me</h2>
      <div class="grid">
        <div v-for="s in received" :key="s.code" class="card stack">
          <div style="font-weight: 700; font-size: 16px">{{ s.name }}</div>
          <div class="row" style="flex-wrap: wrap">
            <span class="chip" :style="{ color: LOADERS[s.loader].color }">{{ LOADERS[s.loader].label }}</span>
            <span class="chip num">{{ s.game_version }}</span>
            <span class="hint">from {{ s.owner ?? "a friend" }}</span>
          </div>
          <button class="btn btn-primary" style="align-self: flex-start" :disabled="joining !== null" @click="join(s)">
            <Loader2 v-if="joining === s.code" class="spin" /><Download v-else /> Install
          </button>
        </div>
      </div>
    </template>

    <!-- selection bar (in <body>: the page's entrance animation would pin it to the page) -->
    <Teleport to="body">
    <Transition name="selbar">
      <div v-if="selecting" class="lib-selbar" role="toolbar" aria-label="Selection">
        <span style="font-weight: 700">{{ selected.size }} selected</span>
        <span class="lib-sep" />
        <button class="btn btn-ghost" @click="clearSelection">Clear</button>
        <button class="btn btn-ghost" @click="selected = new Set(order)">Select all</button>
        <span class="spacer" />
        <button class="btn btn-ghost" @click="openGroupDialog('add')"><FolderPlus /> Add to group</button>
        <span class="lib-sep" />
        <button class="btn btn-ghost" style="color: var(--destructive)" @click="confirmDelete = true"><Trash2 /> Delete</button>
      </div>
    </Transition>
    </Teleport>

    <AppDialog v-if="groupDialog" :title="selecting ? `Add ${selected.size} to a group` : 'New group'" :width="420" @close="groupDialog = null">
      <form class="stack" @submit.prevent="saveGroup">
        <div v-if="selecting && groups.length" class="choices">
          <button v-for="g in groups" :key="g" type="button" class="choice" :class="{ on: groupName === g }" @click="groupName = g">{{ g }}</button>
        </div>
        <div class="field">
          <label for="grp-name">{{ selecting && groups.length ? "Or a new group" : "Name" }}</label>
          <input id="grp-name" v-model="groupName" class="input" maxlength="40" placeholder="e.g. Modpacks, With friends" autofocus />
        </div>
        <div class="dialog-footer" style="margin-top: 4px">
          <button type="button" class="btn btn-lg" @click="groupDialog = null">Cancel</button>
          <button class="btn btn-primary btn-lg" :disabled="busy || !groupName.trim()"><Loader2 v-if="busy" class="spin" />{{ selecting ? "Add" : "Create group" }}</button>
        </div>
      </form>
    </AppDialog>

    <AppDialog v-if="confirmDelete" :title="`Delete ${selected.size} instance${selected.size === 1 ? '' : 's'}?`" @close="confirmDelete = false">
      <p class="muted">Their worlds, mods and settings are deleted from this computer. This can't be undone.</p>
      <p class="hint" style="margin-top: 8px">{{ (instances ?? []).filter((i) => selected.has(i.id)).map((i) => i.name).join(", ") }}</p>
      <div class="dialog-footer">
        <button class="btn" @click="confirmDelete = false">Cancel</button>
        <button class="btn btn-danger" :disabled="busy" @click="deleteSelected"><Loader2 v-if="busy" class="spin" />Delete</button>
      </div>
    </AppDialog>

    <CreateInstanceDialog v-if="creating" @close="(creating = false), load()" />
  </div>
</template>

<style scoped>
.lib-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
  gap: 14px;
}
.lib-section {
  margin-bottom: 12px;
}
.lib-section h2 {
  font-size: 17px;
  font-weight: 700;
}
.lib-select {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  height: 36px;
  padding: 0 10px 0 12px;
  border: 1px solid var(--border);
  color: var(--muted-foreground);
  cursor: pointer;
}
.lib-select select {
  border: none;
  background: transparent;
  color: var(--foreground);
  font: inherit;
  font-weight: 600;
  outline: none;
  cursor: pointer;
}
html[data-design="modern"] .lib-select {
  border: none;
  border-radius: 12px;
  background: var(--fill);
}
.lib-menu {
  position: absolute;
  top: 42px;
  left: 0;
  z-index: 30;
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: 320px;
  padding: 14px;
  background: var(--popover);
  border: 1px solid var(--border);
  box-shadow: var(--raised-shadow);
}
html[data-design="modern"] .lib-menu {
  border: none;
  border-radius: 16px;
}
.lib-active {
  cursor: pointer;
  color: var(--tint);
}
.lib-selbar {
  position: fixed;
  left: 50%;
  bottom: 24px;
  z-index: 40;
  translate: -50% 0;
  display: flex;
  align-items: center;
  gap: 6px;
  width: min(880px, calc(100% - 48px));
  padding: 10px 14px 10px 22px;
  background: var(--popover);
  border: 1px solid var(--border);
  box-shadow: var(--raised-shadow);
}
html[data-design="modern"] .lib-selbar {
  border-radius: 20px;
}
.lib-sep {
  width: 1px;
  height: 24px;
  background: var(--border);
  margin: 0 6px;
}
.selbar-enter-active {
  transition: opacity 220ms var(--ease-out, ease), translate 360ms var(--ease-out, ease);
}
.selbar-leave-active {
  transition: opacity 140ms ease, translate 140ms ease;
}
.selbar-enter-from,
.selbar-leave-to {
  opacity: 0;
  translate: -50% 16px;
}
</style>
