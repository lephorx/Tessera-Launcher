<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { RouterLink, useRoute, useRouter } from "vue-router";
import { ArrowLeft, Boxes, Check, ChevronLeft, ChevronRight, Database, Download, Globe, Heart, History, Image, Loader2, Monitor, Package, Plus, Puzzle, Search, Server, Sun } from "lucide-vue-next";
import InstanceIcon from "@/components/InstanceIcon.vue";
import InstallDialog from "@/components/InstallDialog.vue";
import VersionPicker from "@/components/VersionPicker.vue";
import { api, type GameVersion, type Instance, type ProjectHit, type ProjectType } from "@/lib/api";
import { LOADERS, compact, relative } from "@/lib/format";
import { errorText, toast } from "@/lib/toast";
import { tabDirection, type PanelDirection } from "@/lib/tabSwitch";

/** Set on /instance/:id/add: browsing for one instance, installs go straight into it. */
const props = defineProps<{ instanceId?: string }>();
const route = useRoute();
const router = useRouter();

const TABS = [
  { type: "modpack", label: "Modpacks", icon: Boxes },
  { type: "mod", label: "Mods", icon: Puzzle },
  { type: "resourcepack", label: "Resource Packs", icon: Image },
  { type: "datapack", label: "Data Packs", icon: Database },
  { type: "shader", label: "Shaders", icon: Sun },
] as const satisfies readonly { type: ProjectType; label: string; icon: unknown }[];
const SORTS = [
  { id: "relevance", label: "Relevance" },
  { id: "downloads", label: "Downloads" },
  { id: "follows", label: "Follows" },
  { id: "newest", label: "Newest" },
  { id: "updated", label: "Updated" },
];

const type = ref<ProjectType>((route.query.type as ProjectType) || "modpack");
const query = ref((route.query.q as string) || "");
const sort = ref("relevance");
const limit = ref(20);
const page = ref(1);
const hits = ref<ProjectHit[]>([]);
const total = ref(0);
const loading = ref(false);
const installing = ref<ProjectHit | null>(null);
/** Game version filter when browsing without an instance ("" = any). */
const gameVersion = ref("");
const gameVersions = ref<GameVersion[]>([]);
const packBusy = ref<string | null>(null);

/** Opened from an instance ("Add content"): filter to what fits it. */
const instance = ref<Instance | null>(null);
const tabs = computed(() => {
  const inst = instance.value;
  if (!inst) return TABS;
  // vanilla has no mod loader, so no mods
  return TABS.filter((t) => t.type !== "modpack" && !(inst.loader === "vanilla" && t.type === "mod"));
});
const panelDir = ref<PanelDirection>("none");
function goType(to: ProjectType) {
  panelDir.value = tabDirection(tabs.value.map((t) => t.type), type.value, to);
  type.value = to;
  hits.value = []; // the new panel starts empty, results fade in when they arrive
  total.value = 0;
}
const pages = computed(() => Math.max(1, Math.ceil(Math.min(total.value, 10_000) / limit.value)));

// ── adding to an instance ──
/** Projects already in the instance (data packs: in the chosen world). */
const installed = computed(() => {
  const set = new Set<string>();
  for (const m of instance.value?.mods ?? []) {
    if (m.kind !== "datapack" || m.world === world.value) set.add(m.project_id);
  }
  return set;
});
/** Clicked and waiting or installing; installs run one after another. */
const pending = ref(new Set<string>());
const worlds = ref<string[]>([]);
const world = ref("");
let queue: Promise<void> = Promise.resolve();

function addNow(h: ProjectHit) {
  const inst = instance.value;
  if (!inst || pending.value.has(h.project_id) || installed.value.has(h.project_id)) return;
  if (h.project_type === "datapack" && !world.value) {
    toast.error("Data packs go into a world. Play once and create a world first.");
    return;
  }
  pending.value = new Set(pending.value).add(h.project_id);
  const dpWorld = h.project_type === "datapack" ? world.value : null;
  queue = queue.then(async () => {
    try {
      instance.value = await api.projects.install(inst.id, h.project_id, h.project_type, dpWorld);
      toast.success(`${h.title} added`);
    } catch (e) {
      toast.error(`${h.title}: ${errorText(e)}`);
    } finally {
      const next = new Set(pending.value);
      next.delete(h.project_id);
      pending.value = next;
    }
  });
}

