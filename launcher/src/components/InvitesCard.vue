<script setup lang="ts">
import { onMounted, ref } from "vue";
import { Check, Copy, Loader2, Plus, Ticket, X } from "lucide-vue-next";
import { api, type Invite } from "@/lib/api";
import { errorText, toast } from "@/lib/toast";

/** Invite links you made: each lets one person create an account. Members get three. */
const invites = ref<Invite[] | null>(null);
const remaining = ref<number | null>(null);
const busy = ref(false);

async function load() {
  try {
    const r = await api.invites.list();
    invites.value = r.invites;
    remaining.value = r.remaining;
  } catch (e) {
    toast.error(errorText(e));
  }
}
onMounted(load);

async function create() {
  busy.value = true;
  try {
    const inv = await api.invites.create();
    await navigator.clipboard.writeText(inv.link);
    toast.success("Invite link made and copied");
    await load();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = false;
  }
}

async function revoke(i: Invite) {
  try {
    await api.invites.revoke(i.code);
    await load();
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function copy(i: Invite) {
  await navigator.clipboard.writeText(i.link);
  toast.success("Invite link copied");
}
</script>

<template>
  <div class="card stack">
    <div class="row">
      <span class="tile" style="--type: #8a8aff"><Ticket /></span>
      <div style="flex: 1">
        <div style="font-weight: 700">Invite friends</div>
        <span class="hint">
          Tessera is invite-only. Each link lets one person create an account.
          <template v-if="remaining !== null"> You can make {{ remaining }} more.</template>
          <template v-else> You're an admin: no limit.</template>
        </span>
      </div>
      <button class="btn btn-primary" :disabled="busy || remaining === 0" @click="create"><Loader2 v-if="busy" class="spin" /><Plus v-else /> New invite</button>
    </div>
    <p v-if="invites === null" class="hint row"><Loader2 class="spin" :size="14" /> Loading…</p>
    <div v-for="i in invites ?? []" :key="i.code" class="inset row">
      <span class="mono selectable" style="flex: 1; font-size: 13px; overflow: hidden; text-overflow: ellipsis">{{ i.link }}</span>
      <template v-if="i.used_at">
        <span class="badge" data-state="running"><Check :size="13" /> Used by {{ i.used_by ?? "someone" }}</span>
      </template>
      <template v-else>
        <button class="btn btn-ghost btn-icon btn-sm" title="Copy invite link" aria-label="Copy invite link" @click="copy(i)"><Copy /></button>
        <button class="btn btn-ghost btn-icon btn-sm" title="Revoke (gives the slot back)" aria-label="Revoke invite" @click="revoke(i)"><X /></button>
      </template>
    </div>
  </div>
</template>
