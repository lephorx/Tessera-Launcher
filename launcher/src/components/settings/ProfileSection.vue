<script setup lang="ts">
import { computed, ref } from "vue";
import { save } from "@tauri-apps/plugin-dialog";
import { Check, Download, KeyRound, Loader2, Mail, Plus, Trash2 } from "lucide-vue-next";
import AccountCard from "../AccountCard.vue";
import AccountSecurity from "../AccountSecurity.vue";
import SignedInDevices from "../SignedInDevices.vue";
import AppDialog from "../AppDialog.vue";
import InvitesCard from "../InvitesCard.vue";
import MicrosoftLogo from "../MicrosoftLogo.vue";
import PlayerHead from "../PlayerHead.vue";
import { api } from "@/lib/api";
import { account, setMe } from "@/lib/account";
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

// deleting takes two steps: what goes, then the username and password (and with two-step
// sign-in on, a code emailed for it)
const confirmDelete = ref(false);
const deleteStep = ref<1 | 2>(1);
const typed = ref("");
const password = ref("");
const code = ref("");
const codeSent = ref(false);
const sendingCode = ref(false);
const deleting = ref(false);
const needsCode = computed(() => !!account.me?.two_factor);
const deleteReady = computed(() => typed.value.trim() === confirmWord.value && !!password.value && (!needsCode.value || /^\d{6}$/.test(code.value.trim())));
/** What to type to confirm: the username, or the email before one is picked. */
const confirmWord = computed(() => account.me?.username ?? account.me?.email ?? "");
function askDelete() {
  typed.value = "";
  password.value = "";
  code.value = "";
  codeSent.value = false;
  deleteStep.value = 1;
  confirmDelete.value = true;
}
async function sendDeleteCode() {
  sendingCode.value = true;
  try {
    await api.twoFactor.send("delete_account");
    codeSent.value = true;
    toast.success(`We emailed a code to ${account.me?.email}`);
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    sendingCode.value = false;
  }
}
async function deleteAccount() {
  if (!deleteReady.value) return;
  deleting.value = true;
  try {
    await api.account.remove(password.value, typed.value, needsCode.value ? code.value.trim() : undefined);
    setMe(null);
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
    <h3 class="set-title">Sign-in and security</h3>
    <AccountSecurity />
    <SignedInDevices style="margin-top: 14px" />
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
    <form v-if="deleteStep === 1" class="stack" style="gap: 14px" @submit.prevent="deleteStep = 2">
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
      <p class="hint" style="margin: 0; font-size: 13px">Want a copy first? Cancel and choose <b>Download my data</b>.</p>
      <div class="dialog-footer" style="margin-top: 0">
        <button type="button" class="btn btn-lg" @click="confirmDelete = false">Cancel</button>
        <button class="btn btn-danger btn-lg">Continue</button>
      </div>
    </form>
    <form v-else class="stack" style="gap: 14px" @submit.prevent="deleteAccount">
      <p style="margin: 0">
        Last step. To make sure it's you, type your username and enter your password<template v-if="needsCode">, then the code we email you</template>.
      </p>
      <div class="field">
        <label for="del-confirm">Type <strong>{{ confirmWord }}</strong> to confirm</label>
        <input id="del-confirm" v-model="typed" class="input" autocomplete="off" spellcheck="false" autofocus />
      </div>
      <div class="field">
        <label for="del-password">Password</label>
        <div class="input-icon"><KeyRound /><input id="del-password" v-model="password" class="input" type="password" autocomplete="current-password" /></div>
      </div>
      <div v-if="needsCode" class="field">
        <label for="del-code">Code from the email</label>
        <div class="row" style="gap: 8px">
          <input id="del-code" v-model="code" class="input" inputmode="numeric" autocomplete="one-time-code" maxlength="6" placeholder="000000" style="flex: 1" />
          <button type="button" class="btn" :disabled="sendingCode" @click="sendDeleteCode">
            <Loader2 v-if="sendingCode" class="spin" /><Mail v-else /> {{ codeSent ? "Send again" : "Email me the code" }}
          </button>
        </div>
      </div>
      <div class="dialog-footer" style="margin-top: 0">
        <button type="button" class="btn btn-lg" style="margin-right: auto" @click="deleteStep = 1">Back</button>
        <button type="button" class="btn btn-lg" @click="confirmDelete = false">Cancel</button>
        <button class="btn btn-danger btn-lg" :disabled="deleting || !deleteReady">
          <Loader2 v-if="deleting" class="spin" /><Trash2 v-else /> Delete account
        </button>
      </div>
    </form>
  </AppDialog>
</template>