onMounted(async () => {
  const id = props.instanceId;
  if (id) {
    try {
      instance.value = await api.instances.get(id);
      if (type.value === "modpack" || !tabs.value.some((t) => t.type === type.value)) type.value = tabs.value[0]!.type;
      worlds.value = await api.instances.worlds(id);
      world.value = worlds.value[0] ?? "";
    } catch {
      router.replace("/library");
      return;
    }
  }
  void search();
  api.versions
    .game()
    .then((r) => (gameVersions.value = r.versions))
    .catch(() => {});
});

let timer: ReturnType<typeof setTimeout> | undefined;
watch(query, () => {
  clearTimeout(timer);
  timer = setTimeout(() => ((page.value = 1), search()), 300);
});
watch([type, sort, limit, gameVersion], () => {
  page.value = 1;
  void search();
});
watch(page, () => search());

async function search() {
  loading.value = true;
  try {
    const inst = instance.value;
    const loaders =
      inst && type.value === "mod" ? (inst.loader === "quilt" ? ["quilt", "fabric"] : inst.loader === "vanilla" ? [] : [inst.loader]) : undefined;
    const res = await api.projects.search({
      query: query.value,
      projectType: type.value,
      gameVersion: inst?.game_version ?? (gameVersion.value || undefined),
      loaders,
      sort: sort.value,
      limit: limit.value,
      offset: (page.value - 1) * limit.value,
    });
    hits.value = res.hits;
    total.value = res.total_hits;
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    loading.value = false;
  }
}

function side(h: ProjectHit) {
  const c = h.client_side !== "unsupported";
  const s = h.server_side !== "unsupported";
  if (c && s) return { icon: Globe, label: "Client and server" };
  if (s) return { icon: Server, label: "Server" };
  return { icon: Monitor, label: "Client" };
}

const LOADER_TAGS = new Set(Object.keys(LOADERS));
function tags(h: ProjectHit) {
  const cats = h.display_categories.length ? h.display_categories : h.categories;
  const loaders = cats.filter((c) => LOADER_TAGS.has(c));
  const rest = cats.filter((c) => !LOADER_TAGS.has(c));
  return { shown: [...rest.slice(0, 3), ...loaders.slice(0, 1)], more: Math.max(0, rest.length - 3 + loaders.length - 1) };
}

async function install(h: ProjectHit) {
  if (instance.value) return addNow(h);
  if (h.project_type !== "modpack") {
    installing.value = h;
    return;
  }
  packBusy.value = h.project_id;
  try {
    const inst = await api.projects.installModpack(h.project_id);
    toast.success(`${inst.name} is ready to play`);
    router.push(`/instance/${inst.id}`);
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    packBusy.value = null;
  }
}

function pageList(): (number | "…")[] {
  const n = pages.value, p = page.value;
  if (n <= 7) return Array.from({ length: n }, (_, i) => i + 1);
  const set = new Set([1, 2, p - 1, p, p + 1, n].filter((x) => x >= 1 && x <= n));
  const sorted = [...set].sort((a, b) => a - b);
  const out: (number | "…")[] = [];
  sorted.forEach((x, i) => {
    if (i && x - sorted[i - 1]! > 1) out.push("…");
    out.push(x);
  });
  return out;
}
</script>

