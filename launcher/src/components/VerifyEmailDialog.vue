<script setup lang="ts">
import { ref } from "vue";
import { LogOut, Pencil } from "lucide-vue-next";
import { api } from "@/lib/api";
import { account, loadAccount, setMe, signOut } from "@/lib/account";
import { errorText, toast } from "@/lib/toast";
import ChangeEmailForm from "./ChangeEmailForm.vue";
import CodeForm from "./CodeForm.vue";

/**
 * Signed in but the email isn't verified: nothing else until it is. It can't be closed;
 * entering the code, fixing the address or signing out are the ways on.
 */
const changing = ref(false);
const email = () => account.me?.email ?? "";

const send = () => api.account.sendVerification(email());
async function verify(code: string) {
  const me = await api.account.verifyEmail(email(), code);
  setMe(me);
  toast.success("Your email is verified");
}

async function changed() {
  changing.value = false;
  await loadAccount();
}

async function leave() {
  try {
    await signOut();
  } catch (e) {
    toast.error(errorText(e));
  }
}
</script>

<template>
  <Teleport to="body">
    <div class="backdrop">
      <div class="dialog" style="max-width: 460px" role="alertdialog" aria-modal="true" aria-labelledby="verify-title">
        <div class="dialog-head">
          <h2 id="verify-title">{{ changing ? "Change your email" : "Verify your email" }}</h2>
        </div>
        <div class="dialog-body stack" style="gap: 14px">
          <template v-if="changing">
            <p class="hint" style="margin: 0; font-size: 13px">
              Typo, or an address you can't open? Enter the right one and your password. We email a code there.
            </p>
            <ChangeEmailForm :email="email()" @changed="changed" @cancel="changing = false" />
          </template>
          <template v-else>
            <p style="margin: 0">
              To keep using your Tessera account, verify your email. We emailed a code to <strong>{{ email() }}</strong>.
            </p>
            <CodeForm :key="email()" kind="email-verification" send-on-mount :send="send" :verify="verify" submit-label="Verify" />
            <div class="dialog-footer" style="margin-top: 0">
              <button type="button" class="btn" style="margin-right: auto" @click="leave"><LogOut /> Sign out</button>
              <button type="button" class="btn btn-ghost" @click="changing = true"><Pencil /> Wrong email? Change it</button>
            </div>
          </template>
        </div>
      </div>
    </div>
  </Teleport>
</template>
