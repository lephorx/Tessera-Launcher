<script setup lang="ts">
import { computed, ref, watchEffect } from "vue";
import { api, type Icon } from "@/lib/api";
import { backgroundOf, blockSvg } from "@/lib/icons";
import { iconVersion } from "@/lib/iconCache";

/** An instance icon: generated block on a gradient, an uploaded image, or a preview `src`. */
const props = withDefaults(defineProps<{ icon: Icon | null; id?: string; src?: string | null; size?: number }>(), { size: 56 });

const fileUrl = ref<string | null>(null);
watchEffect(async () => {
  void iconVersion.value; // reload after the icon changed
  fileUrl.value = props.icon?.kind === "file" && props.id && !props.src ? await api.instances.icon(props.id) : null;
});

const image = computed(() => props.src ?? fileUrl.value);
const generated = computed(() => (props.icon?.kind === "generated" ? props.icon : { kind: "generated" as const, background: "slate", symbol: "grass" }));
const style = computed(() => {
  const bg = backgroundOf(generated.value.background);
  return {
    width: `${props.size}px`,
    height: `${props.size}px`,
    background: image.value ? `center / cover no-repeat url("${image.value}")` : `linear-gradient(160deg, ${bg.from}, ${bg.to})`,
  };
});
</script>

<template>
  <span class="inst-icon" :style="style">
    <svg v-if="!image" viewBox="-11 -10 22 22" aria-hidden="true" v-html="blockSvg(generated.symbol)" />
  </span>
</template>
