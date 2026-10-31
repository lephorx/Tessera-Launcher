<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { AtSign, KeyRound, Link2, Loader2, LogOut, Mail } from "lucide-vue-next";
import { api } from "@/lib/api";
import { account, loadAccount, signOut } from "@/lib/account";
import { consentOf, type AgeBand } from "@/lib/legal";
import ConsentFields from "./ConsentFields.vue";
import { errorText, toast } from "@/lib/toast";

/**
 * Sign in to a Tessera account, or show who is signed in. Accounts are only made from an
 * invite link (it opens sign-up on tesseralauncher.com), so there's no sign-up here.
 */
defineProps<{ bare?: boolean }>();
const emit = defineEmits<{ signedIn: [] }>();
const email = ref("");
const password = ref("");
const newUsername = ref("");
const invite = ref("");
const busy = ref(false);
const error = ref("");
// age and Terms, for joining with an invite
const age = ref<AgeBand | null>(null);
const parental = ref(false);
const agreed = ref(false);
const consent = computed(() => consentOf(age.value, parental.value, agreed.value));

onMounted(() => {
  if (!account.loaded) void loadAccount();
});

async function submit() {
  busy.value = true;
  error.value = "";
  try {
    account.me = await api.account.signIn(email.value, password.value);
    password.value = "";
    toast.success(`Signed in as ${account.me.username ?? account.me.email}`);
    emit("signedIn");
  } catch (e) {
    error.value = errorText(e);
  } finally {
    busy.value = false;
  }
}

async function saveUsername() {
  busy.value = true;
  error.value = "";
  try {
    // signed in but not in yet: joining needs an invite
    if (account.me && !account.me.member) {
      if (!consent.value) return;
      account.me = await api.account.redeem(invite.value, newUsername.value.trim(), consent.value);
    }
    else await api.account.setUsername(newUsername.value.trim());
    await loadAccount();
    newUsername.value = "";
    invite.value = "";
  } catch (e) {
    error.value = errorText(e);
  } finally {
    busy.value = false;
  }
}

</script>

<template>
  <div class="stack" :class="{ card: !bare }">
    <template v-if="!account.loaded">
      <p class="hint row"><Loader2 class="spin" :size="14" /> Checking your account…</p>
    </template>

    <template v-else-if="account.me">
      <div class="row" style="gap: 12px">
        <span class="avatar" style="width: 40px; height: 40px; font-size: 16px">{{ (account.me.username ?? account.me.email)[0] }}</span>
        <div style="flex: 1; min-width: 0">
          <div style="font-weight: 700">{{ account.me.username ?? "No username yet" }}</div>
          <div class="hint">{{ account.me.email }}</div>
        </div>
        <button class="btn" @click="signOut"><LogOut /> Sign out</button>
      </div>
      <form v-if="!account.me.member" class="stack" style="gap: 8px" @submit.prevent="saveUsername">
        <span class="hint">Tessera is invite-only for now. Paste the invite link a friend sent you and pick a username.</span>
        <div class="field">
          <label for="acc-join-invite">Invite link</label>
          <div class="input-icon"><Link2 /><input id="acc-join-invite" v-model="invite" class="input" placeholder="tesseralauncher.com/invite/…" required /></div>
        </div>
        <div class="field">
          <label for="acc-join-username">Username</label>
          <div class="input-icon"><AtSign /><input id="acc-join-username" v-model="newUsername" class="input" placeholder="Friends add you by this" pattern="[A-Za-z0-9_]{3,24}" title="3–24 letters, digits or underscores" required /></div>
        </div>
        <ConsentFields v-model:age="age" v-model:parental="parental" v-model:agreed="agreed" />
        <button class="btn btn-primary" style="align-self: flex-start" :disabled="busy || !consent"><Loader2 v-if="busy" class="spin" />Join</button>
      </form>
      <p v-if="error" class="error-text">{{ error }}</p>
    </template>

    <form v-else class="stack" @submit.prevent="submit">
      <div class="field">
        <label for="acc-email">Email</label>
        <div class="input-icon"><Mail /><input id="acc-email" v-model="email" class="input" type="email" placeholder="you@example.com" autocomplete="email" required /></div>
      </div>
      <div class="field">
        <label for="acc-password">Password</label>
        <div class="input-icon">
          <KeyRound /><input id="acc-password" v-model="password" class="input" type="password" autocomplete="current-password" required />
        </div>
      </div>
      <p v-if="error" class="error-text">{{ error }}</p>
      <div class="row">
        <button class="btn btn-primary btn-lg" :disabled="busy"><Loader2 v-if="busy" class="spin" />Sign in</button>
        <span class="hint">You stay signed in on this computer.</span>
      </div>
      <p class="hint" style="margin: 0">New to Tessera? It's invite-only: open the invite link a friend sent you to make your account.</p>
    </form>
  </div>
</template>
