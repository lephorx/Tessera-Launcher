<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { Check, Loader2 } from "lucide-vue-next";
import CodeForm from "./CodeForm.vue";
import { api, type TrustedDevice } from "@/lib/api";
import { account, setMe } from "@/lib/account";
import { errorText, toast } from "@/lib/toast";

/**
 * The email's verified status (with a code to verify it), two-step sign-in by email (a code
 * turns it on or off), and forgetting the other computers that skip the code.
 */
const verifying = ref(false);
// turning two-step sign-in on or off waits for a code
const changing = ref<"enable" | "disable" | null>(null);
const devices = ref<TrustedDevice[] | null>(null);
const others = computed(() => (devices.value ?? []).filter((d) => !d.current).length);
const forgetting = ref(false);

async function loadDevices() {
  devices.value = null;
  if (!account.me?.two_factor) return;
  try {
    devices.value = await api.twoFactor.devices();
  } catch (e) {
    toast.error(errorText(e));
  }
}
onMounted(loadDevices);
watch(() => account.me?.two_factor, loadDevices);

const sendVerification = () => api.account.sendVerification(account.me?.email ?? "");
async function verifyEmail(code: string) {
  setMe(await api.account.verifyEmail(account.me?.email ?? "", code));
  verifying.value = false;
  toast.success("Your email is verified");
}

/** The switch only asks for a code; it flips once the code is right. */
function flip(e: Event) {
  (e.target as HTMLInputElement).checked = !!account.me?.two_factor;
  changing.value = account.me?.two_factor ? "disable" : "enable";
}
const sendChange = () => api.twoFactor.send(changing.value ?? "enable");
async function confirmChange(code: string) {
  const on = changing.value === "enable";
  setMe(await (on ? api.twoFactor.enable(code) : api.twoFactor.disable(code)));
  changing.value = null;
  toast.success(on ? "Two-step sign-in is on" : "Two-step sign-in is off");
}

async function forgetOthers() {
  forgetting.value = true;
  try {
    const n = await api.twoFactor.forgetOthers();
    toast.success(n === 1 ? "Forgot 1 other computer" : `Forgot ${n} other computers`);
    await loadDevices();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    forgetting.value = false;
  }
}
</script>

<template>
  <div v-if="account.me" class="stack" style="gap: 14px">
    <div class="stack" style="gap: 8px">
      <div class="row" style="gap: 8px">
        <span class="label">Email</span>
        <span class="hint ellipsis">{{ account.me.email }}</span>
        <span v-if="account.me.email_verified" class="set-tag"><Check :size="12" :stroke-width="3" /> Verified</span>
        <template v-else>
          <span class="set-tag">Not verified</span>
          <span class="spacer" />
          <button v-if="!verifying" class="btn btn-sm" @click="verifying = true">Verify</button>
        </template>
      </div>
      <template v-if="verifying && !account.me.email_verified">
        <p class="hint" style="margin: 0">We emailed a code to {{ account.me.email }}.</p>
        <CodeForm kind="email-verification" send-on-mount :send="sendVerification" :verify="verifyEmail" submit-label="Verify">
          <button type="button" class="btn btn-ghost" @click="verifying = false">Cancel</button>
        </CodeForm>
      </template>
    </div>

    <div class="stack" style="gap: 8px">
      <label class="row" style="justify-content: space-between">
        <span>
          <span class="label">Two-step sign-in by email</span>
          <span class="hint" style="display: block">Signing in on a new computer also needs a code from your email. A computer that entered one skips it for 30 days.</span>
        </span>
        <span class="switch">
          <input type="checkbox" :checked="account.me.two_factor" :disabled="!!changing || (!account.me.two_factor && !account.me.email_verified)" @change="flip" /><span />
        </span>
      </label>
      <p v-if="!account.me.two_factor && !account.me.email_verified" class="hint" style="margin: 0">Verify your email first.</p>
      <template v-if="changing">
        <p class="hint" style="margin: 0">We emailed a code to {{ account.me.email }}. Enter it to turn two-step sign-in {{ changing === "enable" ? "on" : "off" }}.</p>
        <CodeForm :key="changing" :kind="`2fa-${changing}`" send-on-mount :send="sendChange" :verify="confirmChange" :submit-label="changing === 'enable' ? 'Turn on' : 'Turn off'">
          <button type="button" class="btn btn-ghost" @click="changing = null">Cancel</button>
        </CodeForm>
      </template>
      <div v-if="account.me.two_factor && !changing" class="row" style="gap: 8px">
        <span class="hint">
          <Loader2 v-if="!devices" class="spin" :size="14" />
          <template v-else>{{ others === 0 ? "No other computer skips the code." : others === 1 ? "1 other computer skips the code." : `${others} other computers skip the code.` }}</template>
        </span>
        <span class="spacer" />
        <button class="btn btn-sm" :disabled="forgetting || !others" @click="forgetOthers"><Loader2 v-if="forgetting" class="spin" />Forget other devices</button>
      </div>
    </div>
  </div>
</template>
