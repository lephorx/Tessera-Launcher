<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { Loader2, RotateCcw, Save } from "lucide-vue-next";
import { api, type EasyServer } from "@/lib/api";
import { errorText, toast } from "@/lib/toast";

/** Every key in server.properties. The port and RCON are the launcher's (Settings has the port). */
const props = defineProps<{ server: EasyServer }>();

const original = ref<Record<string, string> | null>(null);
const values = ref<Record<string, string>>({});
const filter = ref("");
const saving = ref(false);

async function load() {
  try {
    original.value = await api.easy.properties(props.server.id);
    values.value = { ...original.value };
  } catch (e) {
    toast.error(errorText(e));
  }
}
onMounted(load);

const keys = computed(() => {
  const q = filter.value.trim().toLowerCase();
  return Object.keys(values.value)
    .filter((k) => !q || k.includes(q) || values.value[k]!.toLowerCase().includes(q))
    .sort();
});
const changed = computed(() => Object.keys(values.value).filter((k) => values.value[k] !== original.value?.[k]));
const isBool = (k: string) => original.value?.[k] === "true" || original.value?.[k] === "false";

async function save() {
  saving.value = true;
  try {
    await api.easy.setProperties(props.server.id, Object.fromEntries(changed.value.map((k) => [k, values.value[k]!])));
    await load();
    toast.success(props.server.state === "stopped" ? "Saved" : "Saved. Restart the server to use the new settings.");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div class="card stack" style="gap: 12px">
    <div class="row" style="flex-wrap: wrap">
      <input v-model="filter" class="input" placeholder="Search properties" style="flex: 1; min-width: 200px" />
      <span v-if="changed.length" class="hint">{{ changed.length }} changed</span>
      <button class="btn" :disabled="!changed.length" @click="values = { ...original! }"><RotateCcw /> Reset</button>
      <button class="btn btn-primary" :disabled="!changed.length || saving" @click="save"><Loader2 v-if="saving" class="spin" /><Save v-else /> Save</button>
    </div>
    <p v-if="original && !Object.keys(original).length" class="hint" style="margin: 0">The server writes its properties the first time it starts.</p>
    <div v-if="original" class="props">
      <label v-for="k in keys" :key="k" class="prop" :class="{ changed: values[k] !== original[k] }">
        <span class="mono key">{{ k }}</span>
        <select v-if="isBool(k)" v-model="values[k]" class="select">
          <option value="true">true</option>
          <option value="false">false</option>
        </select>
        <input v-else v-model="values[k]" class="input mono" />
      </label>
    </div>
    <div v-else class="empty"><Loader2 class="spin" /></div>
    <p class="hint" style="margin: 0">The port is in Settings; RCON is how the launcher sends commands, so it isn't listed.</p>
  </div>
</template>

<style scoped>
.props {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(360px, 1fr));
  gap: 6px 16px;
}
.prop {
  display: grid;
  grid-template-columns: 1fr 1fr;
  align-items: center;
  gap: 10px;
  padding: 4px 8px;
  border-radius: 10px;
}
.prop.changed {
  background: var(--tint-bg);
}
.key {
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
