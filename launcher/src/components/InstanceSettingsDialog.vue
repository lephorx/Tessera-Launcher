<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch, type Component } from "vue";
import { useRouter } from "vue-router";
import { open } from "@tauri-apps/plugin-dialog";
import { ChevronRight, CircleCheck, CircleX, Coffee, FolderSearch, Hammer, Info, Loader2, LogOut, Monitor, RefreshCcw, Trash2, Unlink, UserPlus, Wrench, X } from "lucide-vue-next";
import { api, type Instance, type LoaderVersion, type UnsyncKey } from "@/lib/api";
import { HORIZON_MINECRAFT, LOADERS, horizonBuild } from "@/lib/format";
import { runOf } from "@/lib/store";
import { errorText, toast } from "@/lib/toast";
import AppDialog from "./AppDialog.vue";
import InstanceIcon from "./InstanceIcon.vue";

export type InstanceSection = "general" | "installation" | "java" | "window" | "syncing";

/** An instance's settings as a popup: sections on the left, changes saved as they're made. */
const props = defineProps<{ instance: Instance; section?: InstanceSection }>();
const emit = defineEmits<{ close: []; saved: [Instance]; delete: []; icon: []; share: [] }>();
const router = useRouter();

const horizon = computed(() => props.instance.loader === "horizon");
const joined = computed(() => !!props.instance.joined);
const sections = computed(() => {
  const list: { title: string; items: { id: InstanceSection; label: string; icon: Component }[] }[] = [
    {
      title: "Instance",
      items: [
        { id: "general", label: "General", icon: Info },
        { id: "installation", label: "Installation", icon: Wrench },
      ],
    },
    {
      title: "Game",
      items: [
        ...(horizon.value ? [] : [{ id: "java" as const, label: "Java and memory", icon: Coffee }]),
        { id: "window", label: "Window", icon: Monitor },
        ...(joined.value ? [{ id: "syncing" as const, label: "Syncing", icon: RefreshCcw }] : []),
      ],
    },
  ];
  return list;
});
const section = ref<InstanceSection>(props.section ?? "general");

// ── the settings, saved a moment after each change ──
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
const horizonVersion = ref(props.instance.loader_version ?? "latest");
const horizonBuilds = ref<LoaderVersion[]>([]);
const maxMb = computed(() => Math.max(2048, Math.floor((totalMb.value * 0.75) / 512) * 512));
const memoryWarn = computed(() => customMemory.value && memory.value > totalMb.value * 0.6);
const gb = (mb: number) => (mb / 1024).toFixed(mb % 1024 ? 1 : 0);

onMounted(async () => {
  window.addEventListener("keydown", onKey);
  if (horizon.value) horizonBuilds.value = await api.loaders.versions("horizon", HORIZON_MINECRAFT).catch(() => []);
  totalMb.value = await api.system.memoryMb();
  defaultMb.value = (await api.settings.get()).memory_mb;
  if (!customMemory.value) memory.value = defaultMb.value;
});

let timer: ReturnType<typeof setTimeout> | undefined;
async function save() {
  clearTimeout(timer);
  timer = undefined;
  if (!name.value.trim()) return;
  try {
    const inst = await api.instances.update(props.instance.id, {
      name: name.value,
      memory_mb: customMemory.value ? memory.value : null,
      java_path: javaMode.value === "custom" ? javaPath.value : null,
      jvm_args: jvmArgs.value,
      resolution: customSize.value ? { width: width.value, height: height.value } : null,
      horizon_version: horizon.value ? horizonVersion.value : null,
    });
    emit("saved", inst);
  } catch (e) {
    toast.error(errorText(e));
  }
}
watch([name, customMemory, memory, javaMode, javaPath, jvmArgs, customSize, width, height, horizonVersion], () => {
  clearTimeout(timer);
  timer = setTimeout(save, 600);
});

