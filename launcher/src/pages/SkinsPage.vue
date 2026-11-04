<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { ChevronDown, Download, Loader2, Move3d, Pencil, Plus, X } from "lucide-vue-next";
import SkinEditDialog from "@/components/SkinEditDialog.vue";
import SkinGallery, { type Picked } from "@/components/SkinGallery.vue";
import SkinTile from "@/components/SkinTile.vue";
import SkinViewer from "@/components/SkinViewer.vue";
import { api, type Cape, type Skin, type Skins } from "@/lib/api";
import { ensureAccount, player } from "@/lib/player";
import { errorText, toast } from "@/lib/toast";

/** Pick the skin the active player uses: your saved skins, Minecraft's default ones, and
 * more from MineSkin's gallery or any player. */
const skins = ref<Skins | null>(null);
const editing = ref<Skin | null>(null);
const open_ = ref({ saved: true, defaults: true, more: true });
/** A gallery or player skin shown in the big preview before it's saved. */
const previewing = ref<Picked | null>(null);
const savingPreview = ref(false);
/** The default characters come with both arms; one switch picks which the grid shows. */
const arms = ref<"classic" | "slim">("classic");
const defaultsShown = computed(() => (skins.value?.defaults ?? []).filter((s) => s.model === arms.value).map((s) => ({ ...s, name: s.name.replace(/ \(slim\)$/, "") })));
const adding = ref(false);
const capes = ref<Cape[]>([]);
const applying = ref<string | null>(null);

const all = computed(() => [...(skins.value?.account ? [skins.value.account] : []), ...(skins.value?.saved ?? []), ...(skins.value?.defaults ?? [])]);
const current = computed(() => all.value.find((s) => s.id === skins.value?.selected) ?? skins.value?.defaults.find((s) => s.id === "default:steve:classic") ?? null);
const shown = computed(() => (previewing.value ? { texture: previewing.value.texture, model: previewing.value.model, cape: null } : current.value ? { texture: current.value.texture, model: current.value.model, cape: player.microsoft ? capeOf(current.value) : null } : null));
const capeOf = (s: Skin) => capes.value.find((c) => c.id === s.cape)?.texture ?? null;

async function load() {
  try {
    skins.value = await api.skins.list();
    // the arms of the default skin in use
    const sel = skins.value.defaults.find((s) => s.id === skins.value?.selected);
    if (sel) arms.value = sel.model;
  } catch (e) {
    toast.error(errorText(e));
  }
}

/** The Microsoft account's capes, for the preview and the editor. */
async function loadCapes() {
  if (!player.microsoft) {
    capes.value = [];
    return;
  }
  try {
    capes.value = await api.microsoft.capes();
  } catch (e) {
    toast.error(errorText(e));
  }
}
watch(() => player.microsoft, loadCapes);
// each account has its own skin
watch(() => player.name, load);

async function pick(s: Skin) {
  if (applying.value || !(await ensureAccount())) return;
  // a Microsoft account's skin changes on Mojang's servers, which takes a moment
  applying.value = s.id;
  try {
    await api.skins.select(s.id);
    if (skins.value) skins.value.selected = s.id;
    if (player.microsoft) toast.success(`${s.name} is your skin now`);
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    applying.value = null;
  }
}

/** Adds PNG files as saved skins; slim if the file name says so. */
async function addFiles(paths: string[]) {
  adding.value = true;
  let last = "";
  for (const path of paths) {
    const file = path.split(/[\\/]/).pop() ?? "skin.png";
    const name = file.replace(/\.png$/i, "");
    try {
      const s = await api.skins.add({ path, name, model: /slim|alex/i.test(name) ? "slim" : "classic", cape: null });
      last = s.id;
    } catch (e) {
      toast.error(`${file}: ${errorText(e)}`);
    }
  }
  adding.value = false;
  await load();
  if (last) {
    const added = skins.value?.saved.find((s) => s.id === last);
    if (added) await pick(added);
    toast.success(paths.length > 1 ? "Skins added" : `${added?.name ?? "Skin"} added and in use`);
  }
}

async function browse() {
  const picked = await open({ multiple: true, filters: [{ name: "Minecraft skin", extensions: ["png"] }] });
  if (picked) await addFiles(Array.isArray(picked) ? picked : [picked]);
}

// files dropped on the window while this page is open (App.vue passes them on)
const onDrop = (e: Event) => void addFiles((e as CustomEvent<string[]>).detail);
onMounted(() => {
  void load();
  void loadCapes();
  window.addEventListener("tessera-skin-files", onDrop);
});
onUnmounted(() => window.removeEventListener("tessera-skin-files", onDrop));

