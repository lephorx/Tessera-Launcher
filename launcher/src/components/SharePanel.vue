<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { Copy, Loader2, LogIn, LogOut, RefreshCw, Trash2, UserPlus, Users } from "lucide-vue-next";
import { api, type Instance, type MemberStatus, type Share } from "@/lib/api";
import { relative } from "@/lib/format";
import { shareSync } from "@/lib/store";
import { account, loadAccount } from "@/lib/account";
import { errorText, toast } from "@/lib/toast";
import AppDialog from "./AppDialog.vue";
import ShareDialog from "./ShareDialog.vue";

const props = defineProps<{ instance: Instance }>();
/** The instance changed (pulled an update, left, ownership moved): the page reloads it. */
const emit = defineEmits<{ changed: [] }>();
const member = ref<MemberStatus | null>(null);
const confirmLeave = ref(false);
const share = ref<Share | null>(null);
const loading = ref(true);
const busy = ref<"" | "create" | "update" | "stop" | "pull" | "publish" | "leave">("");
const inviting = ref(false);
const confirmStop = ref(false);

async function load() {
  loading.value = true;
  try {
    if (!account.loaded) await loadAccount();
    member.value = null;
    if (account.me && props.instance.joined) {
      const st = await api.share.memberStatus(props.instance.id);
      if (st && "became_owner" in st && st.became_owner) {
        toast.success(`${props.instance.name} is yours now: you were made the owner`);
        emit("changed");
      } else {
        member.value = st as MemberStatus | null;
      }
    }
    share.value = account.me && !props.instance.joined ? await api.share.status(props.instance.id) : null;
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    loading.value = false;
  }
}
onMounted(load);
// signed in from the dialog (or out from the top bar)
watch(() => account.me?.id, (id, before) => id !== before && load());

async function invite() {
  if (share.value) {
    inviting.value = true;
    return;
  }
  busy.value = "create";
  try {
    share.value = await api.share.create(props.instance.id);
    inviting.value = true;
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = "";
  }
}


async function stop() {
  busy.value = "stop";
  try {
    await api.share.stop(props.instance.id);
    share.value = null;
    confirmStop.value = false;
    toast.success("Stopped sharing");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = "";
  }
}

const live = () => shareSync[props.instance.id];

async function syncNow() {
  busy.value = "pull";
  try {
    await api.share.syncNow(props.instance.id);
    if (live()?.state !== "error") toast.success("In sync");
    await load();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = "";
  }
}



async function leave() {
  busy.value = "leave";
  try {
    await api.share.leave(props.instance.id);
    confirmLeave.value = false;
    toast.success("Left the shared pack. Your copy stays.");
    emit("changed");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = "";
  }
}

async function copy() {
  await navigator.clipboard.writeText(share.value?.link ?? "");
  toast.success("Invite link copied");
}
</script>

