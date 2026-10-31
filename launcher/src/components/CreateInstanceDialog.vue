<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ArrowLeft, Boxes, Check, Compass, Download, FolderSearch, Loader2, Palette, PackageOpen, Plus, RefreshCw, Search, Upload,
} from "lucide-vue-next";
import { api, type FoundInstance, type GameVersion, type Icon, type LauncherDir, type Loader, type LoaderVersion } from "@/lib/api";
import { LOADERS, LOADER_ORDER } from "@/lib/format";
import { randomIcon } from "@/lib/icons";
import { errorText, toast } from "@/lib/toast";
import AppDialog from "./AppDialog.vue";
import IconEditor from "./IconEditor.vue";
import InstanceIcon from "./InstanceIcon.vue";
import VersionPicker from "./VersionPicker.vue";

const emit = defineEmits<{ close: [] }>();
const router = useRouter();

type Step = "choose" | "custom" | "import";
const step = ref<Step>("choose");
const query = ref("");
const busy = ref(false);
const error = ref("");

// ── custom setup ──
const icon = ref<Icon>(randomIcon());
const iconFile = ref<string | null>(null);
const iconPreview = ref<string | null>(null);
const editingIcon = ref(false);
const name = ref("");
const loader = ref<Loader>("fabric");
const gameVersion = ref("");
const versions = ref<GameVersion[]>([]);
const loaderVersions = ref<LoaderVersion[] | null>(null);
const loaderPick = ref<"stable" | "latest" | "other">("stable");
const otherVersion = ref("");

const placeholder = computed(() => `${LOADERS[loader.value].label} ${gameVersion.value}`);
const chosenLoaderVersion = computed(() => {
  const list = loaderVersions.value ?? [];
  if (loader.value === "vanilla") return null;
  if (loaderPick.value === "other") return otherVersion.value || null;
  if (loaderPick.value === "latest") return list[0]?.version ?? null;
  return (list.find((l) => l.stable) ?? list[0])?.version ?? null;
});
const unsupported = computed(() => loader.value !== "vanilla" && loaderVersions.value !== null && loaderVersions.value.length === 0);

onMounted(async () => {
  try {
    const res = await api.versions.game();
    versions.value = res.versions;
    gameVersion.value = res.latest;
  } catch (e) {
    error.value = errorText(e);
  }
});

let token = 0;
watch([loader, gameVersion], async () => {
  loaderVersions.value = null;
  otherVersion.value = "";
  if (loader.value === "vanilla" || !gameVersion.value) return;
  const mine = ++token;
  try {
    const list = await api.loaders.versions(loader.value, gameVersion.value);
    if (mine === token) {
      loaderVersions.value = list;
      otherVersion.value = list[0]?.version ?? "";
    }
  } catch {
    if (mine === token) loaderVersions.value = [];
  }
});

function onIcon(i: Icon, file: string | null, preview: string | null) {
  icon.value = i;
  iconFile.value = file;
  iconPreview.value = preview;
  editingIcon.value = false;
}

async function uploadIcon() {
  const picked = await open({ multiple: false, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }] });
  if (typeof picked !== "string") return;
  iconFile.value = picked;
  iconPreview.value = await api.instances.imagePreview(picked);
  icon.value = { kind: "file" };
}

function randomize() {
  icon.value = randomIcon();
  iconFile.value = iconPreview.value = null;
}

async function create() {
  busy.value = true;
  error.value = "";
  try {
    const inst = await api.instances.create({
      name: name.value.trim() || placeholder.value,
      gameVersion: gameVersion.value,
      loader: loader.value,
      loaderVersion: chosenLoaderVersion.value,
      icon: iconFile.value ? null : icon.value,
      iconFile: iconFile.value,
    });
    emit("close");
    router.push(`/instance/${inst.id}`);
  } catch (e) {
    error.value = errorText(e);
  } finally {
    busy.value = false;
  }
}

// ── search / modpacks ──
function search() {
  emit("close");
  router.push({ path: "/discover", query: { type: "modpack", q: query.value || undefined } });
}

async function uploadPack() {
  const picked = await open({ multiple: false, filters: [{ name: "Modpack", extensions: ["tapack", "mrpack"] }] });
  if (typeof picked !== "string") return;
  emit("close");
  try {
    const inst = await api.projects.importPack(picked);
    toast.success(`${inst.name} is ready`);
    router.push(`/instance/${inst.id}`);
  } catch (e) {
    toast.error(errorText(e));
  }
}

// ── invite links ──
const inviteLink = ref("");

async function joinLink() {
  busy.value = true;
  error.value = "";
  try {
    const inst = await api.share.join(inviteLink.value);
    toast.success(`${inst.name} is ready`);
    emit("close");
    router.push(`/instance/${inst.id}`);
  } catch (e) {
    error.value = errorText(e);
  } finally {
    busy.value = false;
  }
}

