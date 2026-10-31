<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { Ban, Crown, Drumstick, Heart, Loader2, MapPin, Package, ShieldCheck, Skull, Sparkles, Trash2, UserX } from "lucide-vue-next";
import AppDialog from "@/components/AppDialog.vue";
import { api, type EasyAction, type EasyItem, type EasyListKind, type EasyPlayer, type EasyServer } from "@/lib/api";
import { duration, known } from "@/lib/easy";
import { relative } from "@/lib/format";
import { errorText, toast } from "@/lib/toast";

/** One player: their state from the world's files, statistics, and things to do to them. */
const props = defineProps<{ server: EasyServer; name: string }>();
const emit = defineEmits<{ close: []; changed: [] }>();

const p = ref<EasyPlayer | null>(null);
const busy = ref<string | null>(null);
const item = ref("minecraft:diamond");
const count = ref(1);

async function load() {
  try {
    p.value = await api.easy.player(props.server.id, props.name);
  } catch (e) {
    toast.error(errorText(e));
    emit("close");
  }
}
onMounted(load);

const live = computed(() => props.server.state === "running");
const here = computed(() => live.value && !!p.value?.online);

async function act(label: string, action: EasyAction) {
  busy.value = label;
  try {
    const answer = await api.easy.act(props.server.id, props.name, action);
    toast.success(answer.trim() || "Done");
    // the world's files catch up when the game saves
    setTimeout(load, 1500);
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = null;
  }
}

async function list(kind: EasyListKind, add: boolean) {
  busy.value = kind;
  try {
    await api.easy.listChange(props.server.id, kind, props.name, add);
    await load();
    emit("changed");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = null;
  }
}

/** Slots: 0–8 hotbar, 9–35 inventory, 100–103 armor (feet → head), -106 offhand. */
function at(items: EasyItem[], slot: number) {
  return items.find((i) => i.slot === slot) ?? null;
}
const rows = [Array.from({ length: 9 }, (_, i) => 9 + i), Array.from({ length: 9 }, (_, i) => 18 + i), Array.from({ length: 9 }, (_, i) => 27 + i)];
const hotbar = Array.from({ length: 9 }, (_, i) => i);
const armor = [103, 102, 101, 100, -106];
const ender = [0, 1, 2].map((r) => Array.from({ length: 9 }, (_, i) => r * 9 + i));

const words = (id: string) => id.replace(/^minecraft:/, "").split("_").map((w) => w[0]!.toUpperCase() + w.slice(1));
/** Icons from the Minecraft Wiki (Invicon_Diamond_Sword.png). */
const icon = (id: string) => `https://minecraft.wiki/images/Invicon_${words(id).join("_")}.png`;
const label = (id: string) => words(id).join(" ");
const broken = ref(new Set<string>());

const GAMEMODES = ["survival", "creative", "adventure", "spectator"];
</script>

