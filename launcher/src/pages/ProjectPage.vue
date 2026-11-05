<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ArrowLeft, Boxes, Bug, Check, ChevronDown, ChevronRight, Code, Download, ExternalLink, Globe, Heart, History, Image, Layers, Loader2,
  MessageCircle, Monitor, Package, Plus, Search, Server, BookOpen,
} from "lucide-vue-next";
import InstallDialog from "@/components/InstallDialog.vue";
import { api, type Instance, type PackContents, type ProjectDetail, type ProjectVersion } from "@/lib/api";
import { LOADERS, compact, relative } from "@/lib/format";
import { errorText, toast } from "@/lib/toast";
import { tabDirection, type PanelDirection } from "@/lib/tabSwitch";

/** A Modrinth project: its versions, and for modpacks what's inside. `instanceId`: opened from "Add content". */
const props = defineProps<{ id: string; instanceId?: string | null }>();
const router = useRouter();
/** ?local=1 (from a shared pack's extra content): installs into it stay on this computer. */
const local = useRoute().query.local === "1";

const project = ref<ProjectDetail | null>(null);
const versions = ref<ProjectVersion[]>([]);
const instance = ref<Instance | null>(null);
const isPack = computed(() => project.value?.project_type === "modpack");

const TABS = computed(() => [
  ...(isPack.value ? [{ id: "contents", label: "Mods", icon: Layers }] : []),
  { id: "versions", label: "Versions", icon: History },
  ...(project.value?.gallery.length ? [{ id: "gallery", label: "Gallery", icon: Image }] : []),
]);
const tab = ref("versions");
const panelDir = ref<PanelDirection>("none");
function goTab(to: string) {
  panelDir.value = tabDirection(TABS.value.map((t) => t.id), tab.value, to);
  tab.value = to;
}

onMounted(async () => {
  try {
    const [p, v] = await Promise.all([api.projects.detail(props.id), api.projects.versions(props.id)]);
    project.value = p;
    versions.value = v;
    if (p.project_type === "modpack") {
      tab.value = "contents";
      void loadContents((v.find((x) => x.version_type === "release") ?? v[0])?.id ?? null);
    }
  } catch (e) {
    toast.error(errorText(e));
    router.back();
    return;
  }
  if (props.instanceId) {
    try {
      const inst = await api.instances.get(props.instanceId);
      instance.value = inst;
      gameVersion.value = inst.game_version;
      if (project.value?.project_type === "mod" && inst.loader !== "quilt" && inst.loader !== "vanilla") loader.value = inst.loader;
    } catch {
      /* the instance is gone: browse without it */
    }
  }
});

// ── versions ──
const gameVersion = ref("");
const loader = ref("");
const releasesOnly = ref(false);
const shown = ref(40);
const open = ref(new Set<string>());
const allGameVersions = computed(() => {
  const seen = new Set<string>();
  for (const v of versions.value) for (const g of [...v.game_versions].reverse()) seen.add(g);
  return [...seen];
});
const allLoaders = computed(() => [...new Set(versions.value.flatMap((v) => v.loaders))]);
const filtered = computed(() =>
  versions.value.filter(
    (v) =>
      (!gameVersion.value || v.game_versions.includes(gameVersion.value)) &&
      (!loader.value || v.loaders.includes(loader.value)) &&
      (!releasesOnly.value || v.version_type === "release"),
  ),
);
watch([gameVersion, loader, releasesOnly], () => (shown.value = 40));
function toggleOpen(id: string) {
  const next = new Set(open.value);
  if (!next.delete(id)) next.add(id);
  open.value = next;
}
/** "1.20.1 – 1.20.4" for a long run of Minecraft versions. */
function range(list: string[]) {
  if (list.length <= 3) return list.join(", ");
  return `${list[0]} – ${list[list.length - 1]}`;
}
const projectLoaders = computed(() => (project.value?.loaders ?? []).filter((l): l is keyof typeof LOADERS => Object.hasOwn(LOADERS, l)));
function loaderLabel(l: string) {
  return LOADERS[l as keyof typeof LOADERS]?.label ?? l.replace(/-/g, " ");
}
/** The version of this project the instance has, if any. */
const installedVersion = computed(() => instance.value?.mods.find((m) => m.project_id === project.value?.id)?.version_id ?? null);

// ── installing ──
const installing = ref<{ version: ProjectVersion | null } | null>(null);
const packBusy = ref<string | null>(null);
const asHit = computed(() => {
  const p = project.value!;
  return { project_id: p.id, project_type: p.project_type, title: p.title, versions: p.game_versions, categories: [...p.categories, ...p.loaders] };
});

