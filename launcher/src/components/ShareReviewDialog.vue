<script setup lang="ts">
import { computed, ref } from "vue";
import { ArrowRight, Check, FileWarning, Loader2, X } from "lucide-vue-next";
import { api, type HeldJar, type PackVersions, type ShareReview } from "@/lib/api";
import { LOADERS } from "@/lib/format";
import { bytes } from "@/lib/settingsDialog";
import { refreshReviews, shareReviews } from "@/lib/store";
import { errorText, toast } from "@/lib/toast";
import AppDialog from "./AppDialog.vue";

/**
 * Asks before a shared pack's update brings in something Tessera can't vouch for: a jar that
 * isn't a Modrinth file, or another Minecraft or loader version. One instance at a time;
 * closing it means "later" (the Share tab still lists what's waiting).
 */
const later = ref(new Set<string>());
const busy = ref("");

const current = computed<ShareReview | null>(() => shareReviews.list.find((r) => !later.value.has(key(r))) ?? null);
const jars = computed(() => current.value?.held.filter((h) => !h.declined) ?? []);
const version = computed(() => (current.value?.version_change && !current.value.version_change.declined ? current.value.version_change : null));

/** Asked again when something new comes, even after "later". */
function key(r: ShareReview) {
  return `${r.instance}:${r.held.map((h) => h.sha512.slice(0, 12)).join(",")}:${r.version_change?.revision ?? ""}`;
}

const fileName = (h: HeldJar) => h.path.split("/").pop() ?? h.path;
const folder = (h: HeldJar) => h.path.split("/").slice(0, -1).join("/") || "the game folder";
const someone = (by: string | null | undefined) => by ?? "Someone";

const jarTitle = computed(() => {
  const who = new Set(jars.value.map((j) => j.by));
  const by = who.size === 1 ? someone(jars.value[0]?.by) : "Friends";
  return jars.value.length === 1 ? `${by} added a mod Tessera doesn't know` : `${by} added ${jars.value.length} mods Tessera doesn't know`;
});

const loaderText = (v: PackVersions) => (v.loader === "vanilla" ? "No loader" : `${LOADERS[v.loader].label} ${v.loader_version ?? ""}`.trim());
const mcChanged = computed(() => !!version.value && version.value.from.minecraft !== version.value.to.minecraft);
const loaderChanged = computed(() => !!version.value && loaderText(version.value.from) !== loaderText(version.value.to));
const versionTitle = computed(() => {
  const by = someone(version.value?.by);
  if (mcChanged.value && loaderChanged.value) return `${by} changed the Minecraft and loader version`;
  return mcChanged.value ? `${by} changed the Minecraft version` : `${by} changed the loader`;
});

async function decide(accept: string[], decline: string[]) {
  const r = current.value;
  if (!r) return;
  busy.value = accept.length + decline.length > 1 ? (accept.length ? "all-yes" : "all-no") : (accept[0] ?? decline[0] ?? "");
  try {
    await api.share.decideJars(r.instance, accept, decline);
    if (accept.length) toast.success(accept.length === 1 ? "Installed. It's in the game the next time you start it." : `Installed ${accept.length} mods`);
    window.dispatchEvent(new CustomEvent("tessera-instance-changed", { detail: r.instance }));
    window.dispatchEvent(new CustomEvent("tessera-share-review", { detail: r.instance }));
    await refreshReviews();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = "";
  }
}

async function decideVersion(accept: boolean) {
  const r = current.value;
  if (!r) return;
  busy.value = accept ? "v-yes" : "v-no";
  try {
    await api.share.decideVersion(r.instance, accept);
    if (!accept) toast.info(`${r.name} stops syncing until you accept the change in its Share tab`);
    window.dispatchEvent(new CustomEvent("tessera-share-review", { detail: r.instance }));
    await refreshReviews();
  } catch (e) {
    toast.error(errorText(e));
  } finally {
    busy.value = "";
  }
}

function close() {
  if (current.value) later.value = new Set(later.value).add(key(current.value));
}
</script>

