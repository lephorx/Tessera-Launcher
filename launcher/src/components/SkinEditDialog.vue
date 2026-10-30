<script setup lang="ts">
import { ref } from "vue";
import { Loader2, Save, Trash2, X } from "lucide-vue-next";
import { api, type Skin } from "@/lib/api";
import { errorText, toast } from "@/lib/toast";
import AppDialog from "./AppDialog.vue";
import SkinViewer from "./SkinViewer.vue";

/**
 * Name, arm width and cape of a skin. A default skin is saved as your own copy. Capes are
 * the ones your Minecraft account owns, which needs Microsoft sign-in.
 */
const props = defineProps<{ skin: Skin; capes: { id: string; name: string; texture: string }[] }>();
const emit = defineEmits<{ close: []; saved: [id: string]; removed: [] }>();

const name = ref(props.skin.default ? props.skin.name.replace(" (slim)", "") : props.skin.name);
const model = ref<"classic" | "slim">(props.skin.model);
const cape = ref<string | null>(props.skin.cape);
const busy = ref(false);
const confirmDelete = ref(false);
const capeTexture = (id: string | null) => props.capes.find((c) => c.id === id)?.texture ?? null;

async function save() {
  busy.value = true;
  try {
    if (props.skin.default) {
      const s = await api.skins.add({ from: props.skin.id, name: name.value, model: model.value, cape: cape.value });
      toast.success(`${s.name} saved to your skins`);
      emit("saved", s.id);
    } else {
      await api.skins.update(props.skin.id, name.value, model.value, cape.value);
      emit("saved", props.skin.id);
    }
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = false;
  }
}

async function remove() {
  try {
    await api.skins.remove(props.skin.id);
    emit("removed");
  } catch (e) {
    toast.error(errorText(e));
  }
}
</script>

<template>
  <AppDialog title="Editing skin" :width="640" @close="emit('close')">
    <div class="row" style="align-items: flex-start; gap: 24px">
      <div class="stack" style="align-items: center; gap: 6px">
        <SkinViewer :texture="skin.texture" :model="model" :cape="capeTexture(cape)" :width="220" :height="320" />
        <span class="hint">Drag to rotate</span>
      </div>
      <div class="stack" style="flex: 1; gap: 16px">
        <div class="field">
          <label for="sk-name">Name</label>
          <input id="sk-name" v-model="name" class="input" maxlength="40" />
        </div>
        <div class="field">
          <span class="label">Arms</span>
          <div class="choices">
            <button type="button" class="choice" :class="{ on: model === 'classic' }" @click="model = 'classic'">Wide (Steve)</button>
            <button type="button" class="choice" :class="{ on: model === 'slim' }" @click="model = 'slim'">Slim (Alex)</button>
          </div>
        </div>
        <div class="field">
          <span class="label">Cape</span>
          <div class="cape-grid">
            <button type="button" class="cape-none" :class="{ on: cape === null }" @click="cape = null"><X :size="18" /> None</button>
            <button v-for="c in capes" :key="c.id" type="button" class="cape-option" :class="{ on: cape === c.id }" :title="c.name" @click="cape = c.id">
              <span class="cape-front" :style="{ backgroundImage: `url(${c.texture})` }" />
            </button>
          </div>
          <span v-if="!capes.length" class="hint">
            Capes come from your Minecraft account. Once Microsoft sign-in is available, the capes you own show up here.
          </span>
        </div>
      </div>
    </div>
    <div class="dialog-footer">
      <button v-if="!skin.default" class="btn btn-ghost" style="margin-right: auto; color: var(--fill-red)" @click="confirmDelete = true"><Trash2 /> Delete</button>
      <button class="btn btn-lg" @click="emit('close')"><X /> Cancel</button>
      <button class="btn btn-primary btn-lg" :disabled="busy" @click="save"><Loader2 v-if="busy" class="spin" /><Save v-else /> Save skin</button>
    </div>
  </AppDialog>

  <AppDialog v-if="confirmDelete" :title="`Delete ${skin.name}?`" :width="400" @close="confirmDelete = false">
    <p class="muted">The skin is removed from your saved skins.</p>
    <div class="dialog-footer">
      <button class="btn" @click="confirmDelete = false">Cancel</button>
      <button class="btn btn-danger" @click="remove">Delete</button>
    </div>
  </AppDialog>
</template>

<style scoped>
.cape-grid {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.cape-none,
.cape-option {
  width: 64px;
  height: 96px;
  border-radius: 12px;
  border: 2px solid var(--border);
  background: var(--muted);
  color: var(--muted-foreground);
  font: inherit;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  display: grid;
  place-items: center;
  align-content: center;
  gap: 4px;
  overflow: hidden;
}
.cape-none.on,
.cape-option.on {
  border-color: var(--tint);
  color: var(--tint);
  background: var(--tint-bg);
}
/* the cape's front: 10×16 pixels at (1,1) of the 64×32 texture */
.cape-front {
  width: 60px;
  height: 96px;
  background-size: 384px 192px;
  background-position: -6px -6px;
  image-rendering: pixelated;
}
</style>