async function install(version: ProjectVersion | null) {
  const p = project.value;
  if (!p) return;
  if (p.project_type !== "modpack") {
    installing.value = { version };
    return;
  }
  packBusy.value = version?.id ?? "latest";
  try {
    const inst = await api.projects.installModpack(p.id, version?.id ?? null);
    toast.success(`${inst.name} is ready to play`);
    router.push(`/instance/${inst.id}`);
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    packBusy.value = null;
  }
}

async function installed(inst: Instance) {
  installing.value = null;
  if (instance.value && inst.id === instance.value.id) instance.value = inst;
}

// ── modpack contents ──
const contents = ref<PackContents | null>(null);
const contentsVersion = ref<string | null>(null);
const contentsLoading = ref(false);
const contentsQuery = ref("");
const contentsKind = ref<"all" | "mod" | "resourcepack" | "shader">("all");
async function loadContents(versionId: string | null) {
  if (!project.value) return;
  contentsVersion.value = versionId;
  contentsLoading.value = true;
  try {
    contents.value = await api.projects.modpackContents(project.value.id, versionId);
  } catch (e) {
    contents.value = null;
    toast.error(errorText(e));
  } finally {
    contentsLoading.value = false;
  }
}
function viewContents(v: ProjectVersion) {
  goTab("contents");
  void loadContents(v.id);
}
const contentFiles = computed(() => {
  const q = contentsQuery.value.trim().toLowerCase();
  return (contents.value?.files ?? []).filter(
    (f) => f.kind !== "other" && (contentsKind.value === "all" || f.kind === contentsKind.value) && (!q || f.title.toLowerCase().includes(q)),
  );
});
const countKind = (k: string) => (contents.value?.files ?? []).filter((f) => k === "all" ? f.kind !== "other" : f.kind === k).length;
const CONTENT_KINDS = [
  { id: "all", label: "All" },
  { id: "mod", label: "Mods" },
  { id: "resourcepack", label: "Resource packs" },
  { id: "shader", label: "Shaders" },
] as const;

function side(client: string | null | undefined, server: string | null | undefined) {
  const c = client !== "unsupported";
  const s = server !== "unsupported";
  if (c && s) return { icon: Globe, label: "Client and server" };
  if (s) return { icon: Server, label: "Server" };
  return { icon: Monitor, label: "Client" };
}

const links = computed(() => {
  const p = project.value;
  if (!p) return [];
  return [
    { url: `https://modrinth.com/${p.project_type}/${p.slug}`, label: "Modrinth", icon: ExternalLink },
    { url: p.source_url, label: "Source", icon: Code },
    { url: p.issues_url, label: "Issues", icon: Bug },
    { url: p.wiki_url, label: "Wiki", icon: BookOpen },
    { url: p.discord_url, label: "Discord", icon: MessageCircle },
  ].filter((l): l is { url: string; label: string; icon: typeof ExternalLink } => !!l.url && l.url.startsWith("https://"));
});
</script>