<template>
  <AppDialog v-if="current && version" :title="versionTitle" :width="540" @close="close">
    <div class="stack" style="gap: 14px">
      <p class="hint" style="margin: 0">
        <b>{{ current.name }}</b> moved to another version. Accept to get it with the rest of the changes; until then, {{ current.name }} doesn't sync.
      </p>
      <div class="inset stack" style="gap: 10px">
        <div class="row change" :class="{ same: !mcChanged }">
          <span class="label">Minecraft</span>
          <span class="num">{{ version.from.minecraft }}</span><ArrowRight :size="14" class="muted" /><b class="num">{{ version.to.minecraft }}</b>
        </div>
        <div class="row change" :class="{ same: !loaderChanged }">
          <span class="label">Loader</span>
          <span>{{ loaderText(version.from) }}</span><ArrowRight :size="14" class="muted" /><b>{{ loaderText(version.to) }}</b>
        </div>
      </div>
      <p class="hint" style="margin: 0">
        Changed by {{ someone(version.by) }}. Mods may need other versions for it, and a world opened in a newer Minecraft can't go back to an older one.
      </p>
      <div class="dialog-footer">
        <button class="btn" :disabled="busy !== ''" @click="decideVersion(false)"><Loader2 v-if="busy === 'v-no'" class="spin" /><X v-else /> Decline</button>
        <button class="btn btn-primary" :disabled="busy !== ''" @click="decideVersion(true)"><Loader2 v-if="busy === 'v-yes'" class="spin" /><Check v-else /> Accept</button>
      </div>
    </div>
  </AppDialog>

  <AppDialog v-else-if="current && jars.length" :title="jarTitle" :width="540" @close="close">
    <div class="stack" style="gap: 14px">
      <p class="hint" style="margin: 0">
        <b>{{ current.name }}</b> got an update with {{ jars.length === 1 ? "a mod that isn't" : "mods that aren't" }} on Modrinth, so Tessera can't check
        {{ jars.length === 1 ? "it" : "them" }}. A mod can do anything on your computer: accept only what you trust. The rest of the update is installed.
      </p>
      <div class="inset stack jars">
        <div v-for="j in jars" :key="j.sha512 + j.path" class="row jar">
          <span class="tile tile-sm" style="--type: #f2a76d"><FileWarning /></span>
          <div style="flex: 1; min-width: 0">
            <div class="ellipsis" style="font-weight: 600" :title="j.path">{{ fileName(j) }}</div>
            <div class="hint ellipsis">{{ bytes(j.size) }} · in {{ folder(j) }} · added by {{ someone(j.by) }}</div>
          </div>
          <button class="btn btn-sm" :disabled="busy !== ''" :aria-label="`Decline ${fileName(j)}`" @click="decide([], [j.sha512])">
            <Loader2 v-if="busy === j.sha512" class="spin" />Decline
          </button>
          <button class="btn btn-sm btn-primary" :disabled="busy !== ''" :aria-label="`Accept ${fileName(j)}`" @click="decide([j.sha512], [])">Accept</button>
        </div>
      </div>
      <p class="hint" style="margin: 0">Declined mods stay out of your copy on later updates too. You can still accept them in the instance's Share tab.</p>
      <div class="dialog-footer">
        <button v-if="jars.length > 1" class="btn" :disabled="busy !== ''" @click="decide([], jars.map((j) => j.sha512))">
          <Loader2 v-if="busy === 'all-no'" class="spin" /><X v-else /> Decline all
        </button>
        <button v-if="jars.length > 1" class="btn btn-primary" :disabled="busy !== ''" @click="decide(jars.map((j) => j.sha512), [])">
          <Loader2 v-if="busy === 'all-yes'" class="spin" /><Check v-else /> Accept all
        </button>
        <button v-else class="btn" @click="close">Later</button>
      </div>
    </div>
  </AppDialog>
</template>

<style scoped>
.jars {
  gap: 2px;
  max-height: 320px;
  overflow-y: auto;
}
.jar {
  gap: 10px;
  padding: 6px 4px;
}
.change {
  gap: 8px;
}
.change .label {
  width: 90px;
  color: var(--muted-foreground);
  font-weight: 600;
}
.change.same {
  opacity: 0.55;
}
</style>
