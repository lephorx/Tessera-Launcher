<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { useRouter } from "vue-router";
import { Bell, Check, Package, UserCheck, UserPlus, X } from "lucide-vue-next";
import { api, type Notice } from "@/lib/api";
import { relative } from "@/lib/format";
import { markSeen, notices, refreshNotices } from "@/lib/notifications";
import { errorText, toast } from "@/lib/toast";

/** The bell in the top bar: friend requests (accept or decline right here), accepted requests and shared packs. */
const router = useRouter();
const open = ref(false);
const busy = ref<string | null>(null);

function toggle() {
  open.value = !open.value;
  if (open.value) void markSeen();
}

function close(e: MouseEvent) {
  if (!(e.target as HTMLElement).closest(".bell-wrap")) open.value = false;
}
onMounted(() => window.addEventListener("mousedown", close));
onUnmounted(() => window.removeEventListener("mousedown", close));

async function answer(n: Notice, accept: boolean) {
  busy.value = n.user_id;
  try {
    if (accept) await api.friends.accept(n.user_id);
    else await api.friends.remove(n.user_id);
    if (accept) toast.success(`You and ${n.username} are friends`);
    await refreshNotices();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = null;
  }
}

function openShared() {
  open.value = false;
  void router.push("/library");
}

const head = (name: string) => `https://mc-heads.net/avatar/${encodeURIComponent(name)}/28`;
</script>

<template>
  <div class="bell-wrap">
    <button class="btn btn-ghost btn-icon bell" :title="notices.unread ? `${notices.unread} new` : 'Notifications'" aria-label="Notifications" :aria-expanded="open" @click="toggle">
      <Bell />
      <span v-if="notices.unread" class="bell-count num">{{ notices.unread > 9 ? "9+" : notices.unread }}</span>
    </button>
    <div v-if="open" class="account-menu bell-menu" role="menu">
      <div class="account-menu-head"><strong>Notifications</strong></div>
      <p v-if="!notices.list.length" class="hint" style="padding: 10px; margin: 0">Nothing new. Friend requests and packs friends share with you show up here.</p>
      <div v-for="n in notices.list" :key="`${n.kind}:${n.user_id}:${n.code}:${n.at}`" class="notice">
        <img :src="head(n.username)" width="28" height="28" alt="" />
        <div class="stack" style="gap: 2px; flex: 1; min-width: 0">
          <span v-if="n.kind === 'friend_request'"><UserPlus :size="13" /> <b>{{ n.username }}</b> wants to be friends</span>
          <span v-else-if="n.kind === 'friend_accepted'"><UserCheck :size="13" /> <b>{{ n.username }}</b> accepted your friend request</span>
          <span v-else><Package :size="13" /> <b>{{ n.username }}</b> shared <b>{{ n.name }}</b> with you</span>
          <span class="hint">{{ relative(n.at) }}</span>
          <div v-if="n.kind === 'friend_request'" class="row" style="gap: 6px; margin-top: 4px">
            <button class="btn btn-primary btn-sm" :disabled="busy === n.user_id" @click="answer(n, true)"><Check /> Accept</button>
            <button class="btn btn-sm" :disabled="busy === n.user_id" @click="answer(n, false)"><X /> Decline</button>
          </div>
          <button v-else-if="n.kind === 'share_added'" class="btn btn-sm" style="align-self: flex-start; margin-top: 4px" @click="openShared">Get it in the Library</button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.bell-wrap {
  position: relative;
}
.bell {
  position: relative;
}
.bell-count {
  position: absolute;
  top: 2px;
  right: 2px;
  min-width: 16px;
  height: 16px;
  padding: 0 4px;
  border-radius: 999px;
  background: var(--fill-red);
  color: #fff;
  font-size: 10px;
  font-weight: 800;
  line-height: 16px;
  text-align: center;
}
.bell-menu {
  width: 340px;
  max-height: 460px;
  overflow-y: auto;
}
.notice {
  display: flex;
  gap: 10px;
  padding: 10px;
  border-radius: 12px;
  font-size: 13px;
}
.notice + .notice {
  border-top: 1px solid var(--border);
}
.notice img {
  border-radius: 6px;
  image-rendering: pixelated;
}
.notice svg {
  vertical-align: -2px;
  opacity: 0.7;
}
</style>
