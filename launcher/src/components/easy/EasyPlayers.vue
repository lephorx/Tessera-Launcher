<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { Ban, Crown, Loader2, Plus, ShieldCheck, Users, X } from "lucide-vue-next";
import EasyPlayerDialog from "./EasyPlayerDialog.vue";
import { api, type EasyInsights, type EasyListKind, type EasyLists, type EasyServer } from "@/lib/api";
import { duration, head, known, NAME } from "@/lib/easy";
import { relative } from "@/lib/format";
import { errorText, toast } from "@/lib/toast";

/** Players: activity, who's on, the whitelist, operators and bans, and everyone who's played. */
const props = defineProps<{ server: EasyServer }>();
const emit = defineEmits<{ changed: [] }>();

const insights = ref<EasyInsights | null>(null);
const lists = ref<EasyLists | null>(null);
const open = ref<string | null>(null);
const adding = ref<Record<EasyListKind, string>>({ whitelist: "", ops: "", banned: "" });
const busy = ref<string | null>(null);
const search = ref("");

async function load() {
  try {
    [insights.value, lists.value] = await Promise.all([api.easy.insights(props.server.id), api.easy.lists(props.server.id)]);
  } catch (e) {
    toast.error(errorText(e));
  }
}
onMounted(load);
// joins and leaves change the history
watch(() => props.server.players.join(","), load);
const timer = setInterval(load, 30000);
onUnmounted(() => clearInterval(timer));

const now = ref(Date.now());
const tick = setInterval(() => (now.value = Date.now()), 30000);
onUnmounted(() => clearInterval(tick));

/** Seconds played, finished sessions plus the one going on. */
function played(p: { playtime: number; online_since: string | null }) {
  return p.playtime + (p.online_since ? (now.value - new Date(p.online_since).getTime()) / 1000 : 0);
}

const total = computed(() => insights.value?.players.reduce((n, p) => n + played(p), 0) ?? 0);
const everyone = computed(() => {
  const q = search.value.trim().toLowerCase();
  return (insights.value?.players ?? []).filter((p) => !q || p.name.toLowerCase().includes(q));
});

/** Hours played per day, the last 14 days (sessions cut at midnight). */
const days = computed(() => {
  const out: { label: string; hours: number; players: number }[] = [];
  const start = new Date(now.value);
  start.setHours(0, 0, 0, 0);
  start.setDate(start.getDate() - 13);
  const spans = [
    ...(insights.value?.sessions ?? []).map((s) => ({ name: s.name, a: new Date(s.joined).getTime(), b: new Date(s.left).getTime() })),
    ...(insights.value?.players ?? []).filter((p) => p.online_since).map((p) => ({ name: p.name, a: new Date(p.online_since!).getTime(), b: now.value })),
  ];
  for (let i = 0; i < 14; i++) {
    const a = new Date(start);
    a.setDate(a.getDate() + i);
    const b = new Date(a);
    b.setDate(b.getDate() + 1);
    let ms = 0;
    const who = new Set<string>();
    for (const s of spans) {
      const overlap = Math.min(s.b, b.getTime()) - Math.max(s.a, a.getTime());
      if (overlap > 0) {
        ms += overlap;
        who.add(s.name.toLowerCase());
      }
    }
    out.push({ label: a.toLocaleDateString(undefined, { weekday: "short", day: "numeric" }), hours: ms / 3600000, players: who.size });
  }
  return out;
});
const most = computed(() => Math.max(1, ...days.value.map((d) => d.hours)));

async function change(list: EasyListKind, name: string, add: boolean) {
  if (add && !NAME.test(name)) {
    toast.error("That isn't a Minecraft name (3–16 letters, digits or _)");
    return;
  }
  busy.value = `${list}:${name}`;
  try {
    await api.easy.listChange(props.server.id, list, name, add);
    if (add) adding.value[list] = "";
    await load();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = null;
  }
}

