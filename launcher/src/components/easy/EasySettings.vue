<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { ImagePlus, Loader2, RefreshCw, Save, Trash2, X } from "lucide-vue-next";
import AppDialog from "@/components/AppDialog.vue";
import { api, type EasyKind, type EasyServer, type GameVersion, type LoaderVersion } from "@/lib/api";
import { KIND } from "@/lib/easy";
import { errorText, toast } from "@/lib/toast";

/** The server's settings: name, MOTD, icon, gameplay, memory and port; its type and version; deleting it. */
const props = defineProps<{ server: EasyServer; icon: string | null }>();
const emit = defineEmits<{ changed: []; delete: [] }>();

const form = ref({ name: "", motd: "", memGb: 4, port: 25565, maxPlayers: 20, difficulty: "easy", gamemode: "survival", pvp: true });
const saving = ref(false);
const totalMb = ref(16384);
const maxGb = computed(() => Math.max(2, Math.floor((totalMb.value / 1024) * 0.75)));

onMounted(async () => {
  totalMb.value = await api.system.memoryMb().catch(() => 16384);
  const p = await api.easy.properties(props.server.id).catch(() => ({}) as Record<string, string>);
  form.value = {
    name: props.server.name,
    motd: p["motd"] ?? props.server.name,
    memGb: Math.round(props.server.memory_mb / 1024),
    port: props.server.port,
    maxPlayers: Number(p["max-players"] ?? 20),
    difficulty: p["difficulty"] ?? "easy",
    gamemode: p["gamemode"] ?? "survival",
    pvp: (p["pvp"] ?? "true") === "true",
  };
  const v = await api.versions.game();
  versions.value = v.versions;
});

async function save() {
  saving.value = true;
  try {
    const f = form.value;
    const s = await api.easy.update(props.server.id, {
      name: f.name,
      motd: f.motd,
      memory_mb: f.memGb * 1024,
      port: f.port,
      max_players: f.maxPlayers,
      difficulty: f.difficulty,
      gamemode: f.gamemode,
      pvp: f.pvp,
    });
    toast.success(s.state === "stopped" ? "Saved" : "Saved. Restart the server to use the new settings.");
    emit("changed");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    saving.value = false;
  }
}

// ── MOTD: Minecraft's § colour codes ──

const COLORS: Record<string, string> = {
  "0": "#000000", "1": "#0000aa", "2": "#00aa00", "3": "#00aaaa", "4": "#aa0000", "5": "#aa00aa", "6": "#ffaa00", "7": "#aaaaaa",
  "8": "#555555", "9": "#5555ff", a: "#55ff55", b: "#55ffff", c: "#ff5555", d: "#ff55ff", e: "#ffff55", f: "#ffffff",
};
const motdEl = ref<HTMLTextAreaElement | null>(null);

function preview(text: string) {
  const out: { text: string; style: Record<string, string> }[] = [];
  let style: Record<string, string> = { color: "#aaaaaa" };
  const parts = text.split(/(§[0-9a-fk-or])/i);
  for (const p of parts) {
    const m = /^§([0-9a-fk-or])$/i.exec(p);
    if (!m) {
      if (p) out.push({ text: p, style: { ...style } });
      continue;
    }
    const c = m[1]!.toLowerCase();
    if (COLORS[c]) style = { color: COLORS[c]! };
    else if (c === "l") style = { ...style, fontWeight: "800" };
    else if (c === "o") style = { ...style, fontStyle: "italic" };
    else if (c === "n") style = { ...style, textDecoration: "underline" };
    else if (c === "m") style = { ...style, textDecoration: "line-through" };
    else if (c === "r") style = { color: "#aaaaaa" };
  }
  return out;
}

function insert(code: string) {
  const el = motdEl.value;
  const f = form.value;
  const at = el?.selectionStart ?? f.motd.length;
  f.motd = f.motd.slice(0, at) + code + f.motd.slice(el?.selectionEnd ?? at);
  requestAnimationFrame(() => {
    el?.focus();
    el?.setSelectionRange(at + code.length, at + code.length);
  });
}

