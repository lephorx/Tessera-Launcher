<script setup lang="ts">
import { onMounted, ref } from "vue";
import { Globe, Loader2, LogOut, Monitor } from "lucide-vue-next";
import { api, type SignedInDevice } from "@/lib/api";
import { relative } from "@/lib/format";
import { errorText, toast } from "@/lib/toast";

/** Every browser and launcher signed in to the account, with sign-out for each (or all but this one). */
const devices = ref<SignedInDevice[] | null>(null);
const busy = ref<string | null>(null);

async function load() {
  try {
    devices.value = await api.account.sessions();
  } catch (e) {
    toast.error(errorText(e));
    devices.value = [];
  }
}
onMounted(load);

async function end(d?: SignedInDevice) {
  busy.value = d?.id ?? "others";
  try {
    const n = await api.account.endSession(d?.id);
    toast.success(d ? `Signed out ${d.device}` : n === 1 ? "Signed out 1 other device" : `Signed out ${n} other devices`);
    await load();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = null;
  }
}

const others = () => (devices.value ?? []).filter((d) => !d.current).length;
</script>

<template>
  <div class="stack" style="gap: 8px">
    <span class="label">Signed-in devices</span>
    <span class="hint">Every launcher and browser signed in to your account. Don't recognize one? Sign it out and change your password.</span>
    <p v-if="!devices" class="hint row"><Loader2 class="spin" :size="14" /> Loading…</p>
    <div v-else class="set-table">
      <div v-for="d in devices" :key="d.id" class="set-row">
        <Globe v-if="d.device.includes('tesseralauncher.com')" :size="18" class="muted" />
        <Monitor v-else :size="18" class="muted" />
        <span class="stack" style="gap: 0; min-width: 0">
          <span class="ellipsis" style="font-weight: 600">{{ d.device }}</span>
          <span class="hint">Signed in {{ relative(d.created_at).toLowerCase() }} · active {{ relative(d.last_active_at).toLowerCase() }}</span>
        </span>
        <span class="spacer" />
        <span v-if="d.current" class="set-tag">This computer</span>
        <button v-else class="btn btn-sm" :disabled="busy !== null" @click="end(d)"><Loader2 v-if="busy === d.id" class="spin" /><LogOut v-else /> Sign out</button>
      </div>
    </div>
    <button v-if="devices && others() > 0" class="btn btn-sm" style="align-self: flex-start" :disabled="busy !== null" @click="end()">
      <Loader2 v-if="busy === 'others'" class="spin" /> Sign out everywhere else
    </button>
  </div>
</template>
