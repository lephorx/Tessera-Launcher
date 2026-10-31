<script lang="ts">
import { ref } from "vue";
import type { Friend, NewsItem } from "@/lib/api";

// Kept between visits to Home, so going back shows them at once and refreshes quietly.
const friends = ref<Friend[] | null>(null);
const news = ref<NewsItem[] | null>(null);
let newsLoadedAt = 0;
</script>

<script setup lang="ts">
import { computed, onMounted, watch } from "vue";
import { Check, ChevronDown, ExternalLink, Loader2, Plus, Search, Trash2, UserPlus, X } from "lucide-vue-next";
import { openUrl } from "@tauri-apps/plugin-opener";
import MicrosoftLogo from "./MicrosoftLogo.vue";
import PlayerHead from "./PlayerHead.vue";
import { api } from "@/lib/api";
import { account } from "@/lib/account";
import { player, removeMicrosoft, signInMicrosoft, useMicrosoft } from "@/lib/player";
import { errorText, toast } from "@/lib/toast";

/** The sidebar on the right of every page: the Minecraft account you play as, Tessera friends, and launcher news. */

// ── Playing as ──
const accountsOpen = ref(false);

async function run(f: () => Promise<void>) {
  try {
    await f();
  } catch (e) {
    toast.error(errorText(e));
  }
}

// ── Friends ──
const friendsError = ref("");
const query = ref("");
const addingFriend = ref(false);
const friendName = ref("");
const busy = ref(false);
const pendingOpen = ref(true);

async function loadFriends() {
  if (!account.me) {
    friends.value = null;
    return;
  }
  try {
    friends.value = await api.friends.list();
    friendsError.value = "";
  } catch (e) {
    friendsError.value = errorText(e);
  }
}
watch(() => account.me?.id, (id, before) => {
  if (id !== before) friends.value = null;
  void loadFriends();
});

const matches = (f: Friend) => f.username.toLowerCase().includes(query.value.trim().toLowerCase());
const accepted = computed(() => (friends.value ?? []).filter((f) => f.status === "friend" && matches(f)));
const pending = computed(() =>
  (friends.value ?? []).filter((f) => f.status !== "friend" && matches(f)).sort((a, b) => (a.status === "incoming" ? -1 : 1) - (b.status === "incoming" ? -1 : 1)),
);

async function sendRequest() {
  busy.value = true;
  try {
    const f = await api.friends.add(friendName.value.trim());
    toast.success(f.status === "friend" ? `You and ${f.username} are friends now` : `Friend request sent to ${f.username}`);
    friendName.value = "";
    addingFriend.value = false;
    await loadFriends();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = false;
  }
}

async function accept(f: Friend) {
  await run(() => api.friends.accept(f.user_id));
  await loadFriends();
}

async function remove(f: Friend) {
  await run(() => api.friends.remove(f.user_id));
  await loadFriends();
}

// ── News ──
const newsError = ref(false);
const imageFailed = ref(new Set<number>());
const day = (iso: string) => new Date(iso).toLocaleDateString(undefined, { month: "long", day: "numeric", year: "numeric" });

onMounted(async () => {
  void loadFriends();
  if (Date.now() - newsLoadedAt < 10 * 60_000) return;
  try {
    news.value = await api.news();
    newsLoadedAt = Date.now();
  } catch {
    if (!news.value) newsError.value = true;
  }
});
</script>

