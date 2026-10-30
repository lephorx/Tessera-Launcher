<script setup lang="ts">
import { onMounted, ref } from "vue";
import { AtSign, KeyRound, Link2, Loader2, LogOut, Mail, User } from "lucide-vue-next";
import { api } from "@/lib/api";
import { account, loadAccount, signOut } from "@/lib/account";
import { errorText, toast } from "@/lib/toast";

/** Sign in to / create a Tessera account, or show who is signed in. */
defineProps<{ bare?: boolean }>();
const emit = defineEmits<{ signedIn: [] }>();
const mode = ref<"in" | "up">("in");
const email = ref("");
const username = ref("");
const password = ref("");
const newUsername = ref("");
const invite = ref("");
const busy = ref(false);
const error = ref("");

onMounted(() => {
  if (!account.loaded) void loadAccount();
});

async function submit() {
  busy.value = true;
  error.value = "";
  try {
    account.me = mode.value === "in" ? await api.account.signIn(email.value, password.value) : await api.account.signUp(email.value, invite.value, username.value, password.value);
    password.value = "";
    toast.success(`Signed in as ${account.me.username ?? account.me.email}`);
    emit("signedIn");
  } catch (e) {
    error.value = errorText(e);
    await loadAccount(); // sign-up may have worked but the username failed
  } finally {
    busy.value = false;
  }
}

async function saveUsername() {
  busy.value = true;
  error.value = "";
  try {
    // signed in but not in yet: joining needs an invite
    if (account.me && !account.me.member) account.me = await api.account.redeem(invite.value, newUsername.value.trim());
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
        <div class="input-icon"><Link2 /><input v-model="invite" class="input" placeholder="Invite link" required /></div>
        <div class="row">
          <div class="input-icon" style="flex: 1"><AtSign /><input v-model="newUsername" class="input" placeholder="username" pattern="[A-Za-z0-9_]{3,24}" required /></div>
          <button class="btn btn-primary" :disabled="busy"><Loader2 v-if="busy" class="spin" />Join</button>
        </div>
      </form>
      <p v-if="error" class="error-text">{{ error }}</p>
    </template>

    <form v-else class="stack" @submit.prevent="submit">
      <div class="choices">
        <button type="button" class="choice" :class="{ on: mode === 'in' }" @click="mode = 'in'">Sign in</button>
        <button type="button" class="choice" :class="{ on: mode === 'up' }" @click="mode = 'up'">Create account</button>
      </div>
      <template v-if="mode === 'up'">
        <p class="hint" style="margin: 0">Tessera is invite-only for now. You need an invite link from a friend; each one works once.</p>
        <div class="input-icon"><Link2 /><input v-model="invite" class="input" placeholder="Invite link (tesseralauncher.com/invite/…)" required /></div>
      </template>
      <div v-if="mode === 'up'" class="input-icon">
        <User /><input v-model="username" class="input" placeholder="Username (friends add you by this)" pattern="[A-Za-z0-9_]{3,24}" title="3–24 letters, digits or underscores" required />
      </div>
      <div class="input-icon"><Mail /><input v-model="email" class="input" type="email" placeholder="Email" autocomplete="email" required /></div>
      <div class="input-icon">
        <KeyRound /><input v-model="password" class="input" type="password" :placeholder="mode === 'up' ? 'Password (at least 8 characters)' : 'Password'" :minlength="mode === 'up' ? 8 : undefined" required />
      </div>
      <p v-if="error" class="error-text">{{ error }}</p>
      <div class="row">
        <button class="btn btn-primary btn-lg" :disabled="busy"><Loader2 v-if="busy" class="spin" />{{ mode === "in" ? "Sign in" : "Create account" }}</button>
        <span class="hint">You stay signed in on this computer.</span>
      </div>
    </form>
  </div>
</template>
