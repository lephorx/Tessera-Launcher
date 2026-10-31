<script lang="ts">
// when each kind of code was last emailed, so opening the step again doesn't send another at once
const sentAt: Record<string, number> = {};
</script>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { Loader2, ShieldCheck } from "lucide-vue-next";
import { errorText, toast } from "@/lib/toast";

/**
 * Enter a 6-digit code from an email, with "Send a new code" (once a minute). `kind` tells
 * codes apart (email verification, two-step sign-in…); `sent` means one is already on its
 * way, `sendOnMount` sends one unless it was sent in the last few minutes. Extra buttons go in
 * the slot.
 */
const props = defineProps<{
  kind: string;
  send: () => Promise<unknown>;
  verify: (code: string) => Promise<unknown>;
  sent?: boolean;
  sendOnMount?: boolean;
  submitLabel?: string;
}>();

const code = ref("");
const busy = ref(false);
const sending = ref(false);
const error = ref("");
const now = ref(Date.now());
const timer = setInterval(() => (now.value = Date.now()), 1000);
onUnmounted(() => clearInterval(timer));
const wait = computed(() => Math.max(0, Math.ceil(((sentAt[props.kind] ?? 0) + 60_000 - now.value) / 1000)));

async function resend(quiet = false) {
  sending.value = true;
  error.value = "";
  try {
    await props.send();
    sentAt[props.kind] = now.value = Date.now();
    if (!quiet) toast.info("We emailed you a new code");
  } catch (e) {
    error.value = errorText(e);
  } finally {
    sending.value = false;
  }
}

onMounted(() => {
  if (props.sent) sentAt[props.kind] = Date.now();
  else if (props.sendOnMount && Date.now() - (sentAt[props.kind] ?? 0) > 5 * 60_000) void resend(true);
  now.value = Date.now();
});

async function submit() {
  busy.value = true;
  error.value = "";
  try {
    await props.verify(code.value);
    delete sentAt[props.kind];
    code.value = "";
  } catch (e) {
    error.value = errorText(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <form class="stack" style="gap: 10px" @submit.prevent="submit">
    <div class="input-icon" style="max-width: 220px">
      <ShieldCheck />
      <input
        v-model="code"
        class="input num"
        inputmode="numeric"
        autocomplete="one-time-code"
        maxlength="6"
        pattern="[0-9]{6}"
        title="The 6 digits from the email"
        placeholder="6-digit code"
        aria-label="6-digit code"
        required
        autofocus
        @input="code = code.replace(/\D/g, '')"
      />
    </div>
    <p v-if="error" class="error-text">{{ error }}</p>
    <div class="row" style="gap: 8px; flex-wrap: wrap">
      <button class="btn btn-primary" :disabled="busy || code.length !== 6"><Loader2 v-if="busy" class="spin" />{{ submitLabel ?? "Continue" }}</button>
      <button type="button" class="btn" :disabled="sending || wait > 0" @click="resend()">
        <Loader2 v-if="sending" class="spin" />{{ wait > 0 ? `Send a new code (${wait} s)` : "Send a new code" }}
      </button>
      <slot />
    </div>
  </form>
</template>
