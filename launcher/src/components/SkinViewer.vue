<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import { IdleAnimation, SkinViewer } from "skinview3d";

/** A player model in 3D you can drag to turn, with an idle animation. */
const props = withDefaults(defineProps<{ texture: string; model: "classic" | "slim"; cape?: string | null; width?: number; height?: number }>(), {
  cape: null,
  width: 300,
  height: 420,
});

const canvas = ref<HTMLCanvasElement>();
let viewer: SkinViewer | null = null;

async function load() {
  if (!viewer) return;
  await viewer.loadSkin(props.texture, { model: props.model === "slim" ? "slim" : "default" });
  if (props.cape) await viewer.loadCape(props.cape);
  else viewer.loadCape(null);
}

onMounted(() => {
  viewer = new SkinViewer({ canvas: canvas.value, width: props.width, height: props.height, pixelRatio: "match-device", zoom: 0.85 });
  viewer.animation = new IdleAnimation();
  viewer.controls.enableZoom = false;
  viewer.controls.enablePan = false;
  viewer.playerWrapper.rotation.y = -0.45;
  void load();
});
watch(() => [props.texture, props.model, props.cape], load);
watch(
  () => [props.width, props.height],
  () => viewer?.setSize(props.width, props.height),
);
onUnmounted(() => viewer?.dispose());
</script>

<template>
  <canvas ref="canvas" class="skin-viewer" :style="{ width: `${width}px`, height: `${height}px` }" />
</template>

<style scoped>
.skin-viewer {
  display: block;
  cursor: grab;
}
.skin-viewer:active {
  cursor: grabbing;
}
</style>
