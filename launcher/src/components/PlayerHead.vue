<script setup lang="ts">
import { ref, watch } from "vue";
import { headUrl } from "@/lib/player";

/** A Minecraft player's face, or their initial when it can't be loaded (offline). */
const props = withDefaults(defineProps<{ name: string; size?: number }>(), { size: 32 });
const failed = ref(false);
watch(
  () => props.name,
  () => (failed.value = false),
);
</script>

<template>
  <span class="player-head" :style="{ width: `${size}px`, height: `${size}px`, fontSize: `${Math.round(size * 0.42)}px` }">
    <img v-if="!failed" :src="headUrl(name)" :alt="name" draggable="false" @error="failed = true" />
    <template v-else>{{ name.slice(0, 1) }}</template>
  </span>
</template>