function close() {
  if (timer) void save();
  emit("close");
}
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape" && !confirmLeave.value) close();
}
onUnmounted(() => {
  window.removeEventListener("keydown", onKey);
  if (timer) void save();
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

// ── installation ──
const run = computed(() => runOf(props.instance.id));
async function repair() {
  try {
    await api.instances.repair(props.instance.id);
  } catch (e) {
    toast.error(errorText(e));
  }
}

const confirmLeave = ref(false);
const leaving = ref(false);
/** Off the pack; with `remove`, this copy goes too. */
async function leave(remove: boolean) {
  leaving.value = true;
  try {
    await api.share.leave(props.instance.id);
    if (remove) {
      await api.instances.remove(props.instance.id);
      toast.success(`Left ${props.instance.name} and deleted your copy`);
      emit("close");
      await router.push("/library");
      return;
    }
    toast.success("Unlinked. Your copy stays as your own instance.");
    confirmLeave.value = false;
    emit("saved", await api.instances.get(props.instance.id));
    section.value = "general";
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    leaving.value = false;
  }
}

// ── syncing ──
const SYNCED: { key: UnsyncKey; title: string; text: string }[] = [
  { key: "options", title: "Unsync game settings", text: "Keep this instance's options.txt (and OptiFine's and the shaders' settings) instead of the pack's." },
  { key: "servers", title: "Unsync multiplayer servers", text: "Don't add the servers linked to the pack to this instance's server list." },
  { key: "resourcepacks", title: "Unsync resource packs", text: "Keep this instance's resource packs instead of the pack's. Turning it back on keeps yours as extra content." },
];
const unsynced = ref(new Set(props.instance.unsynced ?? []));
async function setUnsynced(key: UnsyncKey, on: boolean) {
  try {
    const inst = await api.instances.setUnsynced(props.instance.id, key, on);
    unsynced.value = new Set(inst.unsynced ?? []);
    emit("saved", inst);
  } catch (e) {
    toast.error(errorText(e));
  }
}
</script>

<template>
  <Teleport to="body">
    <div class="backdrop" @mousedown.self="close">
      <div class="dialog settings-dialog" role="dialog" :aria-label="`${instance.name} settings`">
        <div class="dialog-head">
          <div class="row" style="gap: 10px; min-width: 0">
            <InstanceIcon :icon="instance.icon" :id="instance.id" :size="30" />
            <span class="muted ellipsis" style="font-size: 18px; font-weight: 600">{{ instance.name }}</span>
            <ChevronRight :size="18" class="muted" />
            <h2>Settings</h2>
          </div>
          <button class="dialog-close" aria-label="Close" title="Close" @click="close"><X /></button>
        </div>
        <div class="settings-body">
          <nav class="settings-nav">
            <div class="settings-nav-list">
              <template v-for="g in sections" :key="g.title">
                <span class="settings-group">{{ g.title }}</span>
                <button
                  v-for="i in g.items"
                  :key="i.id"
                  class="settings-link"
                  :class="{ on: section === i.id }"
                  :aria-current="section === i.id ? 'page' : undefined"
                  @click="section = i.id"
                >
                  <component :is="i.icon" :size="18" /> {{ i.label }}
                </button>
              </template>
            </div>
          </nav>

          <section v-if="section === 'general'" class="settings-content">
            <div class="set-block">
              <h3 class="set-title">Name</h3>
              <input v-model="name" class="input" aria-label="Name" @blur="name.trim() || (name = instance.name)" />
            </div>
            <div class="set-block">
              <h3 class="set-title">Icon</h3>
              <div class="row">
                <InstanceIcon :icon="instance.icon" :id="instance.id" :size="48" />
                <button class="btn" @click="emit('icon')">Change icon</button>
              </div>
            </div>
            <div class="set-block">
              <h3 class="set-title">Delete instance</h3>
              <p class="set-hint">Deletes it from this computer with its worlds, mods and settings.</p>
              <button class="btn btn-danger" style="width: max-content" @click="emit('delete')"><Trash2 /> Delete instance</button>
            </div>
          </section>

          <section v-else-if="section === 'installation'" class="settings-content">
            <div class="set-block">
              <h3 class="set-title">Installation info</h3>
              <div class="set-table">
                <div class="set-row"><span class="muted" style="flex: 1">Platform</span><b>{{ LOADERS[instance.loader].label }}</b></div>
                <div class="set-row"><span class="muted" style="flex: 1">Game version</span><b class="num">{{ instance.game_version }}</b></div>
                <div v-if="instance.loader_version && !horizon" class="set-row">
                  <span class="muted" style="flex: 1">{{ LOADERS[instance.loader].label }} version</span><b class="num">{{ instance.loader_version }}</b>
                </div>
              </div>
            </div>
            <div v-if="horizon" class="set-block">
              <h3 class="set-title">Horizon version</h3>
              <select v-model="horizonVersion" class="select" aria-label="Horizon version">
                <option value="latest">Newest Nightly (updates itself)</option>
                <option v-if="horizonVersion !== 'latest' && !horizonBuilds.some((b) => b.version === horizonVersion)" :value="horizonVersion">{{ horizonBuild(horizonVersion) }}</option>
                <option v-for="b in horizonBuilds.filter((b) => b.version !== 'latest')" :key="b.version" :value="b.version">{{ horizonBuild(b.version) }}</option>
              </select>
              <p class="set-hint">Horizon's Nightlies stay on GitHub for a week; a build you keep stays installed here. Horizon needs no Java and manages its own memory.</p>
            </div>
            <div v-else class="set-block">
              <h3 class="set-title">Repair instance</h3>
              <button class="btn" style="width: max-content" :disabled="run.state !== 'idle'" @click="repair">
                <Loader2 v-if="run.state === 'preparing'" class="spin" /><Hammer v-else /> {{ run.state === "preparing" ? run.stage || "Repairing" : "Repair" }}
              </button>
              <p class="set-hint">
                Checks every file of Minecraft and {{ instance.loader === "vanilla" ? "its libraries" : LOADERS[instance.loader].label }} against its checksum and
                downloads the broken or missing ones again. Try it when the game won't start for reasons that aren't a mod's.
              </p>
            </div>
            <div v-if="instance.joined" class="set-block">
              <h3 class="set-title">Linked shared instance</h3>
              <button class="btn btn-warn" style="width: max-content" @click="confirmLeave = true"><Unlink /> Unlink shared instance</button>
              <p class="set-hint">Stops this copy from getting the pack's updates{{ instance.joined.role === "editor" ? " and sending yours" : "" }}. It stays as your own instance, or you can delete it.</p>
            </div>
            <div v-else-if="instance.share_code" class="set-block">
              <h3 class="set-title">Shared instance</h3>
              <button class="btn" style="width: max-content" @click="emit('share')"><UserPlus /> Manage sharing</button>
              <p class="set-hint">You share this instance: your changes go to everyone on it.</p>
            </div>
          </section>

          <section v-else-if="section === 'java'" class="settings-content">
            <div class="set-block">
              <h3 class="set-title">Memory</h3>
              <label class="row" style="justify-content: space-between">
                <span><span class="label">Custom memory for this instance</span><span class="set-hint" style="display: block">Otherwise the default from Settings: {{ gb(defaultMb) }} GB</span></span>
                <span class="switch"><input v-model="customMemory" type="checkbox" /><span /></span>
              </label>
              <template v-if="customMemory">
                <div class="row">
                  <input
                    v-model.number="memory"
                    class="range"
                    type="range"
                    min="1024"
                    :max="maxMb"
                    step="512"
                    aria-label="Memory"
                    :style="{ '--range-fill': `${((Math.min(memory, maxMb) - 1024) / (maxMb - 1024)) * 100}%` }"
                  />
                  <span class="mono num" style="width: 72px; text-align: right; font-weight: 600">{{ gb(memory) }} GB</span>
                </div>
                <span class="set-hint num">This computer has {{ gb(totalMb) }} GB.</span>
                <span v-if="memoryWarn" class="error-text">That's a lot for one game: the rest of your computer may slow down.</span>
              </template>
            </div>
            <div class="set-block">
              <h3 class="set-title">Java</h3>
              <div class="choices">
                <button type="button" class="choice" :class="{ on: javaMode === 'auto' }" @click="javaMode = 'auto'">Automatic</button>
                <button type="button" class="choice" :class="{ on: javaMode === 'custom' }" @click="javaMode = 'custom'">Custom</button>
              </div>
              <p v-if="javaMode === 'auto'" class="set-hint">Tessera downloads and uses the Java version Mojang lists for Minecraft {{ instance.game_version }}. Recommended.</p>
              <template v-else>
                <div class="row">
                  <button type="button" class="btn btn-icon" title="Browse" aria-label="Browse for Java" @click="browseJava"><FolderSearch /></button>
                  <input v-model="javaPath" class="input mono" placeholder="/Library/Java/JavaVirtualMachines/…/bin/java" aria-label="Java path" @change="testJava" />
                  <button type="button" class="btn" :disabled="!javaPath || checking" @click="testJava"><Loader2 v-if="checking" class="spin" />Test</button>
                </div>
                <p v-if="javaCheck" class="row set-hint" :style="{ color: javaCheck.ok ? 'var(--status-green)' : 'var(--destructive)' }">
                  <CircleCheck v-if="javaCheck.ok" :size="15" /><CircleX v-else :size="15" />{{ javaCheck.text }}
                </p>
              </template>
            </div>
            <div class="set-block">
              <h3 class="set-title">Extra JVM arguments</h3>
              <input v-model="jvmArgs" class="input mono" placeholder="-XX:+UseG1GC -Dfml.ignorePatchDiscrepancies=true" aria-label="Extra JVM arguments" />
              <p class="set-hint">Space separated. Leave empty unless a modpack asks for them.</p>
            </div>
          </section>

          <section v-else-if="section === 'window'" class="settings-content">
            <div class="set-block">
              <h3 class="set-title">Window</h3>
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
            </div>
          </section>

          <section v-else class="settings-content">
            <p class="set-hint">Turn one on to keep that part of this instance to yourself: the pack's updates leave it alone{{ instance.joined?.role === "editor" ? ", and what you change there isn't sent to the others" : "" }}.</p>
            <div class="set-table">
              <label v-for="s in SYNCED" :key="s.key" class="set-row" style="gap: 16px">
                <span style="flex: 1">
                  <span class="label" style="display: block">{{ s.title }}</span>
                  <span class="set-hint">{{ s.text }}</span>
                </span>
                <span class="switch">
                  <input type="checkbox" :checked="unsynced.has(s.key)" :aria-label="s.title" @change="setUnsynced(s.key, ($event.target as HTMLInputElement).checked)" /><span />
                </span>
              </label>
            </div>
          </section>
        </div>
      </div>
    </div>

    <AppDialog v-if="confirmLeave" title="Unlink this shared instance?" @close="confirmLeave = false">
      <p class="muted">
        You leave the pack: no more updates{{ instance.joined?.role === "editor" ? ", and your changes don't reach the others" : "" }}, and its servers leave your server
        list. Keep your copy as your own instance, or delete it with its worlds.
      </p>
      <div class="dialog-footer">
        <button class="btn" @click="confirmLeave = false">Cancel</button>
        <button class="btn" :disabled="leaving" @click="leave(false)"><Loader2 v-if="leaving" class="spin" /><LogOut v-else /> Unlink, keep my copy</button>
        <button class="btn btn-danger" :disabled="leaving" @click="leave(true)"><Trash2 /> Unlink and delete</button>
      </div>
    </AppDialog>
  </Teleport>
</template>

<style scoped>
html .btn.btn-warn {
  background: var(--fill-orange);
  border-color: transparent;
  color: #1b1206;
}
html .btn.btn-warn:hover {
  filter: brightness(1.06);
}
</style>
