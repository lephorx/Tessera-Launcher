<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useRouter } from "vue-router";
import { ArrowDownAZ, ExternalLink, HardDrive, Package, Plus, Power, PowerOff, Search, Trash2, X } from "lucide-vue-next";
import { api, type ContentFile, type Instance } from "@/lib/api";
import { errorText, toast } from "@/lib/toast";
import PlayerHead from "./PlayerHead.vue";
import RowMenu, { type RowMenuItem } from "./RowMenu.vue";

/** Everything the shared pack brings, as a searchable table. Editors change it for everyone from here. */
const props = defineProps<{ instance: Instance; files: ContentFile[]; owner: string | null; canEdit: boolean }>();
const emit = defineEmits<{ close: []; changed: [] }>();
const router = useRouter();

const KIND_LABEL: Record<ContentFile["kind"], [string, string]> = {
  mod: ["Mod", "Mods"],
  "horizon-mod": ["Content mod", "Content mods"],
  "native-mod": ["Native mod", "Native mods"],
  resourcepack: ["Resource pack", "Resource packs"],
  shader: ["Shader", "Shaders"],
  datapack: ["Data pack", "Data packs"],
};
const query = ref("");
const kind = ref<"all" | ContentFile["kind"]>("all");
const sort = ref<"az" | "za" | "off">("az");
const kinds = computed(() => [...new Set(props.files.map((f) => f.kind))]);
const count = (k: ContentFile["kind"]) => props.files.filter((f) => f.kind === k).length;

const shown = computed(() => {
  const words = query.value.toLowerCase().split(/\s+/).filter(Boolean);
  const list = props.files.filter((f) => {
    if (kind.value !== "all" && f.kind !== kind.value) return false;
    const text = `${f.title} ${f.filename} ${f.version ?? ""}`.toLowerCase();
    return words.every((w) => text.includes(w));
  });
  const byName = (a: ContentFile, b: ContentFile) => a.title.localeCompare(b.title, undefined, { sensitivity: "base" });
  if (sort.value === "za") return list.sort((a, b) => byName(b, a));
  if (sort.value === "off") return list.sort((a, b) => Number(a.enabled) - Number(b.enabled) || byName(a, b));
  return list.sort(byName);
});
const totals = computed(() =>
  kinds.value
    .map((k) => `${count(k)} ${(count(k) === 1 ? KIND_LABEL[k][0] : KIND_LABEL[k][1]).toLowerCase()}`)
    .join(" · "),
);

async function act(what: () => Promise<unknown>, done: string) {
  try {
    await what();
    toast.success(done);
    emit("changed");
  } catch (e) {
    toast.error(errorText(e));
  }
}

function menu(f: ContentFile): RowMenuItem[] {
  const items: RowMenuItem[] = [];
  if (f.project_id && props.instance.loader !== "horizon") {
    items.push({ label: "Project page", icon: ExternalLink, action: () => router.push({ path: `/project/${f.project_id}`, query: { instance: props.instance.id } }) });
  }
  if (props.canEdit) {
    items.push(
      f.enabled
        ? { label: "Turn off for everyone", icon: PowerOff, action: () => act(() => api.instances.toggleContent(props.instance.id, f), `${f.title} is off for everyone`) }
        : { label: "Turn on for everyone", icon: Power, action: () => act(() => api.instances.toggleContent(props.instance.id, f), `${f.title} is on for everyone`) },
      {
        label: "Keep only on this computer",
        icon: HardDrive,
        action: () => act(() => api.instances.setLocal(props.instance.id, f, true), `${f.title} stays on this computer and leaves the pack for the others`),
      },
      { label: "Remove for everyone", icon: Trash2, danger: true, action: () => act(() => api.instances.removeContent(props.instance.id, f), `${f.title} removed for everyone`) },
    );
  }
  return items;
}

function addToPack() {
  const inst = props.instance;
  emit("close");
  if (inst.loader === "horizon") router.push(`/instance/${inst.id}/horizon`);
  else router.push({ path: `/instance/${inst.id}/add`, query: { type: inst.loader === "vanilla" ? "resourcepack" : "mod" } });
}

