<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { Check, Loader2 } from "lucide-vue-next";
import { onBeforeRouteLeave, RouterLink, useRouter, type RouteLocationRaw } from "vue-router";
import AppDialog from "@/components/AppDialog.vue";
import MicrosoftLogo from "@/components/MicrosoftLogo.vue";
import PlayerHead from "@/components/PlayerHead.vue";
import AccountCard from "@/components/AccountCard.vue";
import InvitesCard from "@/components/InvitesCard.vue";
import { account } from "@/lib/account";
import { api, type Settings } from "@/lib/api";
import { currentDesign, setDesign, type Design } from "@/lib/design";
import { player, signInMicrosoft } from "@/lib/player";
import { errorText, toast } from "@/lib/toast";

const settings = ref<Settings | null>(null);
/** What's on disk, to tell whether the form has unsaved changes. */
const saved = ref<Settings | null>(null);
const design = ref<Design>(currentDesign());
const totalMb = ref(16384);
const router = useRouter();

onMounted(async () => {
  settings.value = await api.settings.get();
  saved.value = { ...settings.value };
  totalMb.value = await api.system.memoryMb().catch(() => 16384);
});

const maxMb = computed(() => Math.max(2048, Math.floor((totalMb.value * 0.75) / 512) * 512));
const gb = (mb: number) => (mb / 1024).toFixed(mb % 1024 ? 1 : 0);

// Design and accounts save on their own; the form only holds the game settings.
const dirty = computed(() => !!settings.value && !!saved.value && settings.value.memory_mb !== saved.value.memory_mb);

// Leaving with unsaved changes asks first.
const leaving = ref<RouteLocationRaw | null>(null);
let discard = false;
onBeforeRouteLeave((to) => {
  if (!dirty.value || discard) return true;
  leaving.value = to.fullPath;
  return false;
});
function leave() {
  const to = leaving.value;
  leaving.value = null;
  if (!to) return;
  discard = true;
  void router.push(to);
}

async function save() {
  if (!settings.value) return;
  try {
    // accounts are managed on Home; keep the current one
    settings.value = await api.settings.save({ ...settings.value, design: design.value, microsoft: player.microsoft });
    saved.value = { ...settings.value };
    toast.success("Saved");
  } catch (e) {
    toast.error(errorText(e));
  }
}

function pick(d: Design) {
  design.value = d;
  setDesign(d);
  if (settings.value) void api.settings.save({ ...settings.value, design: d, microsoft: player.microsoft });
}
</script>

<template>
  <div class="page page-narrow enter">
    <div class="page-header"><h1 class="page-title">Settings</h1></div>

    <h2 class="section-title" style="margin-top: 0">Tessera account</h2>
    <AccountCard />
    <InvitesCard v-if="account.me?.member" style="margin-top: 12px" />

    <form v-if="settings" class="stack" @submit.prevent="save">
      <h2 class="section-title">Minecraft</h2>
      <div class="card stack">
        <div v-if="player.microsoft" class="row" style="gap: 12px">
          <PlayerHead :name="player.name" :size="40" />
          <div style="flex: 1">
            <div style="font-weight: 700">Playing as {{ player.name }}</div>
            <span class="hint">
              Switch accounts under <RouterLink to="/" class="link">Playing as</RouterLink> on Home.
            </span>
          </div>
        </div>
        <span v-else class="hint">Add your Minecraft account to play. You sign in with the Microsoft account you bought Minecraft with.</span>
        <button type="button" class="btn" :disabled="player.signingIn" style="align-self: flex-start" @click="signInMicrosoft">
          <Loader2 v-if="player.signingIn" class="spin" /><MicrosoftLogo v-else /> {{ player.accounts.length ? "Add another account" : "Add account" }}
        </button>
      </div>

      <h2 class="section-title">Game</h2>
      <div class="card stack">
        <div class="field">
          <label for="s-mem">Memory per instance</label>
          <div class="row">
            <input
              id="s-mem"
              v-model.number="settings.memory_mb"
              class="range"
              type="range"
              min="1024"
              :max="maxMb"
              step="512"
              :style="{ '--range-fill': `${((Math.min(settings.memory_mb, maxMb) - 1024) / (maxMb - 1024)) * 100}%` }"
            />
            <span class="mono num" style="width: 72px; text-align: right; font-weight: 600">{{ gb(settings.memory_mb) }} GB</span>
          </div>
          <span class="hint num">4 GB is enough for most modpacks. Each instance can override it. This computer has {{ gb(totalMb) }} GB.</span>
        </div>
      </div>

      <h2 class="section-title">Appearance</h2>
      <div class="row" style="gap: 12px">
        <button
          v-for="d in (['modern', 'classic'] as Design[])"
          :key="d"
          type="button"
          class="card card-hover option"
          :class="{ selected: design === d }"
          style="flex-direction: column; align-items: stretch; gap: 10px"
          @click="pick(d)"
        >
          <span
            :style="{
              height: '90px',
              background: d === 'modern' ? 'linear-gradient(160deg,#2b2e35,#16181d)' : '#09090b',
              borderRadius: d === 'modern' ? '12px' : '0',
              border: d === 'classic' ? '1px solid rgb(255 255 255 / .1)' : 'none',
              display: 'flex',
              gap: '8px',
              padding: '14px',
            }"
          >
            <span :style="{ width: '34%', background: d === 'modern' ? '#212329' : '#18181b', borderRadius: d === 'modern' ? '8px' : '0' }" />
            <span :style="{ flex: 1, background: d === 'modern' ? '#212329' : '#18181b', borderRadius: d === 'modern' ? '8px' : '0' }" />
          </span>
          <span class="row">
            <span style="font-weight: 600">{{ d === "modern" ? "Modern" : "Classic" }}</span>
            <span class="spacer" />
            <Check v-if="design === d" :size="16" style="color: var(--tint)" />
          </span>
        </button>
      </div>

      <div class="row" style="margin-top: 20px">
        <button class="btn btn-primary btn-lg">Save</button>
        <span v-if="dirty" class="hint">You have unsaved changes.</span>
      </div>
    </form>

    <AppDialog v-if="leaving" title="Leave without saving?" :width="420" @close="leaving = null">
      <p class="muted">Your changes to Settings haven't been saved. If you continue, they're lost.</p>
      <div class="dialog-footer">
        <button class="btn" @click="leaving = null">Cancel</button>
        <button class="btn btn-danger" @click="leave">Continue</button>
      </div>
    </AppDialog>
  </div>
</template>
