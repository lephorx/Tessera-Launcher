<script setup lang="ts">
import { computed } from "vue";
import { RouterLink } from "vue-router";
import { Clock, Package } from "lucide-vue-next";
import type { Instance } from "@/lib/api";
import { LOADERS, relative } from "@/lib/format";
import { runOf } from "@/lib/store";
import InstanceIcon from "./InstanceIcon.vue";
import PlayButton from "./PlayButton.vue";
import StateBadge from "./StateBadge.vue";

const props = defineProps<{ instance: Instance }>();
const run = computed(() => runOf(props.instance.id));
</script>

<template>
  <RouterLink :to="`/instance/${instance.id}`" class="card card-hover stack" style="color: inherit">
    <div class="row" style="gap: 12px">
      <InstanceIcon :icon="instance.icon" :id="instance.id" :size="56" />
      <div style="min-width: 0; flex: 1">
        <div style="font-weight: 700; font-size: 16px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap">
          {{ instance.name }}
        </div>
        <div class="row" style="margin-top: 4px"><StateBadge :id="instance.id" /></div>
      </div>
    </div>
    <div class="row" style="flex-wrap: wrap">
      <span class="chip" :style="{ color: LOADERS[instance.loader].color }">{{ LOADERS[instance.loader].label }}</span>
      <span class="chip num">{{ instance.game_version }}</span>
      <span v-if="instance.mods.length" class="chip num"><Package />{{ instance.mods.length }}</span>
    </div>
    <div v-if="run.state === 'preparing'" class="stack" style="gap: 6px">
      <div class="hint">{{ run.stage }}</div>
      <div class="progress"><div :style="{ width: run.total ? `${(run.done / run.total) * 100}%` : '8%' }" /></div>
    </div>
    <div class="row">
      <span class="hint row" style="gap: 5px"><Clock :size="13" />{{ relative(instance.last_played) }}</span>
      <span class="spacer" />
      <PlayButton :id="instance.id" />
    </div>
  </RouterLink>
</template>