<template>
  <AppDialog :title="name" :width="720" @close="emit('close')">
    <div v-if="p" class="stack" style="gap: 16px">
      <div class="row" style="gap: 16px; align-items: flex-start">
        <img :src="`https://mc-heads.net/body/${encodeURIComponent(p.name)}/90`" width="45" height="90" alt="" style="image-rendering: pixelated" />
        <div class="stack" style="gap: 6px; flex: 1; min-width: 0">
          <div class="row" style="flex-wrap: wrap; gap: 6px">
            <span class="badge" :data-state="p.online ? 'running' : 'idle'"><span class="dot" />{{ p.online ? "Online" : "Offline" }}</span>
            <span v-if="p.op" class="chip"><Crown :size="12" /> Operator</span>
            <span v-if="p.whitelisted" class="chip"><ShieldCheck :size="12" /> Whitelisted</span>
            <span v-if="p.banned" class="chip" style="color: var(--fill-red)"><Ban :size="12" /> Banned</span>
            <span v-if="p.gamemode" class="chip" style="text-transform: capitalize">{{ p.gamemode }}</span>
          </div>
          <div class="row hint" style="flex-wrap: wrap; gap: 14px">
            <span v-if="p.health !== null" class="row" style="gap: 4px"><Heart :size="14" /> {{ (p.health / 2).toFixed(1) }} / 10</span>
            <span v-if="p.food !== null" class="row" style="gap: 4px"><Drumstick :size="14" /> {{ p.food / 2 }} / 10</span>
            <span v-if="p.xp_level !== null" class="row" style="gap: 4px"><Sparkles :size="14" /> Level {{ p.xp_level }}</span>
            <span v-if="p.position" class="row mono" style="gap: 4px"
              ><MapPin :size="14" /> {{ p.position.map((n) => Math.round(n)).join(" ") }}<template v-if="p.dimension"> · {{ p.dimension.replace("minecraft:", "") }}</template></span
            >
          </div>
          <div v-if="p.seen" class="hint">
            {{ p.seen.online_since ? "On now" : known(p.seen.last_seen) ? `Last seen ${relative(p.seen.last_seen)}` : "" }}
            <template v-if="p.seen.joins"> · joined {{ p.seen.joins }}×</template>
            <template v-if="known(p.seen.first_seen)"> · first seen {{ new Date(p.seen.first_seen).toLocaleDateString() }}</template>
          </div>
          <div v-if="p.uuid" class="hint mono selectable" style="font-size: 11px">{{ p.uuid }}</div>
        </div>
      </div>

      <div v-if="p.stats" class="stat-row">
        <div><b class="num">{{ duration(p.stats.play_time) }}</b><span class="hint">played</span></div>
        <div><b class="num">{{ p.stats.deaths }}</b><span class="hint">deaths</span></div>
        <div><b class="num">{{ p.stats.mob_kills }}</b><span class="hint">mobs killed</span></div>
        <div><b class="num">{{ p.stats.player_kills }}</b><span class="hint">players killed</span></div>
        <div><b class="num">{{ (p.stats.distance / 1000).toFixed(1) }} km</b><span class="hint">travelled</span></div>
        <div><b class="num">{{ p.stats.blocks_mined }}</b><span class="hint">blocks mined</span></div>
      </div>

      <div v-if="p.inventory.length || p.health !== null" class="stack" style="gap: 8px">
        <h4>Inventory</h4>
        <div class="row" style="gap: 14px; align-items: flex-start; flex-wrap: wrap">
          <div class="slots">
            <div v-for="(r, i) in rows" :key="i" class="slot-row">
              <div v-for="s in r" :key="s" class="slot" :title="at(p.inventory, s) ? label(at(p.inventory, s)!.id) : ''">
                <template v-if="at(p.inventory, s)">
                  <img v-if="!broken.has(at(p.inventory, s)!.id)" :src="icon(at(p.inventory, s)!.id)" alt="" @error="broken.add(at(p.inventory, s)!.id)" />
                  <span v-else class="slot-name">{{ label(at(p.inventory, s)!.id) }}</span>
                  <span v-if="at(p.inventory, s)!.count > 1" class="count">{{ at(p.inventory, s)!.count }}</span>
                </template>
              </div>
            </div>
            <div class="slot-row" style="margin-top: 6px">
              <div v-for="s in hotbar" :key="s" class="slot" :title="at(p.inventory, s) ? label(at(p.inventory, s)!.id) : ''">
                <template v-if="at(p.inventory, s)">
                  <img v-if="!broken.has(at(p.inventory, s)!.id)" :src="icon(at(p.inventory, s)!.id)" alt="" @error="broken.add(at(p.inventory, s)!.id)" />
                  <span v-else class="slot-name">{{ label(at(p.inventory, s)!.id) }}</span>
                  <span v-if="at(p.inventory, s)!.count > 1" class="count">{{ at(p.inventory, s)!.count }}</span>
                </template>
              </div>
            </div>
          </div>
          <div class="slots">
            <div v-for="s in armor" :key="s" class="slot" :title="at(p.inventory, s) ? label(at(p.inventory, s)!.id) : s === -106 ? 'Offhand' : 'Armor'">
              <template v-if="at(p.inventory, s)">
                <img v-if="!broken.has(at(p.inventory, s)!.id)" :src="icon(at(p.inventory, s)!.id)" alt="" @error="broken.add(at(p.inventory, s)!.id)" />
                <span v-else class="slot-name">{{ label(at(p.inventory, s)!.id) }}</span>
              </template>
            </div>
          </div>
        </div>
        <template v-if="p.ender_chest.length">
          <h4>Ender chest</h4>
          <div class="slots">
            <div v-for="(r, i) in ender" :key="i" class="slot-row">
              <div v-for="s in r" :key="s" class="slot" :title="at(p.ender_chest, s) ? label(at(p.ender_chest, s)!.id) : ''">
                <template v-if="at(p.ender_chest, s)">
                  <img v-if="!broken.has(at(p.ender_chest, s)!.id)" :src="icon(at(p.ender_chest, s)!.id)" alt="" @error="broken.add(at(p.ender_chest, s)!.id)" />
                  <span v-else class="slot-name">{{ label(at(p.ender_chest, s)!.id) }}</span>
                  <span v-if="at(p.ender_chest, s)!.count > 1" class="count">{{ at(p.ender_chest, s)!.count }}</span>
                </template>
              </div>
            </div>
          </div>
        </template>
        <p class="hint" style="margin: 0">As the world last saved it{{ p.online ? "; the game saves every few minutes" : "" }}.</p>
      </div>
      <p v-else class="hint" style="margin: 0">This player hasn't been in this world yet, so there's no inventory to show.</p>

      <div class="stack" style="gap: 8px">
        <h4>Actions</h4>
        <p v-if="!here" class="hint" style="margin: 0">{{ live ? `${p.name} isn't online: game mode, healing, items and kicking work while they are.` : "Start the server for game mode, healing, items and kicking." }}</p>
        <div class="row" style="flex-wrap: wrap; gap: 6px">
          <button v-for="g in GAMEMODES" :key="g" class="btn btn-sm" :class="{ 'btn-primary': p.gamemode === g }" :disabled="!here || !!busy" style="text-transform: capitalize" @click="act(g, { type: 'gamemode', mode: g })">{{ g }}</button>
        </div>
        <div class="row" style="flex-wrap: wrap; gap: 6px">
          <button class="btn btn-sm" :disabled="!here || !!busy" @click="act('heal', { type: 'heal' })"><Heart /> Heal</button>
          <button class="btn btn-sm" :disabled="!here || !!busy" @click="act('feed', { type: 'feed' })"><Drumstick /> Feed</button>
          <button class="btn btn-sm" :disabled="!here || !!busy" @click="act('clear', { type: 'clear_inventory' })"><Trash2 /> Clear inventory</button>
          <button class="btn btn-sm" :disabled="!here || !!busy" @click="act('kill', { type: 'kill' })"><Skull /> Kill</button>
          <button class="btn btn-sm" :disabled="!here || !!busy" @click="act('kick', { type: 'kick', reason: null })"><UserX /> Kick</button>
        </div>
        <form class="row" style="gap: 6px" @submit.prevent="act('give', { type: 'give', item: item, count: count })">
          <input v-model="item" class="input mono" placeholder="minecraft:diamond" style="flex: 1" :disabled="!here" />
          <input v-model.number="count" class="input num" type="number" min="1" max="6400" style="width: 80px" :disabled="!here" />
          <button class="btn btn-sm" :disabled="!here || !!busy || !item.trim()"><Package /> Give</button>
        </form>
        <div class="row" style="flex-wrap: wrap; gap: 6px">
          <button class="btn btn-sm" :disabled="!!busy" @click="list('ops', !p.op)"><Loader2 v-if="busy === 'ops'" class="spin" /><Crown v-else /> {{ p.op ? "Remove operator" : "Make operator" }}</button>
          <button class="btn btn-sm" :disabled="!!busy" @click="list('whitelist', !p.whitelisted)"><Loader2 v-if="busy === 'whitelist'" class="spin" /><ShieldCheck v-else /> {{ p.whitelisted ? "Remove from whitelist" : "Add to whitelist" }}</button>
          <button class="btn btn-sm" :style="p.banned ? '' : 'color: var(--fill-red)'" :disabled="!!busy" @click="list('banned', !p.banned)"><Loader2 v-if="busy === 'banned'" class="spin" /><Ban v-else /> {{ p.banned ? "Unban" : "Ban" }}</button>
        </div>
      </div>
    </div>
    <div v-else class="empty"><Loader2 class="spin" /></div>
  </AppDialog>
</template>

<style scoped>
h4 {
  font-size: 13px;
  font-weight: 700;
}
.stat-row {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(100px, 1fr));
  gap: 10px;
}
.stat-row > div {
  display: flex;
  flex-direction: column;
  padding: 10px;
  border-radius: 12px;
  background: var(--muted);
}
.slots {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.slot-row {
  display: flex;
  gap: 3px;
}
.slot {
  position: relative;
  width: 40px;
  height: 40px;
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.28);
  box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.06);
  display: grid;
  place-items: center;
  overflow: hidden;
}
.slot img {
  width: 32px;
  height: 32px;
  image-rendering: pixelated;
}
.slot-name {
  font-size: 8px;
  line-height: 1.1;
  text-align: center;
  padding: 2px;
}
.count {
  position: absolute;
  right: 3px;
  bottom: 1px;
  font-size: 11px;
  font-weight: 800;
  text-shadow: 1px 1px 0 #000;
}
</style>
