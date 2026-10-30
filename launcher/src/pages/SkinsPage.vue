<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { ChevronDown, Loader2, Move3d, Pencil, Plus } from "lucide-vue-next";
import SkinEditDialog from "@/components/SkinEditDialog.vue";
import SkinTile from "@/components/SkinTile.vue";
import SkinViewer from "@/components/SkinViewer.vue";
import { api, type Skin, type Skins } from "@/lib/api";
import { player } from "@/lib/player";
import { errorText, toast } from "@/lib/toast";

/** Pick the skin the active player uses: your saved skins and Minecraft's default ones. */
const skins = ref<Skins | null>(null);
const editing = ref<Skin | null>(null);
const open_ = ref({ saved: true, defaults: true });
const adding = ref(false);

const all = computed(() => [...(skins.value?.saved ?? []), ...(skins.value?.defaults ?? [])]);
const current = computed(() => all.value.find((s) => s.id === skins.value?.selected) ?? skins.value?.defaults.find((s) => s.id === "default:steve:classic") ?? null);

async function load() {
  try {
    skins.value = await api.skins.list();
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function pick(s: Skin) {
  try {
    await api.skins.select(s.id);
    if (skins.value) skins.value.selected = s.id;
  } catch (e) {
    toast.error(errorText(e));
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
  window.addEventListener("tessera-skin-files", onDrop);
});
onUnmounted(() => window.removeEventListener("tessera-skin-files", onDrop));

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
      <SkinViewer v-if="current" :texture="current.texture" :model="current.model" :width="260" :height="380" />
      <div v-else class="empty" style="height: 380px"><Loader2 class="spin" /></div>
      <span class="hint row" style="gap: 6px"><Move3d :size="14" /> Drag to rotate</span>
      <button v-if="current" class="btn" @click="editing = current"><Pencil /> Edit skin</button>
      <p class="hint skins-note">
        Offline accounts show the skin in your own game (singleplayer and LAN), through a small resource pack the launcher adds.
        With Microsoft sign-in it will show for everyone.
      </p>
    </aside>

    <div class="skins-lists">
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
        <div class="skins-grid">
          <SkinTile v-for="s in skins?.defaults ?? []" :key="s.id" :skin="s" :selected="skins?.selected === s.id" @pick="pick(s)" @edit="editing = s" />
        </div>
      </template>
    </div>

    <SkinEditDialog v-if="editing" :skin="editing" :capes="[]" @close="editing = null" @saved="saved" @removed="(editing = null), load()" />
  </div>
</template>
