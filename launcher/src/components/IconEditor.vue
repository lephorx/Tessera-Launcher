<script setup lang="ts">
import { ref } from "vue";
import { Check, Info, RefreshCw, Save, Upload, X } from "lucide-vue-next";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type Icon } from "@/lib/api";
import { BACKGROUNDS, SYMBOLS, randomIcon } from "@/lib/icons";
import AppDialog from "./AppDialog.vue";
import InstanceIcon from "./InstanceIcon.vue";

/** Mix a background and a block, or upload an image. Emits the result; the caller saves it. */
const props = defineProps<{ icon: Icon | null }>();
const emit = defineEmits<{ close: []; save: [icon: Icon, file: string | null, preview: string | null] }>();

const start = props.icon?.kind === "generated" ? props.icon : (randomIcon() as Extract<Icon, { kind: "generated" }>);
const background = ref(start.background);
const symbol = ref(start.symbol);
const file = ref<string | null>(null);
const preview = ref<string | null>(null);

function randomize() {
  const r = randomIcon() as Extract<Icon, { kind: "generated" }>;
  background.value = r.background;
  symbol.value = r.symbol;
  file.value = preview.value = null;
}

async function upload() {
  const picked = await open({ multiple: false, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }] });
  if (typeof picked !== "string") return;
  file.value = picked;
  preview.value = await api.instances.imagePreview(picked);
}

function save() {
  if (file.value) emit("save", { kind: "file" }, file.value, preview.value);
  else emit("save", { kind: "generated", background: background.value, symbol: symbol.value }, null, null);
}

const current = (): Icon => ({ kind: "generated", background: background.value, symbol: symbol.value });
</script>

<template>
  <AppDialog title="Icon editor" :width="860" @close="emit('close')">
    <div style="display: grid; grid-template-columns: 220px 1fr; gap: 24px">
      <div class="stack" style="align-items: center">
        <div class="inset stack" style="align-items: center; width: 100%; padding: 20px">
          <InstanceIcon :icon="current()" :src="preview" :size="132" />
          <div class="row" style="gap: 10px; align-items: flex-end">
            <InstanceIcon :icon="current()" :src="preview" :size="40" />
            <InstanceIcon :icon="current()" :src="preview" :size="30" />
            <InstanceIcon :icon="current()" :src="preview" :size="20" />
          </div>
        </div>
        <button class="btn" style="width: 100%" @click="randomize"><RefreshCw /> Randomize</button>
        <button class="btn" style="width: 100%" @click="upload"><Upload /> Upload image</button>
      </div>
      <div class="stack" style="min-width: 0">
        <span class="label">Background</span>
        <div class="row" style="flex-wrap: wrap; gap: 10px">
          <button
            v-for="bg in BACKGROUNDS"
            :key="bg.id"
            class="swatch"
            :title="bg.id" :aria-label="`Background ${bg.id}`" :aria-pressed="!file && background === bg.id"
            :style="{ background: `linear-gradient(160deg, ${bg.from}, ${bg.to})` }"
            :class="{ on: !file && background === bg.id }"
            @click="(background = bg.id), (file = preview = null)"
          >
            <Check v-if="!file && background === bg.id" />
          </button>
        </div>
        <span class="label" style="margin-top: 8px">Symbol</span>
        <div class="symbols">
          <button
            v-for="s in SYMBOLS"
            :key="s.id"
            class="symbol"
            :class="{ on: !file && symbol === s.id }"
            :title="s.name" :aria-label="`Symbol ${s.name}`" :aria-pressed="!file && symbol === s.id"
            @click="(symbol = s.id), (file = preview = null)"
          >
            <InstanceIcon :icon="{ kind: 'generated', background: 'night', symbol: s.id }" :size="56" class="symbol-icon" />
          </button>
        </div>
      </div>
    </div>
    <div class="row" style="margin-top: 20px">
      <Info :size="16" class="muted" />
      <span class="hint">Mix and match a background and a block, or upload your own image.</span>
      <span class="spacer" />
      <button class="btn" @click="emit('close')"><X /> Cancel</button>
      <button class="btn btn-primary" @click="save"><Save /> Save icon</button>
    </div>
  </AppDialog>
</template>

<style scoped>
.swatch {
  display: grid;
  place-items: center;
  width: 48px;
  height: 48px;
  border: 2px solid transparent;
  color: white;
  cursor: pointer;
}
.swatch svg {
  width: 18px;
  height: 18px;
  filter: drop-shadow(0 1px 2px rgb(0 0 0 / 0.5));
}
.swatch.on {
  border-color: var(--foreground);
}
html[data-design="modern"] .swatch {
  border-radius: 14px;
}
.symbols {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(64px, 1fr));
  gap: 8px;
  max-height: 340px;
  overflow-y: auto;
  padding: 3px;
}
.symbol {
  position: relative;
  display: grid;
  place-items: center;
  aspect-ratio: 1;
  border: 2px solid transparent;
  background: var(--surface-2);
  cursor: pointer;
  padding: 0;
}
.symbol.on {
  border-color: var(--primary);
}
.symbol :deep(.symbol-icon) {
  background: transparent !important;
  width: 100% !important;
  height: 100% !important;
}
.symbol :deep(.symbol-icon svg) {
  width: 78%;
  height: 78%;
}
html[data-design="modern"] .symbol {
  border-radius: 14px;
}
</style>
