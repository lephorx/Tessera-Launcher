<script setup lang="ts">
import { Check, Loader2, Plus, Trash2 } from "lucide-vue-next";
import AccountCard from "../AccountCard.vue";
import InvitesCard from "../InvitesCard.vue";
import MicrosoftLogo from "../MicrosoftLogo.vue";
import PlayerHead from "../PlayerHead.vue";
import { account } from "@/lib/account";
import { player, removeMicrosoft, signInMicrosoft, useMicrosoft } from "@/lib/player";
import { errorText, toast } from "@/lib/toast";

async function run(f: () => Promise<void>) {
  try {
    await f();
  } catch (e) {
    toast.error(errorText(e));
  }
}
</script>

<template>
  <div class="set-block">
    <h3 class="set-title">Tessera account</h3>
    <p class="set-hint">For friends, sharing packs and your servers.</p>
    <AccountCard />
    <InvitesCard v-if="account.me?.member" style="margin-top: 12px" />
  </div>

  <div class="set-block">
    <h3 class="set-title">Minecraft accounts</h3>
    <p class="set-hint">The accounts you play with. You sign in with the Microsoft account you bought Minecraft with.</p>
    <div class="set-table">
      <div v-for="a in player.accounts" :key="a.id" class="set-row">
        <PlayerHead :name="a.name" :size="28" />
        <span class="ellipsis" style="font-weight: 600">{{ a.name }}</span>
        <span v-if="a.id === player.microsoft" class="set-tag"><Check :size="12" :stroke-width="3" /> Playing</span>
        <span class="spacer" />
        <button v-if="a.id !== player.microsoft" class="btn btn-sm" @click="run(() => useMicrosoft(a))">Play as</button>
        <button class="btn btn-ghost btn-icon btn-sm" :title="`Sign out ${a.name}`" :aria-label="`Sign out ${a.name}`" @click="run(() => removeMicrosoft(a))"><Trash2 /></button>
      </div>
      <p v-if="!player.accounts.length" class="set-row hint">No Minecraft account yet.</p>
    </div>
    <button class="btn" :disabled="player.signingIn" style="align-self: flex-start" @click="signInMicrosoft">
      <Loader2 v-if="player.signingIn" class="spin" /><MicrosoftLogo v-else-if="!player.accounts.length" /><Plus v-else /> Add account
    </button>
  </div>
</template>
