<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { Check, Loader2 } from "lucide-vue-next";
import PlayerHead from "../PlayerHead.vue";
import { account } from "@/lib/account";
import { api, type Blocked, type Privacy } from "@/lib/api";
import { errorText, toast } from "@/lib/toast";

/** Who may send you friend requests or add you to shared packs, and who you blocked. */
const privacy = ref<Privacy | null>(null);
const blocks = ref<Blocked[] | null>(null);
const error = ref("");
const blockName = ref("");
const busy = ref(false);

const friendOptions: { value: Privacy["friend_requests"]; label: string }[] = [
  { value: "everyone", label: "Everyone" },
  { value: "friends_of_friends", label: "Friends of friends" },
  { value: "no_one", label: "No one" },
];
const shareOptions: { value: Privacy["share_invites"]; label: string }[] = [
  { value: "everyone", label: "Everyone" },
  { value: "friends", label: "Friends" },
  { value: "no_one", label: "No one" },
];

async function load() {
  if (!account.me?.member) return;
  try {
    [privacy.value, blocks.value] = await Promise.all([api.privacy.get(), api.blocks.list()]);
    error.value = "";
  } catch (e) {
    error.value = errorText(e);
  }
}
onMounted(load);
watch(() => account.me?.id, load);

async function set(p: Partial<Privacy>) {
  try {
    privacy.value = await api.privacy.set(p);
  } catch (e) {
    toast.error(errorText(e));
  }
}

async function block() {
  busy.value = true;
  try {
    await api.blocks.add(blockName.value.trim());
    toast.success(`${blockName.value.trim()} is blocked`);
    blockName.value = "";
    blocks.value = await api.blocks.list();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = false;
  }
}

async function unblock(b: Blocked) {
  try {
    await api.blocks.remove(b.user_id);
    blocks.value = (blocks.value ?? []).filter((x) => x.user_id !== b.user_id);
  } catch (e) {
    toast.error(errorText(e));
  }
}
</script>

<template>
  <div v-if="!account.me" class="set-block">
    <h3 class="set-title">Social</h3>
    <p class="set-hint">Sign in to your Tessera account to choose who can add you as a friend or share packs with you.</p>
    <button class="btn btn-primary" style="align-self: flex-start" @click="account.dialog = true">Sign in</button>
  </div>
  <p v-else-if="!account.me.member" class="set-hint">Join Tessera with an invite first (Profile).</p>
  <p v-else-if="error" class="error-text">{{ error }}</p>
  <div v-else-if="!privacy" class="hint row"><Loader2 class="spin" :size="14" /> Loading…</div>
  <template v-else>
    <div class="set-block">
      <h3 class="set-title">Friend requests</h3>
      <div class="choices">
        <button v-for="o in friendOptions" :key="o.value" type="button" class="choice" :class="{ on: privacy.friend_requests === o.value }" @click="set({ friend_requests: o.value })">
          <Check v-if="privacy.friend_requests === o.value" />{{ o.label }}
        </button>
      </div>
      <p class="set-hint">Who can send you friend requests on Tessera.</p>
    </div>

    <div class="set-block">
      <h3 class="set-title">Shared pack invites</h3>
      <div class="choices">
        <button v-for="o in shareOptions" :key="o.value" type="button" class="choice" :class="{ on: privacy.share_invites === o.value }" @click="set({ share_invites: o.value })">
          <Check v-if="privacy.share_invites === o.value" />{{ o.label }}
        </button>
      </div>
      <p class="set-hint">Who can add you to the packs they share. Invite links still work for anyone you give them to.</p>
    </div>

    <div class="set-block">
      <h3 class="set-title">Blocked users</h3>
      <p class="set-hint">People you block can't send you friend requests or add you to shared packs. Blocking also unfriends them and takes them off each other's shared packs.</p>
      <form class="row" style="gap: 8px" @submit.prevent="block">
        <input v-model="blockName" class="input" placeholder="Tessera username" aria-label="Tessera username to block" maxlength="24" required style="max-width: 260px" />
        <button class="btn" :disabled="busy"><Loader2 v-if="busy" class="spin" />Block</button>
      </form>
      <div class="set-table">
        <div class="set-row set-row-head"><span>User</span><span class="spacer" /><span>Actions</span></div>
        <div v-for="b in blocks ?? []" :key="b.user_id" class="set-row">
          <PlayerHead :name="b.username" :size="24" />
          <span class="ellipsis" style="font-weight: 600">{{ b.username }}</span>
          <span class="spacer" />
          <button class="btn btn-sm" @click="unblock(b)">Unblock</button>
        </div>
        <p v-if="blocks && !blocks.length" class="set-row hint">You haven't blocked anyone.</p>
      </div>
    </div>
  </template>
</template>
