<script setup lang="ts">
import { ref } from "vue";
import { Copy, Crown, Loader2, Plus, Search, X } from "lucide-vue-next";
import { api, type Share, type ShareRole } from "@/lib/api";
import { errorText, toast } from "@/lib/toast";
import AppDialog from "./AppDialog.vue";

/** Invite friends by username, or hand out the invite link. Owners set what each friend may do. */
const props = defineProps<{ share: Share; title: string; instanceId: string }>();
const emit = defineEmits<{ close: []; changed: []; transferred: [] }>();
const transferTo = ref<{ user_id: string; username: string } | null>(null);
const transferring = ref(false);

async function setRole(userId: string, name: string, role: ShareRole) {
  try {
    await api.share.setRole(props.share.code, userId, role);
    toast.success(role === "editor" ? `${name} can add and remove content now` : `${name} can install it but not change it`);
    emit("changed");
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function transfer() {
  if (!transferTo.value) return;
  transferring.value = true;
  try {
    await api.share.transfer(props.instanceId, transferTo.value.user_id);
    toast.success(`${transferTo.value.username} owns ${props.title} now. You stay on it as an editor.`);
    transferTo.value = null;
    emit("transferred");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    transferring.value = false;
  }
}

const username = ref("");
const busy = ref(false);
const editing = ref(false);
const error = ref("");

async function add() {
  busy.value = true;
  error.value = "";
  try {
    await api.share.addFriend(props.share.code, username.value.trim());
    toast.success(`Shared with ${username.value.trim()}`);
    username.value = "";
    emit("changed");
  } catch (e) {
    error.value = errorText(e);
  } finally {
    busy.value = false;
  }
}

async function remove(userId: string, name: string) {
  try {
    await api.share.removeFriend(props.share.code, userId);
    toast.success(`${name} no longer has access`);
    emit("changed");
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function setExpiry(days: number | null) {
  try {
    await api.share.setExpiry(props.share.code, days);
    editing.value = false;
    emit("changed");
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function copy() {
  await navigator.clipboard.writeText(props.share.link ?? "");
  toast.success("Invite link copied");
}

function expiryText(): string {
  if (!props.share.expires_at) return "Your invite link never expires.";
  const days = Math.ceil((new Date(props.share.expires_at).getTime() - Date.now()) / 86_400_000);
  if (days <= 0) return "Your invite link has expired.";
  return `Your invite link expires in ${days} day${days === 1 ? "" : "s"}.`;
}
</script>

<template>
  <AppDialog :title="`Share ${title}`" :width="540" @close="emit('close')">
    <div class="stack" style="gap: 18px">
      <form class="row" @submit.prevent="add">
        <div class="input-icon" style="flex: 1"><Search /><input v-model="username" class="input" placeholder="Enter Tessera username" aria-label="Tessera username" autofocus /></div>
        <button class="btn btn-primary btn-lg" :disabled="busy || !username.trim()"><Loader2 v-if="busy" class="spin" /><Plus v-else /> Add</button>
      </form>
      <p v-if="error" class="error-text" style="margin-top: -10px">{{ error }}</p>

      <div class="stack" style="gap: 8px">
        <span class="label" style="font-weight: 700">Your friends - {{ share.members?.length ?? 0 }}</span>
        <div class="inset stack" style="gap: 4px; min-height: 90px; justify-content: center">
          <p v-if="!share.members?.length" class="hint" style="text-align: center">No friends added yet.</p>
          <div v-for="m in share.members ?? []" :key="m.user_id" class="row" style="padding: 4px 2px">
            <span class="avatar">{{ m.username[0] }}</span>
            <span style="flex: 1; font-weight: 600">{{ m.username }}</span>
            <select
              class="select sd-role"
              :value="m.role ?? 'viewer'"
              :aria-label="`What ${m.username} may do`"
              @change="setRole(m.user_id, m.username, ($event.target as HTMLSelectElement).value as ShareRole)"
            >
              <option value="viewer">Can play</option>
              <option value="editor">Can add content</option>
            </select>
            <button class="btn btn-ghost btn-icon btn-sm" :title="`Make ${m.username} the owner`" :aria-label="`Make ${m.username} the owner`" @click="transferTo = m"><Crown /></button>
            <button class="btn btn-ghost btn-icon btn-sm" :title="`Remove ${m.username}`" :aria-label="`Remove ${m.username}`" @click="remove(m.user_id, m.username)"><X /></button>
          </div>
        </div>
      </div>

      <div class="stack" style="gap: 8px">
        <span class="label" style="font-weight: 700">Or use an invite link</span>
        <div class="row">
          <input class="input mono" :value="share.link" readonly @focus="($event.target as HTMLInputElement).select()" />
          <button class="btn btn-icon" title="Copy link" aria-label="Copy link" @click="copy"><Copy /></button>
        </div>
        <p class="hint">
          {{ expiryText() }}
          <a href="#" @click.prevent="editing = !editing">Edit invite link.</a>
        </p>
        <div v-if="editing" class="choices">
          <button v-for="d in [1, 7, 30]" :key="d" type="button" class="choice" @click="setExpiry(d)">{{ d }} day{{ d === 1 ? "" : "s" }}</button>
          <button type="button" class="choice" @click="setExpiry(null)">Never</button>
        </div>
        <p class="hint">Friends you add by username keep access when the link expires.</p>
      </div>
      <p class="hint">
        <b>Can play</b>: installs the pack and gets your updates. <b>Can add content</b>: may also add and remove mods, resource
        packs, shaders and configs for everyone, but never change the Minecraft or loader version.
      </p>
    </div>
  </AppDialog>

  <AppDialog v-if="transferTo" :title="`Make ${transferTo.username} the owner?`" :width="440" @close="transferTo = null">
    <p class="muted">
      {{ transferTo.username }} gets {{ title }}: they decide who's on it, change versions and can stop sharing it. You stay on it
      as someone who can add content. Only they can give it back.
    </p>
    <div class="dialog-footer">
      <button class="btn" @click="transferTo = null">Cancel</button>
      <button class="btn btn-primary" :disabled="transferring" @click="transfer"><Loader2 v-if="transferring" class="spin" /><Crown v-else /> Make owner</button>
    </div>
  </AppDialog>
</template>

<style scoped>
.sd-role {
  width: auto;
  height: 32px;
  padding: 0 28px 0 10px;
  font-size: 13px;
}
</style>