<template>
  <div class="page enter">
    <template v-if="instanceId">
      <RouterLink :to="`/instance/${instanceId}`" class="row hint" style="margin-bottom: 14px; color: var(--tint); font-weight: 600; width: max-content">
        <ArrowLeft :size="14" /> {{ instance?.name ?? "Back" }}
      </RouterLink>
      <div class="page-header">
        <div v-if="instance" class="row" style="gap: 16px">
          <InstanceIcon :icon="instance.icon" :id="instance.id" :size="64" />
          <div>
            <h1 class="page-title">Add content</h1>
            <p class="page-sub">
              To <b>{{ instance.name }}</b> · {{ LOADERS[instance.loader].label }} {{ instance.game_version }}. Only what works with it is shown;
              Install adds it right away.
            </p>
          </div>
        </div>
      </div>
    </template>
    <div v-else class="page-header">
      <div>
        <h1 class="page-title">Discover</h1>
        <p class="page-sub">Modpacks, mods, resource packs, data packs and shaders from Modrinth.</p>
      </div>
    </div>

    <div v-slider class="tabs">
      <button v-for="t in tabs" :key="t.type" :class="{ active: type === t.type }" @click="goType(t.type)"><component :is="t.icon" />{{ t.label }}</button>
    </div>

    <div class="panel-host">
    <Transition :name="`panel-${panelDir}`">
    <div :key="type">
    <div class="input-icon" style="margin-bottom: 12px">
      <Search />
      <input v-model="query" class="input" :placeholder="`Search ${tabs.find((t) => t.type === type)?.label.toLowerCase()}…`" />
    </div>
    <div class="row" style="margin-bottom: 16px; flex-wrap: wrap">
      <label class="row hint">Sort by
        <select v-model="sort" class="select" style="width: 150px"><option v-for="s in SORTS" :key="s.id" :value="s.id">{{ s.label }}</option></select>
      </label>
      <div v-if="!instance" class="row hint" style="gap: 8px">Version
        <div style="width: 170px; position: relative"><VersionPicker v-model="gameVersion" :versions="gameVersions" clearable class="vp-floating" /></div>
      </div>
      <label class="row hint">View
        <select v-model.number="limit" class="select" style="width: 80px"><option :value="20">20</option><option :value="50">50</option><option :value="100">100</option></select>
      </label>
      <label v-if="instance && type === 'datapack'" class="row hint">World
        <select v-if="worlds.length" v-model="world" class="select" style="width: 180px"><option v-for="w in worlds" :key="w" :value="w">{{ w }}</option></select>
        <span v-else class="error-text">No worlds yet: play once and create one</span>
      </label>
      <span class="hint num"><Loader2 v-if="loading" class="spin" :size="12" /> {{ compact(total) }} results</span>
      <span class="spacer" />
      <div class="pager">
        <button :disabled="page === 1" aria-label="Previous page" @click="page--"><ChevronLeft :size="16" /></button>
        <template v-for="(p, i) in pageList()" :key="i">
          <span v-if="p === '…'" class="hint">…</span>
          <button v-else :class="{ on: p === page }" @click="page = p">{{ p }}</button>
        </template>
        <button :disabled="page >= pages" aria-label="Next page" @click="page++"><ChevronRight :size="16" /></button>
      </div>
    </div>

    <div class="stack">
      <div v-for="h in hits" :key="h.project_id" class="card row" style="gap: 16px; align-items: flex-start">
        <img v-if="h.icon_url" :src="h.icon_url" alt="" width="88" height="88" style="border-radius: 18px; flex-shrink: 0; object-fit: cover" />
        <span v-else class="tile" style="--type: #a2a9b4; width: 88px; height: 88px"><Package /></span>
        <div style="flex: 1; min-width: 0">
          <div class="row" style="flex-wrap: wrap; gap: 6px">
            <span style="font-weight: 800; font-size: 18px">{{ h.title }}</span>
            <span class="hint">by {{ h.author }}</span>
          </div>
          <p class="muted" style="margin-top: 4px; font-size: 13px; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden">{{ h.description }}</p>
          <div class="row" style="margin-top: 10px; flex-wrap: wrap; gap: 6px">
            <span class="chip"><component :is="side(h).icon" />{{ side(h).label }}</span>
            <span
              v-for="c in tags(h).shown"
              :key="c"
              class="chip"
              :style="LOADER_TAGS.has(c) ? { color: LOADERS[c as keyof typeof LOADERS].color } : {}"
            >{{ LOADER_TAGS.has(c) ? LOADERS[c as keyof typeof LOADERS].label : c.replace(/-/g, " ") }}</span>
            <span v-if="tags(h).more > 0" class="chip">+{{ tags(h).more }}</span>
          </div>
        </div>
        <div class="stack" style="align-items: flex-end; gap: 8px; flex-shrink: 0">
          <button v-if="instance && installed.has(h.project_id)" class="btn btn-installed" disabled><Check /> Installed</button>
          <button v-else-if="instance && pending.has(h.project_id)" class="btn btn-primary" disabled><Loader2 class="spin" /> Installing</button>
          <button v-else class="btn btn-primary" :disabled="packBusy !== null" @click="install(h)">
            <Loader2 v-if="packBusy === h.project_id" class="spin" /><Plus v-else /> Install
          </button>
          <span class="hint row num" style="gap: 10px">
            <span class="row" style="gap: 4px"><Download :size="13" />{{ compact(h.downloads) }}</span>
            <span class="row" style="gap: 4px"><Heart :size="13" />{{ compact(h.follows) }}</span>
          </span>
          <span v-if="h.date_modified" class="hint row" style="gap: 4px"><History :size="13" />{{ relative(h.date_modified) }}</span>
        </div>
      </div>
      <div v-if="!loading && !hits.length" class="card empty"><h3>Nothing found</h3><p>Try another search.</p></div>
    </div>
    </div>
    </Transition>
    </div>

    <InstallDialog
      v-if="installing"
      :hit="installing"
      @close="installing = null"
      @installed="installing = null"
    />
  </div>
</template>