function onKey(e: KeyboardEvent) {
  if (e.key === "Escape" && !document.querySelector(".row-menu")) emit("close");
}
onMounted(() => window.addEventListener("keydown", onKey));
onUnmounted(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <Teleport to="body">
    <div class="backdrop" @mousedown.self="emit('close')">
      <div class="dialog shared-dialog" role="dialog" aria-label="Shared content">
        <div class="dialog-head">
          <div class="row" style="gap: 12px; min-width: 0">
            <PlayerHead v-if="owner" :name="owner" :size="36" />
            <h2>Shared content</h2>
          </div>
          <div class="row">
            <button v-if="canEdit" class="btn btn-sm" title="Add mods and more for everyone on the pack" @click="addToPack"><Plus /> Add to pack</button>
            <button class="dialog-close" aria-label="Close" title="Close" @click="emit('close')"><X /></button>
          </div>
        </div>
        <div class="shared-tools">
          <div class="input-icon">
            <Search />
            <input v-model="query" class="input" :placeholder="`Search ${files.length} projects`" aria-label="Search the shared content" @keydown.esc.stop="query = ''" />
          </div>
          <div class="row" style="flex-wrap: wrap">
            <div class="input-icon sort">
              <ArrowDownAZ />
              <select v-model="sort" class="select" aria-label="Sort">
                <option value="az">Name (A-Z)</option>
                <option value="za">Name (Z-A)</option>
                <option value="off">Turned off first</option>
              </select>
            </div>
            <div class="choices">
              <button class="choice" :class="{ on: kind === 'all' }" @click="kind = 'all'">All</button>
              <button v-for="k in kinds" :key="k" class="choice" :class="{ on: kind === k }" @click="kind = k">{{ KIND_LABEL[k][1] }}</button>
            </div>
          </div>
        </div>
        <div class="shared-table">
          <div class="shared-row shared-head">
            <span>Project</span><span>Version</span><span style="text-align: right">Actions</span>
          </div>
          <p v-if="!shown.length" class="hint" style="padding: 24px; text-align: center">
            {{ files.length ? `Nothing matches “${query.trim()}”.` : "The pack has no mods, resource packs or shaders yet." }}
          </p>
          <div v-for="f in shown" :key="f.kind + f.filename" class="shared-row" :class="{ off: !f.enabled }">
            <div class="row" style="gap: 12px; min-width: 0">
              <img v-if="f.icon_url" :src="f.icon_url" alt="" width="44" height="44" class="shared-icon" />
              <span v-else class="tile" style="--type: #a2a9b4"><Package /></span>
              <div style="min-width: 0">
                <div class="ellipsis" style="font-weight: 600">{{ f.title }}</div>
                <div class="hint">{{ KIND_LABEL[f.kind][0] }}{{ f.enabled ? "" : " · turned off" }}</div>
              </div>
            </div>
            <div style="min-width: 0">
              <div class="ellipsis" style="font-weight: 600">{{ f.version ?? "—" }}</div>
              <div class="hint mono ellipsis">{{ f.filename }}</div>
            </div>
            <div class="row" style="justify-content: flex-end">
              <RowMenu v-if="menu(f).length" :items="menu(f)" :label="`${f.title}: actions`" />
            </div>
          </div>
        </div>
        <div class="shared-foot hint"><Package :size="16" /> {{ totals || "Nothing yet" }}</div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.shared-dialog {
  max-width: 900px;
  height: min(760px, calc(100vh - 48px));
}
.shared-tools {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px 24px;
}
.sort {
  width: 190px;
}
.sort .select {
  padding-left: 34px;
}
.shared-table {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  border-top: 1px solid var(--border);
}
.shared-row {
  display: grid;
  grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr) 80px;
  align-items: center;
  gap: 16px;
  padding: 10px 24px;
  border-bottom: 1px solid var(--border);
}
.shared-head {
  position: sticky;
  top: 0;
  z-index: 1;
  padding-block: 12px;
  background: var(--popover);
  font-weight: 700;
  color: var(--muted-foreground);
}
.shared-row.off > :not(:last-child) {
  opacity: 0.55;
}
.shared-icon {
  flex-shrink: 0;
  border-radius: 10px;
}
.shared-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 14px 24px;
  border-top: 1px solid var(--border);
  font-weight: 600;
}
</style>