// ── icon ──

const fileInput = ref<HTMLInputElement | null>(null);

/** Any picture, scaled and cropped to 64×64 (Minecraft wants exactly that). */
async function pickIcon(e: Event) {
  const file = (e.target as HTMLInputElement).files?.[0];
  (e.target as HTMLInputElement).value = "";
  if (!file) return;
  try {
    const img = await createImageBitmap(file);
    const c = document.createElement("canvas");
    c.width = c.height = 64;
    const ctx = c.getContext("2d")!;
    const side = Math.min(img.width, img.height);
    ctx.imageSmoothingEnabled = side > 64;
    ctx.drawImage(img, (img.width - side) / 2, (img.height - side) / 2, side, side, 0, 0, 64, 64);
    const blob = await new Promise<Blob | null>((r) => c.toBlob(r, "image/png"));
    if (!blob) throw new Error("Couldn't read the picture");
    await api.easy.setIcon(props.server.id, [...new Uint8Array(await blob.arrayBuffer())]);
    toast.success(props.server.state === "stopped" ? "Icon saved" : "Icon saved. It shows after a restart.");
    emit("changed");
  } catch (err) {
    toast.error(errorText(err));
  }
}

async function removeIcon() {
  await api.easy.setIcon(props.server.id, null).catch((e) => toast.error(errorText(e)));
  emit("changed");
}

// ── type and version ──

const versions = ref<GameVersion[]>([]);
const releases = computed(() => versions.value.filter((v) => v.kind === "release"));
const kind = ref<EasyKind>(props.server.kind);
const game = ref(props.server.game_version);
const loaderVersions = ref<LoaderVersion[]>([]);
const loaderVersion = ref<string>("");
const changing = ref(false);
const confirmChange = ref(false);

const LOADER: Partial<Record<EasyKind, "fabric" | "forge" | "neoforge">> = { fabric: "fabric", forge: "forge", neoforge: "neoforge" };
watch(
  [kind, game],
  async () => {
    loaderVersions.value = [];
    loaderVersion.value = kind.value === props.server.kind && game.value === props.server.game_version ? (props.server.loader_version ?? "") : "";
    const l = LOADER[kind.value];
    if (!l || !game.value) return;
    try {
      loaderVersions.value = await api.loaders.versions(l, game.value);
    } catch {
      loaderVersions.value = [];
    }
  },
  { immediate: true },
);

const same = computed(() => kind.value === props.server.kind && game.value === props.server.game_version && (loaderVersion.value || null) === (LOADER[kind.value] ? props.server.loader_version : null));
const older = computed(() => {
  const ids = releases.value.map((v) => v.id);
  const a = ids.indexOf(game.value);
  const b = ids.indexOf(props.server.game_version);
  return a >= 0 && b >= 0 && a > b;
});
const family = (k: EasyKind) => (k === "forge" || k === "neoforge" ? "forge" : k);
const modsMove = computed(() => family(kind.value) !== family(props.server.kind) && props.server.kind !== "vanilla");

async function change() {
  confirmChange.value = false;
  changing.value = true;
  try {
    await api.easy.change(props.server.id, { kind: kind.value, game_version: game.value, loader_version: loaderVersion.value || null });
    toast.success(`Now ${KIND[kind.value]} ${game.value}`);
    emit("changed");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    changing.value = false;
  }
}
</script>