<template>
  <div class="page enter">
    <button class="row hint back-link" @click="router.back()"><ArrowLeft :size="14" /> {{ instance ? instance.name : "Discover" }}</button>

    <div v-if="!project" class="card empty"><Loader2 class="spin" /><p>Loading…</p></div>
    <template v-else>
      <div class="card row" style="gap: 20px; align-items: flex-start; margin-bottom: 20px">
        <img v-if="project.icon_url" :src="project.icon_url" alt="" width="104" height="104" style="border-radius: 22px; flex-shrink: 0; object-fit: cover" />
        <span v-else class="tile" style="--type: #a2a9b4; width: 104px; height: 104px"><Package /></span>
        <div style="flex: 1; min-width: 0">
          <div class="row" style="flex-wrap: wrap; gap: 8px">
            <h1 class="page-title" style="margin: 0">{{ project.title }}</h1>
            <span v-if="project.author" class="hint">by {{ project.author }}</span>
          </div>
          <p class="muted" style="margin-top: 6px">{{ project.description }}</p>
          <div class="row" style="margin-top: 12px; flex-wrap: wrap; gap: 6px">
            <span class="chip"><component :is="side(project.client_side, project.server_side).icon" />{{ side(project.client_side, project.server_side).label }}</span>
            <span v-for="l in projectLoaders" :key="l" class="chip" :style="{ color: LOADERS[l].color }">{{ LOADERS[l].label }}</span>
            <span v-for="c in project.categories.slice(0, 4)" :key="c" class="chip">{{ c.replace(/-/g, " ") }}</span>
          </div>
          <div class="row hint num" style="margin-top: 12px; gap: 14px; flex-wrap: wrap">
            <span class="row" style="gap: 4px"><Download :size="13" />{{ compact(project.downloads) }} downloads</span>
            <span class="row" style="gap: 4px"><Heart :size="13" />{{ compact(project.followers) }}</span>
            <span v-if="project.updated" class="row" style="gap: 4px"><History :size="13" />Updated {{ relative(project.updated) }}</span>
            <span v-if="versions.length" class="row" style="gap: 4px"><Boxes :size="13" />{{ range(allGameVersions.slice().reverse()) }}</span>
          </div>
          <div class="row" style="margin-top: 12px; gap: 6px; flex-wrap: wrap">
            <button v-for="l in links" :key="l.label" class="btn btn-ghost btn-sm" @click="openUrl(l.url)"><component :is="l.icon" />{{ l.label }}</button>
          </div>
        </div>
        <button class="btn btn-primary btn-lg" style="flex-shrink: 0" :disabled="packBusy !== null || !versions.length" @click="install(null)">
          <Loader2 v-if="packBusy === 'latest'" class="spin" /><Plus v-else /> Install
        </button>
      </div>

      <div v-slider class="tabs">
        <button v-for="t in TABS" :key="t.id" :class="{ active: tab === t.id }" @click="goTab(t.id)"><component :is="t.icon" />{{ t.label }}</button>
      </div>

      <div class="panel-host">
      <Transition :name="`panel-${panelDir}`">
      <section v-if="tab === 'versions'" key="versions" class="stack">
        <div class="row" style="flex-wrap: wrap">
          <label class="row hint">Minecraft
            <select v-model="gameVersion" class="select" style="width: 130px">
              <option value="">Any</option>
              <option v-for="g in allGameVersions" :key="g" :value="g">{{ g }}</option>
            </select>
          </label>
          <label v-if="allLoaders.length > 1" class="row hint">Loader
            <select v-model="loader" class="select" style="width: 130px">
              <option value="">Any</option>
              <option v-for="l in allLoaders" :key="l" :value="l">{{ loaderLabel(l) }}</option>
            </select>
          </label>
          <label class="row hint" style="gap: 8px">
            <span class="switch"><input v-model="releasesOnly" type="checkbox" aria-label="Releases only" /><span /></span> Releases only
          </label>
          <span class="spacer" />
          <span class="hint num">{{ filtered.length }} of {{ versions.length }} versions</span>
        </div>
        <div v-if="!filtered.length" class="card empty"><h3>No versions</h3><p>None match these filters.</p></div>
        <div v-for="v in filtered.slice(0, shown)" :key="v.id" class="card version">
          <div class="row" style="gap: 12px">
            <button class="btn btn-ghost btn-icon" :aria-label="open.has(v.id) ? 'Hide changes' : 'Show changes'" :title="open.has(v.id) ? 'Hide changes' : 'Show changes'" @click="toggleOpen(v.id)">
              <ChevronDown v-if="open.has(v.id)" /><ChevronRight v-else />
            </button>
            <div style="flex: 1; min-width: 0">
              <div class="row" style="gap: 8px; flex-wrap: wrap">
                <span style="font-weight: 700">{{ v.name || v.version_number }}</span>
                <span class="chip" :class="`vt-${v.version_type}`">{{ v.version_type }}</span>
                <span v-if="installedVersion === v.id" class="chip vt-release"><Check :size="12" /> Installed</span>
              </div>
              <div class="hint" style="margin-top: 2px">
                <span class="mono">{{ v.version_number }}</span> · {{ v.loaders.map(loaderLabel).join(", ") }} · Minecraft {{ range(v.game_versions) }}
              </div>
            </div>
            <span class="hint num row" style="gap: 10px; flex-shrink: 0">
              <span class="row" style="gap: 4px"><Download :size="13" />{{ compact(v.downloads) }}</span>
              <span>{{ relative(v.date_published) }}</span>
            </span>
            <button v-if="isPack" class="btn btn-sm" @click="viewContents(v)"><Layers /> Mods</button>
            <button class="btn btn-sm btn-primary" :disabled="packBusy !== null || installedVersion === v.id" @click="install(v)">
              <Loader2 v-if="packBusy === v.id" class="spin" /><Download v-else /> {{ installedVersion && installedVersion !== v.id ? "Switch" : "Install" }}
            </button>
          </div>
          <div v-if="open.has(v.id)" class="changelog">{{ v.changelog?.trim() || "No changes listed for this version." }}</div>
        </div>
        <button v-if="filtered.length > shown" class="btn" style="align-self: center" @click="shown += 40">Show more</button>
      </section>

      <section v-else-if="tab === 'contents'" key="contents" class="stack">
        <div class="row" style="flex-wrap: wrap">
          <label class="row hint">Version
            <select class="select" style="width: 220px" :value="contentsVersion ?? versions[0]?.id" @change="loadContents(($event.target as HTMLSelectElement).value)">
              <option v-for="v in versions" :key="v.id" :value="v.id">{{ v.name || v.version_number }}</option>
            </select>
          </label>
          <div class="choices">
            <button v-for="k in CONTENT_KINDS" :key="k.id" class="choice" style="height: 30px; padding: 0 10px" :class="{ on: contentsKind === k.id }" @click="contentsKind = k.id">
              {{ k.label }}<span v-if="countKind(k.id)" class="choice-count num">{{ countKind(k.id) }}</span>
            </button>
          </div>
          <span class="spacer" />
          <div class="input-icon" style="width: 220px">
            <Search />
            <input v-model="contentsQuery" class="input" placeholder="Search this pack…" aria-label="Search this pack" />
          </div>
        </div>
        <div v-if="contentsLoading" class="card empty"><Loader2 class="spin" /><p>Looking inside the modpack…</p></div>
        <template v-else-if="contents">
          <p class="hint" style="margin: 0">
            {{ LOADERS[contents.loader].label }}{{ contents.loader_version ? ` ${contents.loader_version}` : "" }} · Minecraft {{ contents.game_version }}
            <template v-if="contents.other_files"> · plus {{ contents.other_files }} config and other files</template>
          </p>
          <div class="card stack" style="gap: 8px">
            <component
              :is="f.project_id ? 'button' : 'div'"
              v-for="f in contentFiles"
              :key="f.path"
              class="inset row pack-file"
              :class="{ link: f.project_id }"
              style="gap: 12px"
              @click="f.project_id && router.push(`/project/${f.project_id}`)"
            >
              <img v-if="f.icon_url" :src="f.icon_url" alt="" width="36" height="36" style="border-radius: 9px" />
              <span v-else class="tile tile-sm" style="--type: #a2a9b4"><Package /></span>
              <div style="flex: 1; min-width: 0; text-align: left">
                <div style="font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap">{{ f.title }}</div>
                <div class="hint mono" style="overflow: hidden; text-overflow: ellipsis; white-space: nowrap">{{ f.version ?? f.path }}</div>
              </div>
              <span v-if="!f.client" class="chip"><Server />Server only</span>
              <span v-if="f.bundled" class="chip" title="Inside the modpack file, not from Modrinth">In the pack</span>
              <span class="chip">{{ CONTENT_KINDS.find((k) => k.id === f.kind)?.label.replace(/s$/, "") }}</span>
              <ChevronRight v-if="f.project_id" :size="16" class="hint" />
            </component>
            <p v-if="!contentFiles.length" class="hint">Nothing here.</p>
          </div>
        </template>
      </section>

      <section v-else key="gallery" class="gallery">
        <figure v-for="g in project.gallery" :key="g.url">
          <img :src="g.url" :alt="g.title ?? ''" loading="lazy" />
          <figcaption v-if="g.title" class="hint">{{ g.title }}</figcaption>
        </figure>
      </section>
      </Transition>
      </div>
    </template>

    <InstallDialog
      v-if="installing && project"
      :hit="asHit"
      :version="installing.version"
      :preselect="instance?.id"
      :local="local"
      @close="installing = null"
      @installed="installed"
    />
  </div>
</template>

<style scoped>
.back-link {
  margin-bottom: 14px;
  color: var(--tint);
  font-weight: 600;
  width: max-content;
  background: none;
  border: 0;
  padding: 0;
  cursor: pointer;
}
.version {
  padding: 12px 14px;
}
.changelog {
  margin: 10px 0 0 44px;
  white-space: pre-wrap;
  font-size: 13px;
  max-height: 320px;
  overflow: auto;
  color: var(--muted-foreground);
}
.vt-release {
  color: #7dd87d;
}
.vt-beta {
  color: #e8c35a;
}
.vt-alpha {
  color: #f08a7a;
}
.pack-file {
  width: 100%;
  border: 0;
  font: inherit;
  color: inherit;
}
.pack-file.link {
  cursor: pointer;
}
.pack-file.link:hover {
  filter: brightness(1.08);
}
.choice-count {
  margin-left: 6px;
  opacity: 0.6;
  font-size: 12px;
}
.gallery {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 14px;
}
.gallery figure {
  margin: 0;
}
.gallery img {
  width: 100%;
  border-radius: 14px;
  display: block;
  aspect-ratio: 16 / 9;
  object-fit: cover;
}
.gallery figcaption {
  margin-top: 6px;
}
</style>