// ── import from other launchers ──
const launchers = ref<LauncherDir[] | null>(null);
const selected = ref(new Set<string>());
const addingPath = ref(false);
const customPath = ref("");

async function openImport() {
  step.value = "import";
  launchers.value = await api.imports.detect();
}

async function browseLauncher() {
  const picked = await open({ directory: true, multiple: false });
  if (typeof picked === "string") customPath.value = picked;
}

async function addLauncher() {
  if (!customPath.value.trim()) return;
  const dir = await api.imports.scan(customPath.value.trim());
  if (!dir.instances.length) {
    error.value = "No Prism, MultiMC or CurseForge instances found there.";
    return;
  }
  error.value = "";
  launchers.value = [...(launchers.value ?? []).filter((l) => l.path !== dir.path), dir];
  customPath.value = "";
  addingPath.value = false;
}

function toggle(f: FoundInstance) {
  const s = new Set(selected.value);
  if (s.has(f.path)) s.delete(f.path);
  else s.add(f.path);
  selected.value = s;
}

async function runImport() {
  const found = (launchers.value ?? []).flatMap((l) => l.instances).filter((f) => selected.value.has(f.path));
  busy.value = true;
  error.value = "";
  try {
    const done = await api.imports.run(found);
    toast.success(`Imported ${done.length} instance${done.length === 1 ? "" : "s"}`);
    emit("close");
    router.push(done.length === 1 ? `/instance/${done[0]!.id}` : "/library");
  } catch (e) {
    error.value = errorText(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <AppDialog title="Create instance" :width="560" @close="emit('close')">
    <!-- choose -->
    <div v-if="step === 'choose'" class="stack" style="gap: 16px">
      <span class="label" style="font-size: 15px; font-weight: 700">Already know what you want to play?</span>
      <form class="input-icon" @submit.prevent="search">
        <Search />
        <input v-model="query" class="input" placeholder="Search modpacks, mods and more…" aria-label="Search modpacks, mods and more" autofocus />
      </form>
      <div class="divider-or">or</div>
      <span class="label" style="font-size: 15px; font-weight: 700">Choose how to start</span>
      <button class="option-row" @click="step = 'custom'">
        <span class="icon"><Boxes /></span>
        <span><h3>Custom setup</h3><span class="hint">Start from scratch by picking a loader and game version.</span></span>
      </button>
      <button class="option-row" @click="search">
        <span class="icon"><Compass /></span>
        <span><h3>Start from a modpack</h3><span class="hint">Pick a modpack and we'll install its newest version.</span></span>
      </button>
      <button class="option-row" @click="uploadPack">
        <span class="icon"><Upload /></span>
        <span><h3>Upload a modpack</h3><span class="hint">Install a Tessera pack (.tapack) or Modrinth pack (.mrpack) from this computer.</span></span>
      </button>
      <button class="option-row" @click="openImport">
        <span class="icon"><PackageOpen /></span>
        <span><h3>Import instance</h3><span class="hint">Bring over instances from Prism Launcher, MultiMC or CurseForge.</span></span>
      </button>
    </div>

    <!-- custom -->
    <form v-else-if="step === 'custom'" class="stack" style="gap: 18px" @submit.prevent="create">
      <div class="row" style="gap: 16px; align-items: stretch">
        <InstanceIcon :icon="icon" :src="iconPreview" :size="120" />
        <div class="stack" style="gap: 8px; justify-content: center">
          <button type="button" class="btn" @click="uploadIcon"><Upload /> Upload</button>
          <button type="button" class="btn" @click="randomize"><RefreshCw /> Randomize</button>
          <button type="button" class="btn" @click="editingIcon = true"><Palette /> Customize</button>
        </div>
      </div>
      <div class="field">
        <label for="ci-name">Name</label>
        <input id="ci-name" v-model="name" class="input" :placeholder="placeholder" />
      </div>
      <div class="field">
        <span class="label">Loader</span>
        <div class="choices">
          <button v-for="l in LOADER_ORDER" :key="l" type="button" class="choice" :class="{ on: loader === l }" @click="loader = l">
            <Check v-if="loader === l" />{{ LOADERS[l].label }}
          </button>
        </div>
      </div>
      <div class="field">
        <span class="label">Game version</span>
        <VersionPicker v-model="gameVersion" :versions="versions" />
      </div>
      <div v-if="loader !== 'vanilla'" class="field">
        <span class="label">Loader version</span>
        <p v-if="loaderVersions === null" class="hint row"><Loader2 class="spin" :size="13" /> Looking up {{ LOADERS[loader].label }} versions…</p>
        <p v-else-if="unsupported" class="error-text">{{ LOADERS[loader].label }} has no version for Minecraft {{ gameVersion }} yet.</p>
        <template v-else>
          <div class="choices">
            <button v-for="k in ['stable', 'latest', 'other'] as const" :key="k" type="button" class="choice" :class="{ on: loaderPick === k }" @click="loaderPick = k">
              <Check v-if="loaderPick === k" />{{ k[0]!.toUpperCase() + k.slice(1) }}
            </button>
          </div>
          <select v-if="loaderPick === 'other'" v-model="otherVersion" class="select">
            <option v-for="v in loaderVersions" :key="v.version" :value="v.version">{{ v.version }}{{ v.stable ? "" : " (beta)" }}</option>
          </select>
          <span v-else class="hint mono">{{ chosenLoaderVersion }}</span>
        </template>
      </div>
      <p v-if="error" class="error-text">{{ error }}</p>
      <div class="dialog-footer" style="margin-top: 0">
        <button type="button" class="btn btn-lg" @click="step = 'choose'"><ArrowLeft /> Back</button>
        <button type="submit" class="btn btn-primary btn-lg" :disabled="busy || !gameVersion || unsupported || (loader !== 'vanilla' && !chosenLoaderVersion)">
          <Loader2 v-if="busy" class="spin" /><Plus v-else /> Create instance
        </button>
      </div>
    </form>

    <!-- import -->
    <div v-else class="stack" style="gap: 14px">
      <span class="label" style="font-size: 15px; font-weight: 700">Launcher instances</span>
      <p v-if="launchers === null" class="hint row"><Loader2 class="spin" :size="13" /> Looking for other launchers…</p>
      <p v-else-if="!launchers.length" class="hint">No Prism Launcher, MultiMC or CurseForge instances found in the usual places. Add the launcher's folder below.</p>
      <div v-for="l in launchers ?? []" :key="l.path" class="inset stack" style="gap: 6px">
        <div class="row"><span style="font-weight: 700">{{ l.launcher }}</span><span class="hint mono" style="overflow: hidden; text-overflow: ellipsis; white-space: nowrap">{{ l.path }}</span></div>
        <label v-for="f in l.instances" :key="f.path" class="row" style="cursor: pointer; padding: 4px 0">
          <input type="checkbox" :checked="selected.has(f.path)" @change="toggle(f)" />
          <span style="flex: 1">{{ f.name }}</span>
          <span class="chip" :style="{ color: LOADERS[f.loader].color }">{{ LOADERS[f.loader].label }}</span>
          <span class="chip num">{{ f.game_version }}</span>
        </label>
      </div>
      <div v-if="addingPath" class="row">
        <button type="button" class="btn btn-icon" title="Browse" aria-label="Browse" @click="browseLauncher"><FolderSearch /></button>
        <input v-model="customPath" class="input mono" placeholder="Path to launcher…" aria-label="Path to launcher" @keydown.enter.prevent="addLauncher" />
        <button type="button" class="btn" :disabled="!customPath.trim()" @click="addLauncher">Add</button>
      </div>
      <button v-else type="button" class="btn btn-lg" @click="addingPath = true">Add launcher path</button>
      <div class="divider-or">or</div>
      <form class="stack" style="gap: 8px" @submit.prevent="joinLink">
        <span class="label" style="font-size: 15px; font-weight: 700">Join a pack a friend shared with you</span>
        <span class="hint" style="margin-top: -4px">Packs friends add you to are also under Library → Shared with me.</span>
        <div class="row">
          <input v-model="inviteLink" class="input mono" placeholder="https://tesseralauncher.com/share/…" aria-label="Invite link" />
          <button class="btn" :disabled="!inviteLink.trim() || busy">Join</button>
        </div>
      </form>
      <button type="button" class="option-row" @click="uploadPack">
        <span class="icon"><Upload /></span>
        <span><h3>Import a modpack file</h3><span class="hint">A Modrinth .mrpack or a Tessera .tapack. You can also drop the file onto the window.</span></span>
      </button>
      <p v-if="error" class="error-text">{{ error }}</p>
      <div class="dialog-footer" style="margin-top: 0">
        <button type="button" class="btn btn-lg" @click="(step = 'choose'), (error = '')"><ArrowLeft /> Back</button>
        <button type="button" class="btn btn-primary btn-lg" :disabled="busy || !selected.size" @click="runImport">
          <Loader2 v-if="busy" class="spin" /><Download v-else /> Import{{ selected.size ? ` ${selected.size}` : "" }}
        </button>
      </div>
    </div>

    <IconEditor v-if="editingIcon" :icon="icon" @close="editingIcon = false" @save="onIcon" />
  </AppDialog>
</template>
