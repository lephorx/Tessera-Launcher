<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { AtSign, KeyRound, Link2, Loader2, LogOut, Mail } from "lucide-vue-next";
import { api, type Me } from "@/lib/api";
import { account, loadAccount, setMe, signOut } from "@/lib/account";
import { consentOf, type AgeBand } from "@/lib/legal";
import CodeForm from "./CodeForm.vue";
import ConsentFields from "./ConsentFields.vue";
import { errorText, toast } from "@/lib/toast";

/**
 * Sign in to a Tessera account, or show who is signed in. Accounts are only made from an
 * invite link (it opens sign-up on tesseralauncher.com), so there's no sign-up here.
 * Signing in can take a code from an email: to verify the address first, or for two-step
 * sign-in (`account.pending`, also after this computer's 30 days of trust run out).
 */
defineProps<{ bare?: boolean }>();
const emit = defineEmits<{ signedIn: [] }>();
const email = ref("");
const password = ref("");
const newUsername = ref("");
const invite = ref("");
const busy = ref(false);
const error = ref("");
// the address to verify before signing in (Neon Auth emailed a code to it)
const verifying = ref<string | null>(null);
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
    const r = await api.account.signIn(email.value, password.value);
    if (r.step === "verify_email") {
      verifying.value = r.email;
      return;
    }
    password.value = "";
    signedIn(r.me);
  } catch (e) {
    error.value = errorText(e);
  } finally {
    busy.value = false;
  }
}

/** Signed in, unless a two-step code is still owed: then that step shows. */
function signedIn(me: Me) {
  setMe(me);
  if (me.two_factor_required) return;
  toast.success(`Signed in as ${me.username ?? me.email}`);
  emit("signedIn");
}

const sendVerification = () => api.account.sendVerification(verifying.value ?? "");
async function verifyEmail(code: string) {
  const me = await api.account.verifyEmail(verifying.value ?? "", code);
  verifying.value = null;
  password.value = "";
  signedIn(me);
}

const sendTwoStep = () => api.twoFactor.send("sign_in");
async function verifyTwoStep(code: string) {
  signedIn(await api.twoFactor.verify(code));
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

    <template v-else-if="account.pending">
      <p style="margin: 0">We emailed a code to <strong>{{ account.pending.email }}</strong>. Enter it to finish signing in.</p>
      <CodeForm kind="sign_in" send-on-mount :send="sendTwoStep" :verify="verifyTwoStep" submit-label="Sign in">
        <button type="button" class="btn btn-ghost" @click="signOut"><LogOut /> Sign out</button>
      </CodeForm>
      <p class="hint" style="margin: 0">Two-step sign-in is on for this account. This computer then skips the code for 30 days.</p>
    </template>

    <template v-else-if="verifying">
      <p style="margin: 0">Your email isn't verified yet. We emailed a code to <strong>{{ verifying }}</strong>.</p>
      <CodeForm kind="email-verification" sent :send="sendVerification" :verify="verifyEmail" submit-label="Verify and sign in">
        <button type="button" class="btn btn-ghost" @click="verifying = null">Back</button>
      </CodeForm>
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
