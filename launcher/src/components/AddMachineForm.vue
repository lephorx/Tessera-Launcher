<script setup lang="ts">
import { ref } from "vue";
import { useRouter } from "vue-router";
import { Globe, KeyRound, Loader2, ShieldCheck, User } from "lucide-vue-next";
import { api } from "@/lib/api";
import { errorText, toast } from "@/lib/toast";

const props = defineProps<{ placeholder?: string }>();
const router = useRouter();
const name = ref("");
const url = ref("");
const username = ref("");
const password = ref("");
const otp = ref("");
const busy = ref(false);
const error = ref("");

async function add() {
  busy.value = true;
  error.value = "";
  try {
    const m = await api.machines.add(name.value, url.value, username.value.trim(), password.value, otp.value.trim());
    toast.success(`${m.name} added`);
    router.push(`/servers/${m.id}`);
  } catch (e) {
    error.value = errorText(e);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <form class="stack" @submit.prevent="add">
    <div class="field">
      <label for="m-url">Dashboard address</label>
      <div class="input-icon"><Globe /><input id="m-url" v-model="url" class="input mono" :placeholder="props.placeholder ?? 'http://100.101.102.103:3000'" required /></div>
      <span class="hint">The address you open CSCM's dashboard with: localhost, a LAN IP, or a Tailscale name or IP.</span>
    </div>
    <div class="field">
      <label for="m-name">Name <span class="hint">(optional)</span></label>
      <input id="m-name" v-model="name" class="input" placeholder="Home server" />
    </div>
    <div class="field">
      <span class="label">CSCM login</span>
      <div class="input-icon"><User /><input v-model="username" class="input" placeholder="Username" autocomplete="username" required /></div>
      <div class="input-icon"><KeyRound /><input v-model="password" class="input" type="password" placeholder="Password" autocomplete="current-password" required /></div>
      <div class="input-icon"><ShieldCheck /><input v-model="otp" class="input num" inputmode="numeric" maxlength="6" placeholder="One-time code" required /></div>
    </div>
    <p v-if="error" class="error-text">{{ error }}</p>
    <div class="row">
      <button type="submit" class="btn btn-primary btn-lg" :disabled="busy">
        <Loader2 v-if="busy" class="spin" /> Connect
      </button>
    </div>
  </form>
</template>