async function toggleWhitelist() {
  if (!lists.value) return;
  try {
    await api.easy.update(props.server.id, { whitelist: !lists.value.whitelist_on });
    await load();
    toast.success(props.server.state === "stopped" ? "Saved" : "Saved. Restart the server to use it.");
    emit("changed");
  } catch (e) {
    toast.error(errorText(e));
  }
}

const LISTS: { id: EasyListKind; title: string; icon: typeof Users; hint: string }[] = [
  { id: "whitelist", title: "Whitelist", icon: ShieldCheck, hint: "Who may join while the whitelist is on." },
  { id: "ops", title: "Operators", icon: Crown, hint: "Can use every command, like /gamemode and /tp." },
  { id: "banned", title: "Banned", icon: Ban, hint: "Can't join." },
];
const namesOf = (id: EasyListKind) => (id === "banned" ? (lists.value?.banned.map((b) => b.name) ?? []) : (lists.value?.[id] ?? []));
</script>

<template>
  <div v-if="insights && lists" class="stack" style="gap: 16px">
    <div class="stat-grid">
      <div class="card stat">
        <span class="hint">Online now</span><b class="num">{{ server.players.length }} / {{ server.max_players }}</b>
      </div>
      <div class="card stat">
        <span class="hint">Players so far</span><b class="num">{{ insights.players.length }}</b>
      </div>
      <div class="card stat">
        <span class="hint">Most at once</span><b class="num">{{ insights.peak }}</b>
        <span v-if="insights.peak_at" class="hint">{{ relative(insights.peak_at) }}</span>
      </div>
      <div class="card stat">
        <span class="hint">Time played, everyone</span><b class="num">{{ duration(total) }}</b>
      </div>
    </div>

    <div class="card stack">
      <h3>Hours played, last 14 days</h3>
      <div class="chart">
        <div v-for="d in days" :key="d.label" class="bar-col" :title="`${d.label}: ${d.hours.toFixed(1)} h, ${d.players} player${d.players === 1 ? '' : 's'}`">
          <div class="bar" :style="{ height: `${(d.hours / most) * 100}%` }" />
          <span class="hint">{{ d.label }}</span>
        </div>
      </div>
      <p v-if="!insights.sessions.length && !server.players.length" class="hint" style="margin: 0">Nobody has played yet. Times show up here once people join.</p>
    </div>

    <div class="card stack">
      <h3 class="row" style="gap: 8px"><Users :size="18" /> Online <span class="hint num">{{ server.players.length }}</span></h3>
      <p v-if="!server.players.length" class="hint" style="margin: 0">{{ server.state === "running" ? "Nobody's on right now." : "The server isn't running." }}</p>
      <button v-for="p in server.players" :key="p" class="player-row" @click="open = p">
        <img :src="head(p)" width="24" height="24" alt="" /> <b>{{ p }}</b>
        <span class="spacer" />
        <span class="hint">on for {{ duration((now - new Date(insights.players.find((x) => x.name === p)?.online_since ?? now).getTime()) / 1000) }}</span>
      </button>
    </div>

    <div class="list-grid">
      <div v-for="l in LISTS" :key="l.id" class="card stack" style="gap: 10px">
        <h3 class="row" style="gap: 8px">
          <component :is="l.icon" :size="18" /> {{ l.title }} <span class="hint num">{{ namesOf(l.id).length }}</span>
          <span class="spacer" />
          <label v-if="l.id === 'whitelist'" class="row hint" style="gap: 6px; font-weight: 500"><input type="checkbox" :checked="lists.whitelist_on" @change="toggleWhitelist" /> On</label>
        </h3>
        <p class="hint" style="margin: 0">{{ l.hint }}</p>
        <form class="row" @submit.prevent="change(l.id, adding[l.id].trim(), true)">
          <input v-model="adding[l.id]" class="input" placeholder="Player name" maxlength="16" style="flex: 1" />
          <button class="btn btn-icon" :disabled="!adding[l.id].trim() || !!busy" :title="`Add to ${l.title.toLowerCase()}`"><Plus /></button>
        </form>
        <div v-for="n in namesOf(l.id)" :key="n" class="row list-row">
          <img :src="head(n, 20)" width="20" height="20" alt="" />
          <button class="link" @click="open = n">{{ n }}</button>
          <span v-if="l.id === 'banned'" class="hint ellipsis" style="flex: 1">{{ lists.banned.find((b) => b.name === n)?.reason }}</span>
          <span class="spacer" />
          <button class="btn btn-ghost btn-icon btn-sm" :disabled="!!busy" :title="l.id === 'banned' ? 'Unban' : 'Remove'" @click="change(l.id, n, false)">
            <Loader2 v-if="busy === `${l.id}:${n}`" class="spin" /><X v-else />
          </button>
        </div>
      </div>
    </div>

    <div class="card stack">
      <div class="row">
        <h3>Everyone who's played</h3>
        <span class="spacer" />
        <input v-model="search" class="input" placeholder="Search" style="width: 200px" />
      </div>
      <table class="table">
        <thead>
          <tr><th>Player</th><th>Last seen</th><th>Time played</th><th>Joins</th><th>First seen</th></tr>
        </thead>
        <tbody>
          <tr v-for="p in everyone" :key="p.name" class="clickable" @click="open = p.name">
            <td><span class="row" style="gap: 8px"><img :src="head(p.name, 20)" width="20" height="20" alt="" /><b>{{ p.name }}</b><span v-if="p.online_since" class="badge" data-state="running"><span class="dot" />Online</span></span></td>
            <td>{{ p.online_since ? "Now" : known(p.last_seen) ? relative(p.last_seen) : "Before tracking" }}</td>
            <td class="num">{{ played(p) ? duration(played(p)) : "–" }}</td>
            <td class="num">{{ p.joins || "–" }}</td>
            <td>{{ known(p.first_seen) ? new Date(p.first_seen).toLocaleDateString() : "–" }}</td>
          </tr>
          <tr v-if="!everyone.length"><td colspan="5" class="hint">Nobody yet.</td></tr>
        </tbody>
      </table>
    </div>

    <EasyPlayerDialog v-if="open" :server="server" :name="open" @close="open = null" @changed="load" />
  </div>
  <div v-else class="empty"><Loader2 class="spin" /></div>