/** Saves the previewed gallery skin; `use` also makes it your skin. */
async function savePreview(use: boolean) {
  const p = previewing.value;
  if (!p || savingPreview.value) return;
  savingPreview.value = true;
  try {
    const s = await api.skins.saveTexture(p.id, p.name === "Untitled" ? "Gallery skin" : p.name, p.model);
    await load();
    previewing.value = null;
    const added = skins.value?.saved.find((x) => x.id === s.id);
    if (use && added) await pick(added);
    else toast.success(`${s.name} saved`);
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    savingPreview.value = false;
  }
}

async function saved(id: string) {
  editing.value = null;
  await load();
  const s = all.value.find((x) => x.id === id);
  if (s) await pick(s);
}
</script>

<template>
  <div class="page enter skins-page">
    <aside class="skins-preview">
      <h1 class="page-title">Skins</h1>
      <span class="skins-name mono">{{ player.name }}</span>
      <SkinViewer v-if="shown" :texture="shown.texture" :model="shown.model" :cape="shown.cape" :width="260" :height="380" />
      <div v-else class="empty" style="height: 380px"><Loader2 class="spin" /></div>
      <span v-if="applying" class="hint row" style="gap: 6px"><Loader2 class="spin" :size="14" /> Changing your skin…</span>
      <span v-else class="hint row" style="gap: 6px"><Move3d :size="14" /> Drag to rotate</span>
      <template v-if="previewing">
        <span class="skins-name" style="text-align: center">{{ previewing.name }}</span>
        <button class="btn btn-primary" :disabled="savingPreview" @click="savePreview(true)">
          <Loader2 v-if="savingPreview" class="spin" /><Download v-else /> Save and use
        </button>
        <div class="row" style="gap: 8px">
          <button class="btn" :disabled="savingPreview" @click="savePreview(false)">Save</button>
          <button class="btn btn-ghost" :disabled="savingPreview" @click="previewing = null"><X /> Back</button>
        </div>
      </template>
      <button v-else-if="current" class="btn" @click="editing = current"><Pencil /> Edit skin</button>
      <p v-if="player.microsoft" class="hint skins-note">Picking a skin makes it your Minecraft skin and cape, so everyone sees it.</p>
      <p v-else class="hint skins-note">Add your Minecraft account under Playing as on Home to change your skin.</p>
    </aside>

    <div class="skins-lists">
      <template v-if="skins?.account">
        <h2 class="skins-section" style="cursor: default">On your Minecraft account</h2>
        <div class="skins-grid">
          <SkinTile :skin="skins.account" :selected="skins.selected === skins.account.id" @pick="pick(skins.account)" @edit="editing = skins.account" />
        </div>
      </template>

      <button class="skins-section" :aria-expanded="open_.saved" @click="open_.saved = !open_.saved">
        <ChevronDown :size="18" class="side-chevron" :class="{ open: !open_.saved }" style="transform-origin: center" /> Saved skins
      </button>
      <div v-if="open_.saved" class="skins-grid">
        <button class="skin-add" :disabled="adding" @click="browse">
          <Loader2 v-if="adding" class="spin" :size="26" /><Plus v-else :size="28" />
          <strong>Add skin</strong>
          <span class="hint">Choose a file or drop one here</span>
        </button>
        <SkinTile v-for="s in skins?.saved ?? []" :key="s.id" :skin="s" :selected="skins?.selected === s.id" @pick="pick(s)" @edit="editing = s" />
      </div>

      <button class="skins-section" :aria-expanded="open_.defaults" @click="open_.defaults = !open_.defaults">
        <ChevronDown :size="18" class="side-chevron" :class="{ open: !open_.defaults }" /> Default skins
      </button>
      <template v-if="open_.defaults">
        <p v-if="skins && !skins.defaults.length" class="hint">The default skins come from Minecraft itself; play any version once (1.19.3 or newer) to get them.</p>
        <template v-else>
          <div v-slider class="tabs" role="tablist" aria-label="Arms" style="margin-bottom: 0; align-self: flex-start">
            <button role="tab" :aria-selected="arms === 'classic'" :class="{ active: arms === 'classic' }" @click="arms = 'classic'">Classic arms</button>
            <button role="tab" :aria-selected="arms === 'slim'" :class="{ active: arms === 'slim' }" @click="arms = 'slim'">Slim arms</button>
          </div>
          <div class="skins-grid">
            <SkinTile v-for="s in defaultsShown" :key="s.id" :skin="s" :selected="skins?.selected === s.id" @pick="pick(s)" @edit="editing = s" />
          </div>
        </template>
      </template>

      <button class="skins-section" :aria-expanded="open_.more" @click="open_.more = !open_.more">
        <ChevronDown :size="18" class="side-chevron" :class="{ open: !open_.more }" /> More skins
      </button>
      <SkinGallery v-if="open_.more" :previewing="previewing?.id ?? null" @preview="(s) => (previewing = s)" />
    </div>

    <SkinEditDialog v-if="editing" :skin="editing" :capes="capes" @close="editing = null" @saved="saved" @removed="(editing = null), load()" />
  </div>
</template>
