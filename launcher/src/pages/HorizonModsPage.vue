<script setup lang="ts">
/** "Add Horizon mods" for a Horizon instance: Horizon's own registry (github.com/horizon-rust/horizon-mods). */
import { computed, onMounted, ref, watch } from "vue";
import { RouterLink } from "vue-router";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowLeft, Check, Cpu, Download, ExternalLink, Image, Loader2, Package, Plus, RefreshCw, Search, ShieldAlert, Sun, Puzzle } from "lucide-vue-next";
import AppDialog from "@/components/AppDialog.vue";
import InstanceIcon from "@/components/InstanceIcon.vue";
import { api, type HorizonKind, type HorizonProject, type Instance } from "@/lib/api";
import { compact, horizonBuild } from "@/lib/format";
import { errorText, toast } from "@/lib/toast";

const props = defineProps<{ instanceId: string }>();

const TABS = [
  { kind: "native-mod", label: "Native mods", icon: Cpu },
  { kind: "mod", label: "Content mods", icon: Puzzle },
  { kind: "resourcepack", label: "Resource packs", icon: Image },
  { kind: "shader", label: "Shaders", icon: Sun },
] as const satisfies readonly { kind: HorizonKind; label: string; icon: unknown }[];

/** The content kind an installed project of a registry kind has. */
const CONTENT: Record<HorizonKind, string> = { mod: "horizon-mod", "native-mod": "native-mod", resourcepack: "resourcepack", shader: "shader" };

const kind = ref<HorizonKind>("native-mod");
const query = ref("");
const all = ref<HorizonProject[]>([]);
const loading = ref(true);
const error = ref("");
const instance = ref<Instance | null>(null);
const pending = ref(new Set<string>());
const updates = ref(new Map<string, string>());
/** A native mod waiting for the "full access" answer. */
const trusting = ref<HorizonProject | null>(null);
const TRUSTED = "tessera.horizon.native-trusted";

const shown = computed(() => {
  const q = query.value.trim().toLowerCase();
  return all.value.filter((p) => p.kind === kind.value && (!q || `${p.name} ${p.id} ${p.summary} ${p.authors.join(" ")}`.toLowerCase().includes(q)));
});
const installed = computed(() => {
  const m = new Map<string, string>();
  for (const i of instance.value?.mods ?? []) m.set(`${i.kind}:${i.project_id}`, i.version);
  return m;
});
const installedVersion = (p: HorizonProject) => installed.value.get(`${CONTENT[p.kind]}:${p.id}`);
const countOf = (k: HorizonKind) => all.value.filter((p) => p.kind === k).length;

async function load() {
  loading.value = true;
  error.value = "";
  try {
    const [inst, list] = await Promise.all([api.instances.get(props.instanceId), api.horizon.mods("")]);
    instance.value = inst;
    all.value = list;
    updates.value = new Map(await api.horizon.updates(props.instanceId).catch(() => []));
  } catch (e) {
    error.value = errorText(e);
  } finally {
    loading.value = false;
  }
}
onMounted(load);
watch(kind, () => (query.value = ""));

function install(p: HorizonProject) {
  if (p.kind === "native-mod" && !localStorage.getItem(TRUSTED)) {
    trusting.value = p;
    return;
  }
  void doInstall(p);
}

async function trustAndInstall() {
  const p = trusting.value;
  trusting.value = null;
  if (!p) return;
  localStorage.setItem(TRUSTED, "1");
  await doInstall(p);
}

async function doInstall(p: HorizonProject) {
  pending.value = new Set(pending.value).add(p.id);
  try {
    instance.value = await api.horizon.install(props.instanceId, p.id);
    const u = new Map(updates.value);
    u.delete(p.id);
    updates.value = u;
    toast.success(p.kind === "native-mod" ? `${p.name} is installed: it's compiled into the game the next time you press Play` : `${p.name} is installed`);
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    const s = new Set(pending.value);
    s.delete(p.id);
    pending.value = s;
  }
}

const byline = (p: HorizonProject) => (p.authors.length ? `by ${p.authors.join(", ")}` : "");
const newest = (p: HorizonProject) => p.versions[0];
</script>

