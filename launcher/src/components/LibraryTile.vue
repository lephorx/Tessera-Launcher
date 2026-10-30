<script setup lang="ts">
import { computed } from "vue";
import { RouterLink } from "vue-router";
import { Check, Loader2, Play, Square } from "lucide-vue-next";
import { api, type Instance } from "@/lib/api";
import { LOADERS } from "@/lib/format";
import { runOf, runs } from "@/lib/store";
import { ensureAccount } from "@/lib/player";
import { errorText, toast } from "@/lib/toast";
import InstanceIcon from "./InstanceIcon.vue";

/** Compact library tile: icon, name, loader + version. Hover: play and select. */
const props = defineProps<{ instance: Instance; selected: boolean; selecting: boolean }>();
const emit = defineEmits<{ toggle: [shift: boolean] }>();
const run = computed(() => runOf(props.instance.id));

async function play() {
  if (!(await ensureAccount())) return;
  runs[props.instance.id] = { state: "preparing", stage: "Preparing", done: 0, total: 0 };
  try {
    await api.instances.launch(props.instance.id);
  } catch (e) {
    delete runs[props.instance.id];
    toast.error(errorText(e));
  }
}

async function stop() {
  await api.instances.stop(props.instance.id).catch((e) => toast.error(errorText(e)));
}

/** While something is selected, a click selects instead of opening. */
function onClick(e: MouseEvent) {
  if (props.selecting || e.metaKey || e.ctrlKey || e.shiftKey) {
    e.preventDefault();
    emit("toggle", e.shiftKey);
  }
}
</script>

<template>
  <RouterLink :to="`/instance/${instance.id}`" class="lib-tile" :class="{ selected, running: run.state !== 'idle' }" @click="onClick">
    <div class="lib-icon">
      <InstanceIcon :icon="instance.icon" :id="instance.id" :size="168" />
      <button
        type="button"
        class="lib-check"
        :class="{ on: selected }"
        :aria-label="selected ? `Deselect ${instance.name}` : `Select ${instance.name}`"
        @click.stop.prevent="emit('toggle', $event.shiftKey)"
      >
        <Check v-if="selected" :size="15" />
      </button>
      <button
        v-if="!selecting"
        type="button"
        class="lib-play"
        :class="run.state"
        :title="run.state === 'running' ? 'Stop' : run.state === 'preparing' ? run.stage : 'Play'"
        :aria-label="run.state === 'running' ? `Stop ${instance.name}` : `Play ${instance.name}`"
        :disabled="run.state === 'preparing'"
        @click.stop.prevent="run.state === 'running' ? stop() : play()"
      >
        <Loader2 v-if="run.state === 'preparing'" class="spin" :size="20" />
        <Square v-else-if="run.state === 'running'" :size="18" />
        <Play v-else :size="20" />
      </button>
    </div>
    <div class="lib-name">{{ instance.name }}</div>
    <div class="lib-sub">{{ LOADERS[instance.loader].label }} {{ instance.game_version }}</div>
  </RouterLink>
</template>

<style scoped>
.lib-tile {
  display: flex;
  flex-direction: column;
  padding: 12px;
  border: 1px solid var(--border);
  background: var(--card);
  color: inherit;
  min-width: 0;
}
.lib-icon {
  position: relative;
  aspect-ratio: 1;
  margin-bottom: 10px;
}
.lib-icon :deep(.inst-icon) {
  width: 100% !important;
  height: 100% !important;
}
.lib-name {
  font-weight: 700;
  font-size: 15px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.lib-sub {
  margin-top: 2px;
  font-size: 13px;
  color: var(--muted-foreground);
  white-space: nowrap;
}
.lib-check {
  position: absolute;
  top: 8px;
  right: 8px;
  display: grid;
  place-items: center;
  width: 26px;
  height: 26px;
  border-radius: 999px;
  border: 2px solid rgb(255 255 255 / 0.85);
  background: rgb(0 0 0 / 0.25);
  color: #111;
  cursor: pointer;
  opacity: 0;
  transition: opacity var(--dur-fast, 0s);
}
.lib-check.on {
  background: white;
  border-color: white;
  opacity: 1;
}
.lib-play {
  position: absolute;
  right: 10px;
  bottom: 10px;
  display: grid;
  place-items: center;
  width: 46px;
  height: 46px;
  border: none;
  border-radius: 999px;
  background: var(--fill-green, #22c55e);
  color: var(--on-fill, #0e1116);
  box-shadow: 0 6px 18px rgb(0 0 0 / 0.35);
  cursor: pointer;
  opacity: 0;
  translate: 0 6px;
  transition: opacity var(--dur-fast, 0s), translate var(--dur, 0s) var(--ease-out, ease), scale var(--dur, 0s) var(--ease-spring, ease);
}
.lib-play.running {
  background: var(--fill-red, #ef4444);
}
.lib-play:hover {
  scale: 1.06;
}
.lib-play:active {
  scale: 0.94;
}
.lib-tile:hover .lib-check,
.lib-tile:focus-visible .lib-check,
.lib-tile.selected .lib-check {
  opacity: 1;
}
.lib-tile:hover .lib-play,
.lib-tile:focus-within .lib-play,
.lib-tile.running .lib-play {
  opacity: 1;
  translate: 0 0;
}
.lib-tile.selected {
  border-color: var(--foreground);
  outline: 1px solid var(--foreground);
}
html[data-design="modern"] .lib-tile {
  border-radius: 20px;
  border-color: transparent;
  box-shadow: var(--surface-shadow);
  transition: translate var(--dur) var(--ease-out), box-shadow var(--dur) var(--ease-out);
}
html[data-design="modern"] .lib-tile:hover {
  translate: 0 -2px;
  box-shadow: var(--hover-shadow);
}
html[data-design="modern"] .lib-tile.selected {
  border-color: color-mix(in srgb, var(--foreground) 70%, transparent);
  outline: none;
}
html[data-design="modern"] .lib-icon :deep(.inst-icon) {
  border-radius: 16px;
}
</style>
