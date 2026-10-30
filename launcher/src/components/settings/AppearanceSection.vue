<script setup lang="ts">
import { ref } from "vue";
import { Check } from "lucide-vue-next";
import { currentDesign, setDesign, type Design } from "@/lib/design";
import { saveSettings } from "@/lib/settingsDialog";
import { errorText, toast } from "@/lib/toast";

const design = ref<Design>(currentDesign());

async function pick(d: Design) {
  design.value = d;
  setDesign(d);
  try {
    await saveSettings({ design: d });
  } catch (e) {
    toast.error(errorText(e));
  }
}
</script>

<template>
  <div class="set-block">
    <h3 class="set-title">Design</h3>
    <p class="set-hint">How the launcher looks. Tessera's dashboard uses the same two.</p>
    <div class="row" style="gap: 12px">
      <button
        v-for="d in (['modern', 'classic'] as Design[])"
        :key="d"
        type="button"
        class="card card-hover option"
        :class="{ selected: design === d }"
        style="flex: 1; flex-direction: column; align-items: stretch; gap: 10px"
        @click="pick(d)"
      >
        <span
          :style="{
            height: '90px',
            background: d === 'modern' ? 'linear-gradient(160deg,#2b2e35,#16181d)' : '#09090b',
            borderRadius: d === 'modern' ? '12px' : '0',
            border: d === 'classic' ? '1px solid rgb(255 255 255 / .1)' : 'none',
            display: 'flex',
            gap: '8px',
            padding: '14px',
          }"
        >
          <span :style="{ width: '34%', background: d === 'modern' ? '#212329' : '#18181b', borderRadius: d === 'modern' ? '8px' : '0' }" />
          <span :style="{ flex: 1, background: d === 'modern' ? '#212329' : '#18181b', borderRadius: d === 'modern' ? '8px' : '0' }" />
        </span>
        <span class="row">
          <span style="font-weight: 600">{{ d === "modern" ? "Modern" : "Classic" }}</span>
          <span class="spacer" />
          <Check v-if="design === d" :size="16" style="color: var(--tint)" />
        </span>
      </button>
    </div>
  </div>
</template>
