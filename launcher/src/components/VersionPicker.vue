<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from "vue";
import { Check, ChevronDown, Search, X } from "lucide-vue-next";
import type { GameVersion } from "@/lib/api";

/**
 * Game version picker like the website's filter: a search box, the 10 newest releases,
 * and "Show all versions" (snapshots and old versions too). Opens inline, so it works
 * inside dialogs. With `clearable`, the value can be empty ("Any version").
 */
const props = defineProps<{ versions: GameVersion[]; clearable?: boolean; placeholder?: string }>();
const model = defineModel<string>({ required: true });

const open = ref(false);
const query = ref("");
const all = ref(false);
const input = ref<HTMLInputElement | null>(null);
const root = ref<HTMLElement | null>(null);

// a click anywhere else closes it
function outside(e: MouseEvent) {
  if (!root.value?.contains(e.target as Node)) open.value = false;
}
watch(open, (o) => (o ? document.addEventListener("mousedown", outside) : document.removeEventListener("mousedown", outside)));
onUnmounted(() => document.removeEventListener("mousedown", outside));

const shown = computed(() => {
  const q = query.value.trim();
  const pool = props.versions.filter((v) => q || all.value || v.kind === "release");
  const hits = q ? pool.filter((v) => v.id.includes(q)) : pool;
  return q || all.value ? hits : hits.slice(0, 10);
});

const kindLabel = (k: GameVersion["kind"]) => (k === "release" ? "" : k === "snapshot" ? "Snapshot" : k === "old_beta" ? "Beta" : "Alpha");

async function toggle() {
  open.value = !open.value;
  if (open.value) {
    query.value = "";
    await nextTick();
    input.value?.focus();
  }
}

function pick(id: string) {
  model.value = id;
  open.value = false;
}
</script>

<template>
  <div ref="root" class="version-picker">
    <div class="vp-field">
      <button type="button" class="select vp-trigger" :aria-expanded="open" :disabled="!versions.length" @click="toggle">
        <span :class="{ muted: !model }">{{ model || placeholder || "Any version" }}</span>
        <span class="spacer" />
        <span v-if="clearable && model" class="vp-clear-slot" />
        <ChevronDown :size="16" class="muted" :style="{ rotate: open ? '180deg' : '0deg' }" />
      </button>
      <!-- beside the trigger, not inside it: a button in a button can't be reached with Tab -->
      <button v-if="clearable && model" type="button" class="vp-clear" title="Clear" aria-label="Clear version" @click="model = ''"><X :size="15" /></button>
    </div>
    <div v-if="open" class="vp-panel inset stack" style="gap: 8px">
      <div class="input-icon">
        <Search />
        <input ref="input" v-model="query" class="input" placeholder="Search…" aria-label="Search versions" @keydown.esc.stop="open = false" />
      </div>
      <div class="vp-list">
        <button v-if="clearable" type="button" class="vp-item" :class="{ on: !model }" @click="pick('')">
          <span>Any version</span><span class="spacer" /><Check v-if="!model" :size="15" />
        </button>
        <button v-for="v in shown" :key="v.id" type="button" class="vp-item num" :class="{ on: model === v.id }" @click="pick(v.id)">
          <span>{{ v.id }}</span>
          <span v-if="kindLabel(v.kind)" class="pill">{{ kindLabel(v.kind) }}</span>
          <span class="spacer" />
          <Check v-if="model === v.id" :size="15" />
        </button>
        <p v-if="!shown.length" class="hint" style="padding: 6px 10px">No version matches “{{ query }}”.</p>
      </div>
      <label v-if="!query" class="row hint" style="font-weight: 400; cursor: pointer">
        <input v-model="all" type="checkbox" /> Show all versions
      </label>
    </div>
  </div>
</template>

<style scoped>
.version-picker {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.vp-field {
  position: relative;
  display: flex;
  flex-direction: column;
}
.vp-clear {
  position: absolute;
  top: 50%;
  right: 34px;
  translate: 0 -50%;
  display: grid;
  place-items: center;
  width: 22px;
  height: 22px;
  padding: 0;
  border: none;
  border-radius: 6px;
  background: none;
  color: var(--muted-foreground);
  cursor: pointer;
}
.vp-clear:hover {
  color: var(--foreground);
  background: color-mix(in srgb, var(--foreground) 8%, transparent);
}
.vp-clear-slot {
  width: 22px;
  flex-shrink: 0;
}
.vp-trigger {
  display: flex;
  align-items: center;
  gap: 8px;
  text-align: left;
  cursor: pointer;
}
.vp-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 260px;
  overflow-y: auto;
}
.vp-item {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 32px;
  padding: 0 10px;
  border: none;
  background: none;
  color: var(--foreground);
  font: inherit;
  font-weight: 600;
  text-align: left;
  cursor: pointer;
}
.vp-item:hover {
  background: var(--muted);
}
.vp-item.on {
  background: var(--tint-bg);
  color: var(--tint);
}
html[data-design="modern"] .vp-item {
  border-radius: 10px;
}
html[data-design="modern"] .vp-item:hover:not(.on) {
  background: color-mix(in srgb, var(--foreground) 6%, transparent);
}
</style>