<template>
  <div class="stack">
    <div v-if="loading" class="empty"><Loader2 class="spin" /></div>

    <div v-else-if="!account.me" class="card empty" style="padding: 64px 24px">
      <UserPlus />
      <h3>Sign in to share</h3>
      <p>Sharing goes through your Tessera account, so friends can find the instance by your invite.</p>
      <button class="btn btn-cta" @click="account.dialog = true"><LogIn /> Sign in</button>
    </div>

    <template v-else-if="instance.joined">
      <div class="card stack">
        <div class="row" style="gap: 12px">
          <span class="tile" style="--type: #8a8aff"><Users /></span>
          <div style="flex: 1; min-width: 0">
            <div style="font-weight: 700; font-size: 16px">Shared by {{ member?.owner ?? instance.joined.owner ?? "a friend" }}</div>
            <div class="hint">
              {{
                member?.removed
                  ? "You're no longer on this pack, or it was deleted. Your copy keeps working."
                  : (member?.role ?? instance.joined.role) === "editor"
                    ? "You can add and remove content for everyone. The Minecraft and loader version stay as the owner set them."
                    : "You get the owner's updates. Ask them to let you add content if you want to change it."
              }}
            </div>
          </div>
          <span v-if="!member?.removed" class="badge" :data-state="(member?.role ?? instance.joined.role) === 'editor' ? 'running' : 'idle'">
            <span class="dot" />{{ (member?.role ?? instance.joined.role) === "editor" ? "Can add content" : "Can play" }}
          </span>
        </div>
        <template v-if="!member?.removed">
          <div class="row live-row">
            <span class="live-dot" :class="live()?.state === 'error' ? 'bad' : live()?.state === 'waiting' ? 'wait' : ''" />
            <span class="hint" style="flex: 1">
              <b>Live.</b>
              {{
                (member?.role ?? instance.joined.role) === "editor"
                  ? "Changes from everyone arrive within seconds, and what you add or remove goes to everyone. Your worlds stay yours."
                  : "Changes arrive within seconds. Your worlds stay yours."
              }}
              <template v-if="live()?.state === 'waiting'"> New changes are applied when you close the game.</template>
              <template v-else-if="live()?.state === 'error'"> Last try failed: {{ live()?.message }}</template>
              <template v-else-if="live()?.state === 'pulled'"> Last update {{ relative(new Date(live()!.at).toISOString()).toLowerCase() }}{{ live()?.by ? ` from ${live()?.by}` : "" }}.</template>
            </span>
          </div>
          <div class="row">
            <button class="btn" :disabled="busy !== ''" @click="syncNow"><Loader2 v-if="busy === 'pull'" class="spin" /><RefreshCw v-else /> Sync now</button>
            <span class="spacer" />
            <button class="btn btn-danger" :disabled="busy !== ''" @click="confirmLeave = true"><LogOut /> Leave</button>
          </div>
        </template>
      </div>
    </template>

    <div v-else-if="!share" class="card empty" style="padding: 64px 24px">
      <UserPlus />
      <h3>No friends invited</h3>
      <p>You can share this instance with your friends! They get the same mods and settings.</p>
      <button class="btn btn-cta" :disabled="busy !== ''" @click="invite">
        <Loader2 v-if="busy === 'create'" class="spin" /><UserPlus v-else /> Invite friends
      </button>
    </div>

    <template v-else>
      <div class="card stack">
        <div class="row">
          <span style="font-weight: 700; font-size: 16px">Shared with {{ share.members?.length ?? 0 }} friend{{ share.members?.length === 1 ? "" : "s" }}</span>
          <span class="spacer" />
          <button class="btn btn-primary" @click="invite"><UserPlus /> Invite friends</button>
        </div>
        <div class="inset row">
          <span class="mono selectable" style="flex: 1; font-size: 13px">{{ share.link }}</span>
          <button class="btn btn-ghost btn-icon btn-sm" title="Copy invite link" @click="copy"><Copy /></button>
        </div>
        <div class="row live-row">
          <span class="live-dot" :class="live()?.state === 'error' ? 'bad' : ''" />
          <span class="hint" style="flex: 1">
            <b>Live.</b> Changes you or your editors make reach everyone within seconds.
            <template v-if="live()?.state === 'error'"> Last try failed: {{ live()?.message }}</template>
            <template v-else-if="live()"> Last sync {{ relative(new Date(live()!.at).toISOString()).toLowerCase() }}.</template>
          </span>
        </div>
        <div class="row">
          <button class="btn" :disabled="busy !== ''" @click="syncNow"><Loader2 v-if="busy === 'pull'" class="spin" /><RefreshCw v-else /> Sync now</button>
          <span class="spacer" />
          <button class="btn btn-danger" :disabled="busy !== ''" @click="confirmStop = true"><Trash2 /> Stop sharing</button>
        </div>
      </div>
    </template>

    <ShareDialog
      v-if="inviting && share"
      :share="share"
      :title="instance.name"
      :instance-id="instance.id"
      @close="inviting = false"
      @changed="load"
      @transferred="(inviting = false), emit('changed')"
    />

    <AppDialog v-if="confirmLeave" title="Leave this shared pack?" @close="confirmLeave = false">
      <p class="muted">You stop getting updates{{ instance.joined?.role === "editor" ? " and can't publish changes" : "" }}. Your copy stays and keeps working.</p>
      <div class="dialog-footer">
        <button class="btn" @click="confirmLeave = false">Cancel</button>
        <button class="btn btn-danger" :disabled="busy === 'leave'" @click="leave">Leave</button>
      </div>
    </AppDialog>

    <AppDialog v-if="confirmStop" title="Stop sharing?" @close="confirmStop = false">
      <p class="muted">The invite link stops working and friends can't install it anymore. Copies they already installed stay on their computers.</p>
      <div class="dialog-footer">
        <button class="btn" @click="confirmStop = false">Cancel</button>
        <button class="btn btn-danger" :disabled="busy === 'stop'" @click="stop">Stop sharing</button>
      </div>
    </AppDialog>
  </div>
</template>

<style scoped>
.live-row {
  gap: 10px;
  align-items: flex-start;
}
.live-dot {
  flex-shrink: 0;
  width: 9px;
  height: 9px;
  margin-top: 5px;
  border-radius: 999px;
  background: var(--fill-green);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--fill-green) 25%, transparent);
  animation: live-pulse 2s ease-in-out infinite;
}
.live-dot.wait {
  background: var(--fill-orange);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--fill-orange) 25%, transparent);
}
.live-dot.bad {
  background: var(--fill-red);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--fill-red) 25%, transparent);
  animation: none;
}
@keyframes live-pulse {
  50% {
    opacity: 0.45;
  }
}
</style>