<template>
  <aside class="side">
    <section class="side-section">
      <h3 class="side-title">Playing as</h3>
      <div v-if="!player.microsoft" class="side-card side-signin">
        <p class="hint">Add your Minecraft account to play. You sign in with the Microsoft account you bought Minecraft with.</p>
        <button class="btn btn-primary side-wide" :disabled="player.signingIn" @click="signInMicrosoft">
          <Loader2 v-if="player.signingIn" class="spin" /><MicrosoftLogo v-else /> Add account
        </button>
      </div>
      <div v-else class="side-card">
        <button class="side-player" :aria-expanded="accountsOpen" @click="accountsOpen = !accountsOpen">
          <PlayerHead :name="player.name" :size="36" />
          <span class="side-player-text">
            <strong>{{ player.name }}</strong>
            <span class="hint">Minecraft account</span>
          </span>
          <ChevronDown :size="18" class="muted side-chevron" :class="{ open: accountsOpen }" />
        </button>
        <div class="side-flow" :class="{ open: accountsOpen }" :inert="!accountsOpen">
        <div class="side-flow-inner">
        <div class="side-accounts">
          <div v-for="a in player.accounts" :key="a.id" class="side-account" :class="{ on: a.id === player.microsoft }">
            <button class="side-account-pick" :title="`Play as ${a.name}`" @click="run(() => useMicrosoft(a))">
              <span class="side-radio"><Check v-if="a.id === player.microsoft" :size="12" :stroke-width="3" /></span>
              <PlayerHead :name="a.name" :size="24" />
              <span class="ellipsis">{{ a.name }}</span>
            </button>
            <button class="btn btn-ghost btn-icon btn-sm" :title="`Sign out ${a.name}`" :aria-label="`Sign out ${a.name}`" @click="run(() => removeMicrosoft(a))">
              <Trash2 />
            </button>
          </div>
          <button class="btn side-wide" :disabled="player.signingIn" @click="signInMicrosoft">
            <Loader2 v-if="player.signingIn" class="spin" /><Plus v-else /> Add account
          </button>
        </div>
        </div>
        </div>
      </div>
    </section>

    <section class="side-section">
      <div class="row" style="gap: 8px">
        <h3 class="side-title" style="margin: 0">Friends</h3>
        <span class="spacer" />
        <button v-if="account.me?.username" class="btn btn-ghost btn-icon btn-sm" title="Add a friend" aria-label="Add a friend" @click="addingFriend = !addingFriend">
          <UserPlus />
        </button>
      </div>

      <template v-if="!account.loaded">
        <p class="hint row"><Loader2 class="spin" :size="14" /> Loading…</p>
      </template>
      <template v-else-if="!account.me">
        <p class="hint">Sign in to your Tessera account to add friends and share packs with them.</p>
        <button class="btn btn-primary side-wide" @click="account.dialog = true">Sign in</button>
      </template>
      <template v-else-if="!account.me.username">
        <p class="hint">Pick a username in Settings so friends can find you.</p>
      </template>
      <template v-else>
        <form v-if="addingFriend" class="row" style="gap: 6px" @submit.prevent="sendRequest">
          <input v-model="friendName" class="input" placeholder="Their username" aria-label="Friend's Tessera username" maxlength="24" required autofocus />
          <button class="btn btn-primary" :disabled="busy"><Loader2 v-if="busy" class="spin" />Add</button>
        </form>
        <div class="input-icon"><Search /><input v-model="query" class="input" placeholder="Search friends…" aria-label="Search friends" /></div>
        <p v-if="friendsError" class="error-text">{{ friendsError }}</p>

        <template v-if="friends && pending.length">
          <button class="side-fold" :aria-expanded="pendingOpen" @click="pendingOpen = !pendingOpen">
            Pending · {{ pending.length }}
            <ChevronDown :size="16" class="side-chevron" :class="{ open: pendingOpen }" />
          </button>
          <template v-if="pendingOpen">
            <div v-for="f in pending" :key="f.user_id" class="side-friend">
              <PlayerHead :name="f.username" :size="28" />
              <span class="side-friend-text">
                <span class="ellipsis" style="font-weight: 600">{{ f.username }}</span>
                <span class="hint">{{ f.status === "incoming" ? "Wants to be friends" : "Request sent" }}</span>
              </span>
              <button v-if="f.status === 'incoming'" class="btn btn-ghost btn-icon btn-sm" title="Accept" aria-label="Accept" @click="accept(f)"><Check /></button>
              <button class="btn btn-ghost btn-icon btn-sm" :title="f.status === 'incoming' ? 'Decline' : 'Cancel request'" :aria-label="f.status === 'incoming' ? 'Decline' : 'Cancel request'" @click="remove(f)">
                <X />
              </button>
            </div>
          </template>
        </template>

        <div v-for="f in accepted" :key="f.user_id" class="side-friend">
          <PlayerHead :name="f.username" :size="28" />
          <span class="side-friend-text"><span class="ellipsis" style="font-weight: 600">{{ f.username }}</span></span>
          <button class="btn btn-ghost btn-icon btn-sm side-friend-remove" :title="`Remove ${f.username}`" :aria-label="`Remove ${f.username}`" @click="remove(f)"><X /></button>
        </div>
        <p v-if="friends && !friends.length" class="hint">No friends yet. Add someone by their Tessera username.</p>
        <p v-else-if="friends && query && !accepted.length && !pending.length" class="hint">No friend called “{{ query }}”.</p>
      </template>
    </section>

    <section class="side-section">
      <h3 class="side-title">News</h3>
      <p v-if="newsError" class="hint">Couldn't load the news.</p>
      <template v-else-if="!news">
        <div v-for="i in 2" :key="i" class="side-news side-news-skeleton" aria-hidden="true">
          <span class="side-news-image" />
          <span class="skeleton-line" style="width: 60%" />
          <span class="skeleton-line" />
          <span class="skeleton-line" style="width: 80%" />
        </div>
      </template>
      <p v-else-if="!news.length" class="hint">Nothing new.</p>
      <component
        :is="n.url ? 'button' : 'article'"
        v-for="n in news ?? []"
        :key="n.id"
        class="side-news"
        :class="{ link: n.url }"
        @click="n.url && openUrl(n.url)"
      >
        <img
          v-if="n.image_url && !imageFailed.has(n.id)"
          class="side-news-image"
          :src="n.image_url"
          alt=""
          loading="lazy"
          draggable="false"
          @error="imageFailed.add(n.id)"
        />
        <strong>{{ n.title }} <ExternalLink v-if="n.url" :size="13" /></strong>
        <span class="muted">{{ n.body }}</span>
        <span class="hint">{{ day(n.published_at) }}</span>
      </component>
    </section>
  </aside>
</template>
