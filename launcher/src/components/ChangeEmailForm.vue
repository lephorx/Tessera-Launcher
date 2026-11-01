<script setup lang="ts">
import { ref } from "vue";
import { KeyRound, Loader2, Mail } from "lucide-vue-next";
import { api } from "@/lib/api";
import { errorText, toast } from "@/lib/toast";

/**
 * Fixes the address of an account that isn't verified yet (a typo, a throwaway address).
 * Needs the password, unless the sign-in form already has it; a code then goes to the new
 * address and `changed` says which one.
 */
const props = defineProps<{ email: string; password?: string }>();
const emit = defineEmits<{ changed: [email: string]; cancel: [] }>();
const newEmail = ref("");
const password = ref("");
const busy = ref(false);
const error = ref("");

async function submit() {
  busy.value = true;
  error.value = "";
  try {
    const next = await api.account.changeEmail(props.email, props.password ?? password.value, newEmail.value);
    toast.success(`We emailed a code to ${next}`);
    emit("changed", next);
  } catch (e) {
    error.value = errorText(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <form class="stack" style="gap: 10px" @submit.prevent="submit">
    <div class="field">
      <label for="change-email">New email</label>
      <div class="input-icon"><Mail /><input id="change-email" v-model="newEmail" class="input" type="email" placeholder="you@example.com" autocomplete="email" required autofocus /></div>
    </div>
    <div v-if="props.password === undefined" class="field">
      <label for="change-email-password">Your password</label>
      <div class="input-icon"><KeyRound /><input id="change-email-password" v-model="password" class="input" type="password" autocomplete="current-password" required /></div>
    </div>
    <p v-if="error" class="error-text">{{ error }}</p>
    <div class="row" style="gap: 8px">
      <button class="btn btn-primary" :disabled="busy"><Loader2 v-if="busy" class="spin" />Change email</button>
      <button type="button" class="btn btn-ghost" @click="emit('cancel')">Back</button>
    </div>
  </form>
</template>
