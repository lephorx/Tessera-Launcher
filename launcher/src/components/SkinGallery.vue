<script setup lang="ts">
import { onMounted, ref } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Loader2, Search, User } from "lucide-vue-next";
import { api, type GallerySkin } from "@/lib/api";
import { renderSkinAuto } from "@/lib/skinRender";
import { errorText } from "@/lib/toast";

/**
 * More skins: MineSkin's public gallery (searchable by name) and any player's skin by
 * their Minecraft name. Each shows as a still 3D picture; picking one previews it.
 */
export type Picked = GallerySkin & { model: "classic" | "slim" };
defineProps<{ previewing: string | null }>();
const emit = defineEmits<{ preview: [skin: Picked] }>();

type Tile = GallerySkin & { picture: string; detected: "classic" | "slim" | null };
const mode = ref<"gallery" | "player">("gallery");
const query = ref("");
const playerName = ref("");
const tiles = ref<Tile[]>([]);
const next = ref<string | null>(null);
const loading = ref(false);
const error = ref("");
let generation = 0;

/** Pictures render one after another; a tile shows once its picture is ready. */
function add(list: GallerySkin[], gen: number) {
  for (const s of list) {
    if (tiles.value.some((t) => t.id === s.id)) continue;
    const tile: Tile = { ...s, picture: "", detected: s.model };
    tiles.value.push(tile);
    void renderSkinAuto(s.texture)
      .then(({ picture, model }) => {
        if (gen !== generation) return;
        const t = tiles.value.find((x) => x.id === s.id);
        if (t) {
          t.picture = picture;
          t.detected = s.model ?? model;
        }
      })
      .catch(() => {
        tiles.value = tiles.value.filter((x) => x.id !== s.id);
      });
  }
}

async function search(more = false) {
  const gen = more ? generation : ++generation;
  if (!more) {
    tiles.value = [];
    next.value = null;
  }
  loading.value = true;
  error.value = "";
  try {
    const page = await api.skins.gallery(query.value, more ? next.value : null);
    if (gen !== generation) return;
    next.value = page.next;
    add(page.skins, gen);
  } catch (e) {
    error.value = errorText(e);
  } finally {
    if (gen === generation) loading.value = false;
  }
}

async function findPlayer() {
  const name = playerName.value.trim();
  if (!name) return;
  const gen = ++generation;
  tiles.value = [];
  next.value = null;
  loading.value = true;
  error.value = "";
  try {
    const s = await api.skins.player(name);
    if (gen !== generation) return;
    add([s], gen);
  } catch (e) {
    error.value = errorText(e);
  } finally {
    if (gen === generation) loading.value = false;
  }
}

function setMode(m: "gallery" | "player") {
  mode.value = m;
  error.value = "";
  if (m === "gallery") void search();
  else {
    generation++;
    tiles.value = [];
    next.value = null;
    loading.value = false;
  }
}

function pick(t: Tile) {
  if (!t.picture) return;
  emit("preview", { id: t.id, name: t.name, texture: t.texture, model: t.detected ?? "classic" });
}

onMounted(() => void search());
</script>

<template>
  <div class="stack" style="gap: 12px">
    <div class="row" style="gap: 8px; flex-wrap: wrap">
      <div v-slider class="tabs" role="tablist" style="margin-bottom: 0" aria-label="Where the skins come from">
        <button role="tab" :aria-selected="mode === 'gallery'" :class="{ active: mode === 'gallery' }" @click="setMode('gallery')">Gallery</button>
        <button role="tab" :aria-selected="mode === 'player'" :class="{ active: mode === 'player' }" @click="setMode('player')">A player's skin</button>
      </div>
      <form v-if="mode === 'gallery'" class="input-icon" style="flex: 1; min-width: 220px" @submit.prevent="search()">
        <Search />
        <input v-model="query" class="input" placeholder="Search skins: knight, pirate, robot…" aria-label="Search skins" maxlength="40" />
      </form>
      <form v-else class="row" style="flex: 1; min-width: 220px; gap: 8px" @submit.prevent="findPlayer">
        <div class="input-icon" style="flex: 1">
          <User />
          <input v-model="playerName" class="input" placeholder="Minecraft name" aria-label="Minecraft name" maxlength="16" />
        </div>
        <button class="btn" :disabled="loading || !playerName.trim()">Show</button>
      </form>
    </div>

    <p v-if="error" class="error-text">{{ error }}</p>
    <div class="skins-grid">
      <div v-for="t in tiles" :key="t.id" class="skin-tile" :class="{ on: previewing === t.id }" :title="t.name">
        <button class="skin-tile-pick" :aria-label="`Preview ${t.name}`" :aria-pressed="previewing === t.id" @click="pick(t)">
          <img v-if="t.picture" :src="t.picture" alt="" draggable="false" />
          <span v-else class="skin-tile-loading" />
        </button>
        <span class="skin-tile-name">{{ t.name }}</span>
      </div>
    </div>
    <p v-if="!loading && !error && !tiles.length && mode === 'gallery'" class="hint">No skins with that name. Try another word.</p>
    <p v-if="mode === 'player' && !tiles.length && !loading && !error" class="hint">See the skin any Minecraft player wears now, and save it.</p>
    <div v-if="loading" class="hint row" style="gap: 6px"><Loader2 class="spin" :size="14" /> Loading skins…</div>
    <button v-else-if="mode === 'gallery' && next" class="btn" style="align-self: center" @click="search(true)">Load more</button>
    <p class="hint" style="font-size: 12px">
      Gallery skins are uploads to <a href="#" @click.prevent="openUrl('https://mineskin.org')">MineSkin</a>'s public gallery, made by their players.
    </p>
  </div>
</template>
