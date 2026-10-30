<script setup lang="ts">
import { computed } from "vue";
import { Loader2, Play, Square } from "lucide-vue-next";
import { api } from "@/lib/api";
import { runOf, runs } from "@/lib/store";
import { errorText, toast } from "@/lib/toast";

const props = defineProps<{ id: string; large?: boolean }>();
const run = computed(() => runOf(props.id));

async function play() {
  runs[props.id] = { state: "preparing", stage: "Preparing", done: 0, total: 0 };
  try {
    await api.instances.launch(props.id);
  } catch (e) {
    delete runs[props.id];
    toast.error(errorText(e));
  }
}

async function stop() {
  try {
    await api.instances.stop(props.id);
  } catch (e) {
    toast.error(errorText(e));
  }
}
</script>

<template>
  <button v-if="run.state === 'running'" class="btn btn-stop" :class="{ 'btn-lg': large }" @click.stop.prevent="stop">
    <Square /> Stop
  </button>
  <button v-else-if="run.state === 'preparing'" class="btn" :class="{ 'btn-lg': large }" disabled>
    <Loader2 class="spin" /> {{ run.total ? `${Math.round((run.done / run.total) * 100)}%` : "Starting" }}
  </button>
  <button v-else class="btn btn-play" :class="{ 'btn-lg': large }" @click.stop.prevent="play"><Play /> Play</button>
</template>
