<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { Check, Loader2, Search, UserPlus } from "lucide-vue-next";
import { api, type Friend, type Share, type ShareRole } from "@/lib/api";
import { errorText, toast } from "@/lib/toast";
import AppDialog from "./AppDialog.vue";
import PlayerHead from "./PlayerHead.vue";

/** Picks friends to share a pack with: packs only go to friends, at most MAX_MEMBERS. */
const props = defineProps<{ share: Share; title: string }>();
const emit = defineEmits<{ close: []; changed: [] }>();

/** The API's limit per pack. */
const MAX_MEMBERS = 10;

const friends = ref<Friend[] | null>(null);
const picked = ref(new Map<string, ShareRole>());
const query = ref("");
const busy = ref(false);
const error = ref("");

onMounted(async () => {
  try {
    friends.value = (await api.friends.list()).filter((f) => f.status === "friend");
  } catch (e) {
    error.value = errorText(e);
    friends.value = [];
  }
});

const onIt = computed(() => new Set((props.share.members ?? []).map((m) => m.user_id)));
const room = computed(() => MAX_MEMBERS - onIt.value.size);
const choices = computed(() => {
  const q = query.value.trim().toLowerCase();
  return (friends.value ?? []).filter((f) => !onIt.value.has(f.user_id) && f.username.toLowerCase().includes(q));
});

function toggle(f: Friend) {
  const m = new Map(picked.value);
  if (m.has(f.user_id)) m.delete(f.user_id);
  else if (m.size < room.value) m.set(f.user_id, "viewer");
  else toast.error(`A pack can be shared with ${MAX_MEMBERS} friends`);
  picked.value = m;
}

function setRole(id: string, role: ShareRole) {
  picked.value = new Map(picked.value).set(id, role);
}

async function add() {
  busy.value = true;
  error.value = "";
  const names: string[] = [];
  try {
    for (const [id, role] of picked.value) {
      const f = friends.value!.find((x) => x.user_id === id)!;
      await api.share.addFriend(props.share.code, f.username, role);
      names.push(f.username);
    }
    toast.success(`Shared with ${names.join(", ")}`);
    emit("changed");
    emit("close");
  } catch (e) {
    error.value = errorText(e);
    if (names.length) emit("changed");
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <AppDialog :title="`Share ${title}`" :width="500" @close="emit('close')">
    <div class="stack" style="gap: 14px">
      <p class="hint" style="margin: 0">Pick friends to share it with: {{ onIt.size }} of {{ MAX_MEMBERS }} so far. They get the same mods and settings, and your changes.</p>
      <div v-if="friends && friends.length" class="input-icon"><Search /><input v-model="query" class="input" placeholder="Search friends" aria-label="Search friends" /></div>
      <div class="inset stack picker">
        <div v-if="!friends" class="empty"><Loader2 class="spin" /></div>
        <p v-else-if="!friends.length" class="hint" style="text-align: center; margin: 12px 0">
          You have no friends on Tessera yet. Add them by username in the sidebar; once they accept, you can share with them.
        </p>
        <p v-else-if="!choices.length" class="hint" style="text-align: center; margin: 12px 0">{{ query ? `No friend called “${query}”.` : "It's shared with all your friends already." }}</p>
        <div v-for="f in choices" :key="f.user_id" class="pick row" :class="{ on: picked.has(f.user_id) }" @click="toggle(f)">
          <span class="check"><Check v-if="picked.has(f.user_id)" :size="14" /></span>
          <PlayerHead :name="f.username" :size="26" />
          <span style="flex: 1; font-weight: 600">{{ f.username }}</span>
          <select v-if="picked.has(f.user_id)" class="select role" :value="picked.get(f.user_id)" :aria-label="`What ${f.username} may do`" @click.stop @change="setRole(f.user_id, ($event.target as HTMLSelectElement).value as ShareRole)">
            <option value="viewer">Can play</option>
            <option value="editor">Can add content</option>
          </select>
        </div>
      </div>
      <p v-if="error" class="error-text" style="margin: 0">{{ error }}</p>
      <p class="hint" style="margin: 0">
        <b>Can play</b>: installs the pack and gets your updates. <b>Can add content</b>: may also add and remove mods, resource packs, shaders and configs for
        everyone, but never change the Minecraft or loader version.
      </p>
      <div class="dialog-footer">
        <button class="btn" @click="emit('close')">Cancel</button>
        <button class="btn btn-primary" :disabled="busy || !picked.size" @click="add">
          <Loader2 v-if="busy" class="spin" /><UserPlus v-else /> {{ picked.size ? `Share with ${picked.size} friend${picked.size === 1 ? "" : "s"}` : "Share" }}
        </button>
      </div>
    </div>
  </AppDialog>
</template>

<style scoped>
.picker {
  gap: 2px;
  max-height: 320px;
  overflow-y: auto;
  min-height: 80px;
}
.pick {
  gap: 10px;
  padding: 6px 8px;
  border-radius: 10px;
  cursor: pointer;
}
.pick:hover,
.pick.on {
  background: var(--muted);
}
.check {
  display: grid;
  place-items: center;
  width: 20px;
  height: 20px;
  border-radius: 6px;
  border: 1px solid var(--border);
}
.pick.on .check {
  background: var(--tint);
  border-color: var(--tint);
  color: #fff;
}
.role {
  width: auto;
  height: 30px;
  padding: 0 28px 0 10px;
  font-size: 13px;
}
</style>