<template>
  <div class="stack" style="gap: 16px">
    <form class="card stack" style="gap: 16px" @submit.prevent="save">
      <h3>General</h3>
      <div class="row" style="gap: 16px; align-items: flex-start">
        <div class="stack" style="gap: 6px; align-items: center">
          <div class="icon-box">
            <img v-if="icon" :src="icon" width="64" height="64" alt="" />
            <ImagePlus v-else :size="24" style="opacity: 0.5" />
          </div>
          <div class="row" style="gap: 4px">
            <button type="button" class="btn btn-sm" @click="fileInput?.click()">{{ icon ? "Change" : "Icon" }}</button>
            <button v-if="icon" type="button" class="btn btn-ghost btn-icon btn-sm" title="Remove the icon" @click="removeIcon"><X /></button>
          </div>
          <input ref="fileInput" type="file" accept="image/*" hidden @change="pickIcon" />
        </div>
        <div class="stack" style="gap: 12px; flex: 1; min-width: 0">
          <div class="field"><label for="s-name">Name in the launcher</label><input id="s-name" v-model="form.name" class="input" maxlength="48" /></div>
          <div class="field">
            <label for="s-motd">Message in the server list (MOTD)</label>
            <textarea id="s-motd" ref="motdEl" v-model="form.motd" class="input mono" rows="2" maxlength="200" style="resize: none" />
            <div class="row" style="flex-wrap: wrap; gap: 3px; margin-top: 6px">
              <button v-for="(c, k) in COLORS" :key="k" type="button" class="swatch" :style="{ background: c }" :title="`§${k}`" @click="insert(`§${k}`)" />
              <button type="button" class="btn btn-sm" style="font-weight: 800" title="Bold" @click="insert('§l')">B</button>
              <button type="button" class="btn btn-sm" style="font-style: italic" title="Italic" @click="insert('§o')">I</button>
              <button type="button" class="btn btn-sm" title="Reset" @click="insert('§r')">Reset</button>
            </div>
            <div class="motd-preview">
              <img v-if="icon" :src="icon" width="40" height="40" alt="" />
              <div class="stack" style="gap: 0; min-width: 0">
                <b style="color: #fff">{{ form.name }}</b>
                <div v-for="(line, i) in form.motd.split('\n').slice(0, 2)" :key="i" class="ellipsis"><span v-for="(s, j) in preview(line)" :key="j" :style="s.style">{{ s.text }}</span></div>
              </div>
            </div>
          </div>
        </div>
      </div>
      <div class="form-grid">
        <div class="field"><label for="s-max">Max players</label><input id="s-max" v-model.number="form.maxPlayers" class="input num" type="number" min="1" max="1000" /></div>
        <div class="field">
          <label for="s-diff">Difficulty</label>
          <select id="s-diff" v-model="form.difficulty" class="select"><option value="peaceful">Peaceful</option><option value="easy">Easy</option><option value="normal">Normal</option><option value="hard">Hard</option></select>
        </div>
        <div class="field">
          <label for="s-mode">Game mode</label>
          <select id="s-mode" v-model="form.gamemode" class="select"><option value="survival">Survival</option><option value="creative">Creative</option><option value="adventure">Adventure</option><option value="spectator">Spectator</option></select>
        </div>
        <div class="field"><label for="s-port">Port</label><input id="s-port" v-model.number="form.port" class="input num" type="number" min="1024" max="65535" /></div>
        <div class="field" style="grid-column: span 2">
          <label for="s-mem">Memory</label>
          <div class="row">
            <input id="s-mem" v-model.number="form.memGb" class="range" type="range" min="1" :max="maxGb" step="1" style="flex: 1" :style="{ '--range-fill': `${((form.memGb - 1) / (maxGb - 1)) * 100}%` }" />
            <span class="mono num" style="width: 48px; text-align: right; font-weight: 600">{{ form.memGb }} GB</span>
          </div>
        </div>
      </div>
      <label class="row" style="gap: 8px"><input v-model="form.pvp" type="checkbox" /> Players can hurt each other (PvP)</label>
      <div class="row">
        <button class="btn btn-primary" :disabled="saving"><Loader2 v-if="saving" class="spin" /><Save v-else /> Save</button>
        <span class="hint">Changes apply the next time the server starts. Every other setting is under Properties.</span>
      </div>
    </form>

    <div class="card stack" style="gap: 14px">
      <div class="stack" style="gap: 2px">
        <h3>Type and version</h3>
        <span class="hint">Now {{ KIND[server.kind] }} {{ server.game_version }}{{ server.loader_version ? ` (${server.loader_version})` : "" }}. The world and settings stay; a backup is made first.</span>
      </div>
      <div class="form-grid">
        <div class="field">
          <label for="c-kind">Type</label>
          <select id="c-kind" v-model="kind" class="select">
            <option v-for="(label, k) in KIND" :key="k" :value="k">{{ label }}</option>
          </select>
        </div>
        <div class="field">
          <label for="c-game">Minecraft version</label>
          <select id="c-game" v-model="game" class="select">
            <option v-for="v in releases" :key="v.id" :value="v.id">{{ v.id }}</option>
          </select>
        </div>
        <div v-if="LOADER[kind]" class="field">
          <label for="c-loader">{{ KIND[kind] }} version</label>
          <select id="c-loader" v-model="loaderVersion" class="select">
            <option value="">Newest</option>
            <option v-for="l in loaderVersions" :key="l.version" :value="l.version">{{ l.version }}{{ l.stable ? "" : " (beta)" }}</option>
          </select>
        </div>
      </div>
      <p v-if="older" class="hint" style="margin: 0; color: var(--fill-orange)">{{ game }} is older than {{ server.game_version }}. Minecraft can't open a world in an older version; it may break. The backup made first keeps it safe.</p>
      <p v-if="modsMove" class="hint" style="margin: 0">{{ KIND[server.kind] }}'s mods and plugins don't work with {{ KIND[kind] }}: they're moved to a "-{{ server.kind }}" folder next to them, not deleted.</p>
      <div class="row">
        <button class="btn" :disabled="same || changing || server.state !== 'stopped'" :title="server.state !== 'stopped' ? 'Stop the server first' : ''" @click="confirmChange = true">
          <Loader2 v-if="changing" class="spin" /><RefreshCw v-else /> Switch to {{ KIND[kind] }} {{ game }}
        </button>
        <span v-if="server.state !== 'stopped'" class="hint">Stop the server first.</span>
      </div>
    </div>

    <div class="card row" style="gap: 12px">
      <div class="stack" style="gap: 2px; flex: 1">
        <h3>Delete server</h3>
        <span class="hint">The server, its world and its backups are deleted from this computer.</span>
      </div>
      <button class="btn btn-danger" @click="emit('delete')"><Trash2 /> Delete</button>
    </div>

    <AppDialog v-if="confirmChange" :title="`Switch to ${KIND[kind]} ${game}?`" :width="440" @close="confirmChange = false">
      <p class="muted">The launcher backs up the server, then installs {{ KIND[kind] }} {{ game }}. The world, settings and players stay.</p>
      <p v-if="older" class="muted" style="color: var(--fill-orange)">Going back to an older Minecraft can break the world. If it does, restore the backup.</p>
      <div class="dialog-footer">
        <button class="btn" @click="confirmChange = false">Cancel</button>
        <button class="btn btn-primary" @click="change">Switch</button>
      </div>
    </AppDialog>
  </div>
</template>

<style scoped>
h3 {
  font-size: 15px;
  font-weight: 700;
}
.form-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 14px;
}
.icon-box {
  width: 72px;
  height: 72px;
  border-radius: 14px;
  background: var(--muted);
  display: grid;
  place-items: center;
  overflow: hidden;
}
.icon-box img {
  image-rendering: pixelated;
}
.swatch {
  width: 20px;
  height: 20px;
  border-radius: 5px;
  border: 1px solid var(--border);
  cursor: pointer;
}
.motd-preview {
  display: flex;
  gap: 10px;
  align-items: center;
  margin-top: 8px;
  padding: 8px 10px;
  border-radius: 8px;
  background: #1d1d1d;
  font-family: var(--font-mono);
  font-size: 13px;
}
.motd-preview img {
  image-rendering: pixelated;
}
</style>