</template>

<style scoped>
h3 {
  font-size: 15px;
  font-weight: 700;
}
.stat-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
  gap: 12px;
}
.stat {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.stat b {
  font-size: 22px;
}
.chart {
  display: flex;
  align-items: flex-end;
  gap: 6px;
  height: 140px;
}
.bar-col {
  flex: 1;
  height: 100%;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  align-items: center;
  gap: 6px;
  min-width: 0;
}
.bar-col .hint {
  font-size: 10px;
  white-space: nowrap;
}
.bar {
  width: 100%;
  min-height: 2px;
  border-radius: 6px 6px 2px 2px;
  background: var(--tint);
}
.list-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
  gap: 14px;
  align-items: start;
}
.list-row {
  gap: 8px;
}
.player-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border: 0;
  border-radius: 10px;
  background: none;
  color: inherit;
  font: inherit;
  cursor: pointer;
  text-align: left;
}
.player-row:hover,
.clickable:hover {
  background: var(--muted);
}
.link {
  border: 0;
  background: none;
  color: inherit;
  font: inherit;
  font-weight: 600;
  cursor: pointer;
  padding: 0;
}
.table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}
.table th {
  text-align: left;
  font-weight: 600;
  color: var(--muted-foreground);
  padding: 6px 8px;
}
.table td {
  padding: 8px;
  border-top: 1px solid var(--border);
}
.clickable {
  cursor: pointer;
}
</style>
