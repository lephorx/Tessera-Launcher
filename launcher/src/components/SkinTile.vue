<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { Check, Pencil } from "lucide-vue-next";
import type { Skin } from "@/lib/api";
import { renderSkin } from "@/lib/skinRender";

/** One skin in a grid: a still 3D picture, a check when it's in use, Edit on hover. */
const props = defineProps<{ skin: Skin; selected: boolean }>();
const emit = defineEmits<{ pick: []; edit: [] }>();
const picture = ref("");

async function draw() {
  picture.value = await renderSkin(props.skin.texture, props.skin.model).catch(() => "");
}
onMounted(draw);
watch(() => [props.skin.texture, props.skin.model], draw);
</script>

<template>
  <div class="skin-tile" :class="{ on: selected }" :title="skin.name">
    <button class="skin-tile-pick" :aria-label="`Use ${skin.name}`" :aria-pressed="selected" @click="emit('pick')">
      <img v-if="picture" :src="picture" alt="" draggable="false" />
      <span v-else class="skin-tile-loading" />
    </button>
    <span v-if="selected" class="skin-tile-check"><Check :size="14" :stroke-width="3" /></span>
    <button class="btn btn-primary btn-sm skin-tile-edit" @click="emit('edit')"><Pencil /> Edit</button>
    <span class="skin-tile-name">{{ skin.name }}</span>
  </div>
</template>
