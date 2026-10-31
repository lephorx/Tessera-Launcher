<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import { Search, Send } from "lucide-vue-next";
import { api, type EasyServer } from "@/lib/api";
import { errorText, toast } from "@/lib/toast";

/** The server's console: what it writes, filtered by level or text, and commands (↑/↓ for earlier ones). */
const props = defineProps<{ server: EasyServer }>();

const lines = ref<string[]>([]);
const command = ref("");
const filter = ref("");
const level = ref<"all" | "error" | "warn" | "info">("all");
const history = ref<string[]>([]);
let back = -1;
const el = ref<HTMLElement | null>(null);

const ERROR = /\b(ERROR|FATAL|SEVERE)\]|\[(ERROR|FATAL|SEVERE)\b|\b(ERROR|FATAL)\b.*:|Exception/;
const WARN = /\bWARN(ING)?\]|\[WARN(ING)?\b|\bWARN\b/;
const levelOf = (l: string) => (ERROR.test(l) ? "error" : WARN.test(l) ? "warn" : "info");

const shown = computed(() => {
  const q = filter.value.trim().toLowerCase();
  return lines.value.filter((l) => (level.value === "all" || levelOf(l) === level.value) && (!q || l.toLowerCase().includes(q)));
});

async function scrollDown(force = false) {
  await nextTick();
  const c = el.value;
  if (c && (force || c.scrollHeight - c.scrollTop - c.clientHeight < 80)) c.scrollTop = c.scrollHeight;
}

let unlisten: (() => void) | undefined;
onMounted(async () => {
  lines.value = await api.easy.log(props.server.id);
  void scrollDown(true);
  unlisten = await listen<{ id: string; line: string | null }>("easy-server", (e) => {
    if (e.payload.id !== props.server.id || e.payload.line === null) return;
    if (e.payload.line.startsWith("[Tessera] Starting")) lines.value = [];
    lines.value.push(e.payload.line);
    if (lines.value.length > 3000) lines.value.splice(0, lines.value.length - 3000);
    void scrollDown();
  });
});
onUnmounted(() => unlisten?.());

const ready = computed(() => props.server.state === "running");

async function send() {
  const line = command.value.trim();
  if (!line) return;
  try {
    await api.easy.send(props.server.id, line);
    history.value = [line, ...history.value.filter((h) => h !== line)].slice(0, 100);
    back = -1;
    command.value = "";
    void scrollDown(true);
  } catch (e) {
    toast.error(errorText(e));
  }
}

function key(e: KeyboardEvent) {
  if (e.key === "ArrowUp" && back < history.value.length - 1) {
    back++;
    command.value = history.value[back]!;
    e.preventDefault();
  } else if (e.key === "ArrowDown") {
    back = Math.max(-1, back - 1);
    command.value = back < 0 ? "" : history.value[back]!;
    e.preventDefault();
  }
}
</script>

<template>
  <div class="stack" style="gap: 10px">
    <div class="row" style="flex-wrap: wrap">
      <div class="search" style="flex: 1; min-width: 200px">
        <Search :size="16" />
        <input v-model="filter" class="input" placeholder="Search the console" />
      </div>
      <select v-model="level" class="select" style="width: 150px" aria-label="Level">
        <option value="all">Everything</option>
        <option value="error">Errors</option>
        <option value="warn">Warnings</option>
        <option value="info">Info</option>
      </select>
    </div>
    <div ref="el" class="console">
      <div v-for="(l, i) in shown" :key="i" :class="[levelOf(l), { cmd: l.startsWith('> ') }]">{{ l }}</div>
      <span v-if="!lines.length" style="opacity: 0.6">{{ server.state === "stopped" ? "The server isn't running." : "Starting…" }}</span>
    </div>
    <form class="row" @submit.prevent="send">
      <input
        v-model="command"
        class="input mono"
        :disabled="!ready"
        :placeholder="ready ? 'Command, e.g. op YourName (↑ for earlier ones)' : server.state === 'starting' ? 'Commands work once it has started' : 'Start the server to send commands'"
        style="flex: 1"
        @keydown="key"
      />
      <button class="btn" :disabled="!ready || !command.trim()"><Send /> Send</button>
    </form>
  </div>
</template>

<style scoped>
.console .cmd {
  color: #9db4ff;
}
.search {
  position: relative;
}
.search svg {
  position: absolute;
  left: 12px;
  top: 50%;
  transform: translateY(-50%);
  opacity: 0.6;
}
.search .input {
  padding-left: 36px;
  width: 100%;
}
</style>