<template>
  <div class="page enter">
    <RouterLink :to="`/instance/${instanceId}`" class="row hint" style="margin-bottom: 14px; color: var(--tint); font-weight: 600; width: max-content">
      <ArrowLeft :size="14" /> {{ instance?.name ?? "Back" }}
    </RouterLink>
    <div class="page-header">
      <div class="row" style="gap: 16px">
        <InstanceIcon v-if="instance" :icon="instance.icon" :id="instance.id" :size="64" />
        <div>
          <h1 class="page-title">Add Horizon mods</h1>
          <p class="page-sub">
            To <b>{{ instance?.name }}</b> · Horizon {{ horizonBuild(instance?.loader_version ?? null) }}. From
            <a href="#" @click.prevent="openUrl('https://github.com/horizon-rust/horizon-mods')">Horizon's mod registry</a>, where each project is reviewed
            when it's listed. Java mods (Fabric, Forge) don't run in Horizon.
          </p>
        </div>
      </div>
      <button class="btn" :disabled="loading" @click="load"><RefreshCw :class="{ spin: loading }" /> Refresh</button>
    </div>

    <div v-slider class="tabs">
      <button v-for="t in TABS" :key="t.kind" :class="{ active: kind === t.kind }" @click="kind = t.kind">
        <component :is="t.icon" />{{ t.label }}<span v-if="countOf(t.kind)" class="hint num" style="margin-left: 6px">{{ countOf(t.kind) }}</span>
      </button>
    </div>

    <p v-if="kind === 'native-mod'" class="card hint row native-note">
      <ShieldAlert :size="18" />
      <span>
        Native mods change the game's own Rust code, so Tessera compiles Horizon with them when you press Play. The first build downloads Rust
        and takes several minutes; after that a change only takes about a minute. They run with full access to this computer.
      </span>
    </p>

    <div class="input-icon" style="margin-bottom: 16px">
      <Search />
      <input v-model="query" class="input" :placeholder="`Search ${TABS.find((t) => t.kind === kind)?.label.toLowerCase()}…`" aria-label="Search Horizon's mods" />
    </div>

    <p v-if="error" class="error-text">{{ error }}</p>
    <div class="stack">
      <div v-for="p in shown" :key="p.id" class="card row" style="gap: 16px; align-items: flex-start">
        <img v-if="p.icon" :src="p.icon" alt="" width="72" height="72" style="border-radius: 16px; flex-shrink: 0; object-fit: cover" />
        <span v-else class="tile" style="--type: #f0795a; width: 72px; height: 72px"><Package /></span>
        <div style="flex: 1; min-width: 0">
          <div class="row" style="flex-wrap: wrap; gap: 6px">
            <span style="font-weight: 800; font-size: 17px">{{ p.name }}</span>
            <span class="hint">{{ byline(p) }}</span>
          </div>
          <p class="muted" style="margin-top: 4px; font-size: 13px">{{ p.summary }}</p>
          <div class="row" style="margin-top: 10px; flex-wrap: wrap; gap: 6px">
            <span class="chip mono">{{ newest(p)?.version }}</span>
            <span v-if="newest(p)?.side" class="chip">{{ newest(p)?.side === "both" ? "Client and server" : newest(p)?.side === "client" ? "Client" : "Server" }}</span>
            <span v-if="p.license" class="chip">{{ p.license }}</span>
            <span v-if="!p.key" class="chip" title="Its author doesn't sign its files; the registry still pins each file's hash">Unsigned</span>
          </div>
        </div>
        <div class="stack" style="align-items: flex-end; gap: 8px; flex-shrink: 0">
          <button v-if="pending.has(p.id)" class="btn btn-primary" disabled><Loader2 class="spin" /> Installing</button>
          <button v-else-if="installedVersion(p) && updates.has(p.id)" class="btn btn-primary" @click="install(p)"><Download /> Update to {{ updates.get(p.id) }}</button>
          <button v-else-if="installedVersion(p)" class="btn btn-installed" disabled><Check /> Installed</button>
          <button v-else class="btn btn-primary" @click="install(p)"><Plus /> Install</button>
          <button v-if="p.source" class="btn btn-sm btn-ghost" @click="openUrl(p.source)"><ExternalLink /> Source</button>
          <span v-if="p.downloads" class="hint row num" style="gap: 4px"><Download :size="13" />{{ compact(p.downloads) }}</span>
        </div>
      </div>
      <div v-if="loading && !all.length" class="card empty"><Loader2 class="spin" /><p>Reading Horizon's registry…</p></div>
      <div v-else-if="!shown.length" class="card empty"><h3>Nothing found</h3><p>Try another search or kind.</p></div>
    </div>

    <AppDialog v-if="trusting" title="Native mods run with full access" @close="trusting = null">
      <p class="muted">
        A native mod is Rust code compiled into the game. It isn't sandboxed: it can do anything a program on this computer can. Horizon's registry
        has someone read each project's source when it's first listed; later versions are only checked by machine. Install native mods you trust.
      </p>
      <p class="muted" style="margin-top: 10px">
        The next time you press Play, Tessera downloads Rust (about 450 MB, once) and Horizon's source, and compiles the game with your native mods.
      </p>
      <div class="dialog-footer">
        <button class="btn" @click="trusting = null">Cancel</button>
        <button class="btn btn-primary" @click="trustAndInstall">I trust it: install {{ trusting.name }}</button>
      </div>
    </AppDialog>
  </div>
</template>

<style scoped>
.native-note {
  gap: 10px;
  align-items: flex-start;
  margin: 0 0 14px;
}
.native-note svg {
  flex-shrink: 0;
  color: var(--tint);
  margin-top: 1px;
}
</style>
