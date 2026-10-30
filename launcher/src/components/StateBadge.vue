<script setup lang="ts">
import { computed } from "vue";
import { runOf } from "@/lib/store";

const props = defineProps<{ id: string }>();
const run = computed(() => runOf(props.id));
const label = computed(() =>
  run.value.state === "running" ? "Running" : run.value.state === "preparing" ? "Starting" : "Ready",
);
</script>

<template>
  <span class="badge" :data-state="run.state"><span class="dot" />{{ label }}</span>
</template>
