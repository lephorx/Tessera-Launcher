<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { CircleCheck, CircleX, FolderSearch, Loader2, Trash2 } from "lucide-vue-next";
import { api, type Instance } from "@/lib/api";
import { errorText, toast } from "@/lib/toast";

/** Per-instance settings: name, memory, Java, JVM arguments, window size. */
const props = defineProps<{ instance: Instance }>();
const emit = defineEmits<{ saved: [Instance]; delete: [] }>();

const name = ref(props.instance.name);
const customMemory = ref(props.instance.memory_mb !== null);
const memory = ref(props.instance.memory_mb ?? 4096);
const totalMb = ref(16384);
const defaultMb = ref(4096);
const javaMode = ref<"auto" | "custom">(props.instance.java_path ? "custom" : "auto");
const javaPath = ref(props.instance.java_path ?? "");
const javaCheck = ref<{ ok: boolean; text: string } | null>(null);
const checking = ref(false);
const jvmArgs = ref(props.instance.jvm_args ?? "");
const customSize = ref(props.instance.resolution !== null);
const width = ref(props.instance.resolution?.width ?? 1280);
const height = ref(props.instance.resolution?.height ?? 720);
const saving = ref(false);

// keep a quarter of the RAM for the system; steps of 512 MB
const maxMb = computed(() => Math.max(2048, Math.floor((totalMb.value * 0.75) / 512) * 512));
const memoryWarn = computed(() => customMemory.value && memory.value > totalMb.value * 0.6);

onMounted(async () => {
  totalMb.value = await api.system.memoryMb();
  defaultMb.value = (await api.settings.get()).memory_mb;
  if (!customMemory.value) memory.value = defaultMb.value;
});

async function browseJava() {
  const picked = await open({ multiple: false, title: "Choose a java executable" });
  if (typeof picked === "string") {
    javaPath.value = picked;
    await testJava();
  }
}

async function testJava() {
  checking.value = true;
  try {
    javaCheck.value = { ok: true, text: await api.system.checkJava(javaPath.value) };
  } catch (e) {
    javaCheck.value = { ok: false, text: errorText(e) };
  } finally {
    checking.value = false;
  }
}

async function save() {
  saving.value = true;
  try {
    const inst = await api.instances.update(props.instance.id, {
      name: name.value,
      memory_mb: customMemory.value ? memory.value : null,
      java_path: javaMode.value === "custom" ? javaPath.value : null,
      jvm_args: jvmArgs.value,
      resolution: customSize.value ? { width: width.value, height: height.value } : null,
    });
    emit("saved", inst);
    toast.success("Saved");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    saving.value = false;
  }
}

const gb = (mb: number) => (mb / 1024).toFixed(mb % 1024 ? 1 : 0);
</script>

<template>
  <div class="stack" style="max-width: 640px; gap: 16px">
    <section class="card stack">
      <h3 class="opt-title">General</h3>
      <div class="field">
        <label for="opt-name">Name</label>
        <input id="opt-name" v-model="name" class="input" />
      </div>
    </section>

    <section class="card stack">
      <h3 class="opt-title">Memory</h3>
      <label class="row" style="justify-content: space-between">
        <span><span class="label">Custom memory for this instance</span><span class="hint" style="display: block">Otherwise the default from Settings: {{ gb(defaultMb) }} GB</span></span>
        <span class="switch"><input v-model="customMemory" type="checkbox" /><span /></span>
      </label>
      <div v-if="customMemory" class="stack" style="gap: 6px">
        <div class="row">
          <input
            v-model.number="memory"
            class="range"
            type="range"
            min="1024"
            :max="maxMb"
            step="512"
            :style="{ '--range-fill': `${((Math.min(memory, maxMb) - 1024) / (maxMb - 1024)) * 100}%` }"
          />
          <span class="mono num" style="width: 72px; text-align: right; font-weight: 600">{{ gb(memory) }} GB</span>
        </div>
        <span class="hint num">This computer has {{ gb(totalMb) }} GB.</span>
        <span v-if="memoryWarn" class="error-text">That's a lot for one game: the rest of your computer may slow down.</span>
      </div>
    </section>

    <section class="card stack">
      <h3 class="opt-title">Java</h3>
      <div class="choices">
        <button type="button" class="choice" :class="{ on: javaMode === 'auto' }" @click="javaMode = 'auto'">Automatic</button>
        <button type="button" class="choice" :class="{ on: javaMode === 'custom' }" @click="javaMode = 'custom'">Custom</button>
      </div>
      <p v-if="javaMode === 'auto'" class="hint">Tessera downloads and uses the Java version Mojang lists for Minecraft {{ instance.game_version }}. Recommended.</p>
      <template v-else>
        <div class="row">
          <button type="button" class="btn btn-icon" title="Browse" aria-label="Browse for Java" @click="browseJava"><FolderSearch /></button>
          <input v-model="javaPath" class="input mono" placeholder="/Library/Java/JavaVirtualMachines/…/bin/java" aria-label="Java path" @change="testJava" />
          <button type="button" class="btn" :disabled="!javaPath || checking" @click="testJava"><Loader2 v-if="checking" class="spin" />Test</button>
        </div>
        <p v-if="javaCheck" class="row hint" :style="{ color: javaCheck.ok ? 'var(--status-green)' : 'var(--destructive)' }">
          <CircleCheck v-if="javaCheck.ok" :size="15" /><CircleX v-else :size="15" />{{ javaCheck.text }}
        </p>
      </template>
      <div class="field">
        <label for="opt-jvm">Extra JVM arguments</label>
        <input id="opt-jvm" v-model="jvmArgs" class="input mono" placeholder="-XX:+UseG1GC -Dfml.ignorePatchDiscrepancies=true" />
        <span class="hint">Space separated. Leave empty unless a modpack asks for them.</span>
      </div>
    </section>

    <section class="card stack">
      <h3 class="opt-title">Window</h3>
      <label class="row" style="justify-content: space-between">
        <span class="label">Custom window size</span>
        <span class="switch"><input v-model="customSize" type="checkbox" /><span /></span>
      </label>
      <div v-if="customSize" class="row">
        <input v-model.number="width" type="number" min="320" class="input num" style="width: 120px" aria-label="Width" />
        <span class="hint">×</span>
        <input v-model.number="height" type="number" min="240" class="input num" style="width: 120px" aria-label="Height" />
        <span class="hint">pixels</span>
      </div>
    </section>

    <div class="row">
      <button class="btn btn-primary btn-lg" :disabled="saving" @click="save"><Loader2 v-if="saving" class="spin" />Save</button>
      <span class="spacer" />
      <button class="btn btn-danger btn-lg" @click="emit('delete')"><Trash2 /> Delete instance</button>
    </div>
  </div>
</template>

<style scoped>
.opt-title {
  font-size: 15px;
  font-weight: 700;
}
</style>
