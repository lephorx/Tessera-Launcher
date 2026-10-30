<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { api } from "@/lib/api";
import { saveSettings } from "@/lib/settingsDialog";
import { errorText, toast } from "@/lib/toast";

const memory = ref<number | null>(null);
const totalMb = ref(16384);
const maxMb = computed(() => Math.max(2048, Math.floor((totalMb.value * 0.75) / 512) * 512));
const gb = (mb: number) => (mb / 1024).toFixed(mb % 1024 ? 1 : 0);

onMounted(async () => {
  memory.value = (await api.settings.get()).memory_mb;
  totalMb.value = await api.system.memoryMb().catch(() => 16384);
});

/** Saved when the slider is let go. */
async function save() {
  if (memory.value === null) return;
  try {
    await saveSettings({ memory_mb: memory.value });
  } catch (e) {
    toast.error(errorText(e));
  }
}
</script>

<template>
  <div v-if="memory !== null" class="set-block">
    <h3 class="set-title">Memory per instance</h3>
    <div class="row">
      <input
        v-model.number="memory"
        class="range"
        type="range"
        min="1024"
        :max="maxMb"
        step="512"
        aria-label="Memory per instance"
        :style="{ '--range-fill': `${((Math.min(memory, maxMb) - 1024) / (maxMb - 1024)) * 100}%` }"
        @change="save"
      />
      <span class="mono num" style="width: 72px; text-align: right; font-weight: 600">{{ gb(memory) }} GB</span>
    </div>
    <p class="set-hint num">4 GB is enough for most modpacks. Each instance can override it. This computer has {{ gb(totalMb) }} GB.</p>
  </div>
  <div class="set-block">
    <h3 class="set-title">Java</h3>
    <p class="set-hint">The launcher downloads the Java each Minecraft version needs. To use your own, set it per instance under its settings.</p>
  </div>
</template>
