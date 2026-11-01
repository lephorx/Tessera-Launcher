<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { ArrowRight, Check, FileWarning, Loader2, PauseCircle } from "lucide-vue-next";
import { api, type HeldJar, type PackVersions, type ShareReview } from "@/lib/api";
import { LOADERS } from "@/lib/format";
import { bytes } from "@/lib/settingsDialog";
import { refreshReviews } from "@/lib/store";
import { errorText, toast } from "@/lib/toast";

/**
 * The Share tab's part of what this copy held back from the share: mods Tessera doesn't
 * know (waiting for an answer or declined, each can be accepted here) and a version change
 * that pauses syncing.
 */
const props = defineProps<{ id: string }>();
/** paused: a version change waits for an answer, so nothing syncs. */
const emit = defineEmits<{ changed: []; paused: [boolean] }>();
const review = ref<ShareReview | null>(null);
const busy = ref("");

async function load() {
  try {
    review.value = await api.share.review(props.id);
  } catch {
    review.value = null;
  }
  emit("paused", !!review.value?.version_change);
}
const onReview = (e: Event) => (e as CustomEvent<string>).detail === props.id && load();
onMounted(() => {
  void load();
  window.addEventListener("tessera-share-review", onReview);
});
onUnmounted(() => window.removeEventListener("tessera-share-review", onReview));

const fileName = (h: HeldJar) => h.path.split("/").pop() ?? h.path;
const loaderText = (v: PackVersions) => (v.loader === "vanilla" ? "no loader" : `${LOADERS[v.loader].label} ${v.loader_version ?? ""}`.trim());
const versionText = (v: PackVersions) => `Minecraft ${v.minecraft}, ${loaderText(v)}`;

async function decide(h: HeldJar, accept: boolean) {
  busy.value = h.sha512 + accept;
  try {
    review.value = await api.share.decideJars(props.id, accept ? [h.sha512] : [], accept ? [] : [h.sha512]);
    if (accept) {
      toast.success(`Installed ${fileName(h)}`);
      emit("changed");
    }
    void refreshReviews();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = "";
  }
}

async function decideVersion(accept: boolean) {
  busy.value = accept ? "v-yes" : "v-no";
  try {
    await api.share.decideVersion(props.id, accept);
    await load();
    void refreshReviews();
    if (accept) emit("changed");
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = "";
  }
}
</script>

<template>
  <template v-if="review && (review.held.length || review.version_change)">
    <div v-if="review.version_change" class="row paused">
      <PauseCircle :size="18" class="paused-icon" />
      <div style="flex: 1; min-width: 0">
        <div style="font-weight: 600">Syncing is paused</div>
        <div class="hint">
          {{ review.version_change.by ?? "Someone" }} moved it from {{ versionText(review.version_change.from) }}
          <ArrowRight :size="12" style="vertical-align: -1px" /> {{ versionText(review.version_change.to) }}.
          {{ review.version_change.declined ? "You declined. Accept to get it and sync again." : "Accept to get it and sync again." }}
        </div>
      </div>
      <button v-if="!review.version_change.declined" class="btn btn-sm" :disabled="busy !== ''" @click="decideVersion(false)">
        <Loader2 v-if="busy === 'v-no'" class="spin" />Decline
      </button>
      <button class="btn btn-sm btn-primary" :disabled="busy !== ''" @click="decideVersion(true)"><Loader2 v-if="busy === 'v-yes'" class="spin" /><Check v-else /> Accept</button>
    </div>

    <div v-if="review.held.length" class="stack" style="gap: 6px">
      <div style="font-weight: 600">Mods Tessera doesn't know</div>
      <p class="hint" style="margin: 0">Not on Modrinth, so they weren't installed. Accept only what you trust: a mod can do anything on your computer.</p>
      <div class="inset stack" style="gap: 2px">
        <div v-for="h in review.held" :key="h.sha512 + h.path" class="row held">
          <FileWarning :size="16" class="muted" />
          <div style="flex: 1; min-width: 0">
            <div class="ellipsis" style="font-weight: 600" :title="h.path">{{ fileName(h) }}</div>
            <div class="hint ellipsis">{{ bytes(h.size) }} · added by {{ h.by ?? "someone" }} · {{ h.declined ? "Declined" : "Waiting for you" }}</div>
          </div>
          <button v-if="!h.declined" class="btn btn-sm" :disabled="busy !== ''" @click="decide(h, false)">
            <Loader2 v-if="busy === h.sha512 + false" class="spin" />Decline
          </button>
          <button class="btn btn-sm btn-primary" :disabled="busy !== ''" @click="decide(h, true)">
            <Loader2 v-if="busy === h.sha512 + true" class="spin" />Accept
          </button>
        </div>
      </div>
    </div>
  </template>
</template>

<style scoped>
.held {
  gap: 10px;
  padding: 5px 4px;
}
.paused {
  gap: 10px;
  align-items: flex-start;
  padding: 10px 12px;
  border-radius: 10px;
  background: color-mix(in srgb, var(--fill-orange) 12%, transparent);
}
.paused-icon {
  flex-shrink: 0;
  margin-top: 1px;
  color: var(--fill-orange);
}
</style>
