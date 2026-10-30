<script setup lang="ts">
import { computed, ref } from "vue";
import { save } from "@tauri-apps/plugin-dialog";
import { Check, Download, Loader2, Plus, Trash2 } from "lucide-vue-next";
import AccountCard from "../AccountCard.vue";
import AppDialog from "../AppDialog.vue";
import InvitesCard from "../InvitesCard.vue";
import MicrosoftLogo from "../MicrosoftLogo.vue";
import PlayerHead from "../PlayerHead.vue";
import { api } from "@/lib/api";
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

// ── your data: download it, or delete the account ──

const exporting = ref(false);
async function exportData() {
  const path = await save({ defaultPath: "tessera-data.json", filters: [{ name: "JSON", extensions: ["json"] }] });
  if (!path) return;
  exporting.value = true;
  try {
    await api.account.exportData(path);
    toast.success("Your data is saved");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    exporting.value = false;
  }
}

const confirmDelete = ref(false);
const typed = ref("");
const deleting = ref(false);
/** What to type to confirm: the username, or the email before one is picked. */
const confirmWord = computed(() => account.me?.username ?? account.me?.email ?? "");
function askDelete() {
  typed.value = "";
  confirmDelete.value = true;
}
async function deleteAccount() {
  if (typed.value.trim() !== confirmWord.value) return;
  deleting.value = true;
  try {
    await api.account.remove();
    account.me = null;
    confirmDelete.value = false;
    toast.success("Your Tessera account is deleted");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    deleting.value = false;
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

  <div v-if="account.me" class="set-block">
    <h3 class="set-title">Your data</h3>
    <p class="set-hint">Download everything Tessera stores about your account as a JSON file, or delete the account.</p>
    <div class="row" style="gap: 8px">
      <button class="btn" :disabled="exporting" @click="exportData"><Loader2 v-if="exporting" class="spin" /><Download v-else /> Download my data</button>
      <button class="btn btn-danger" @click="askDelete"><Trash2 /> Delete account</button>
    </div>
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

  <AppDialog v-if="confirmDelete" title="Delete your Tessera account?" :width="460" @close="confirmDelete = false">
    <form class="stack" style="gap: 14px" @submit.prevent="deleteAccount">
      <p style="margin: 0">This deletes, for good:</p>
      <ul class="hint" style="margin: 0; padding-left: 18px; font-size: 13px; line-height: 1.6">
        <li>your Tessera account and username</li>
        <li>the packs you share, with their uploaded files (friends lose access to them)</li>
        <li>your friends, friend requests and blocks</li>
        <li>your invites</li>
      </ul>
      <p class="hint" style="margin: 0; font-size: 13px">
        Your instances, worlds and Minecraft accounts stay on this computer. It can't be undone.
      </p>
      <div class="field">
        <label for="del-confirm">Type <strong>{{ confirmWord }}</strong> to confirm</label>
        <input id="del-confirm" v-model="typed" class="input" autocomplete="off" spellcheck="false" />
      </div>
      <div class="dialog-footer" style="margin-top: 0">
        <button type="button" class="btn btn-lg" @click="confirmDelete = false">Cancel</button>
        <button class="btn btn-danger btn-lg" :disabled="deleting || typed.trim() !== confirmWord">
          <Loader2 v-if="deleting" class="spin" /><Trash2 v-else /> Delete account
        </button>
      </div>
    </form>
  </AppDialog>
</template>
