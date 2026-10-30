<script setup lang="ts">
import { onMounted, onUnmounted, ref, type Component } from "vue";
import { Coffee, Gauge, HeartHandshake, Paintbrush, User, Wrench, X } from "lucide-vue-next";
import AppearanceSection from "./settings/AppearanceSection.vue";
import MemorySection from "./settings/MemorySection.vue";
import ProfileSection from "./settings/ProfileSection.vue";
import ResourcesSection from "./settings/ResourcesSection.vue";
import SocialSection from "./settings/SocialSection.vue";
import TroubleshootingSection from "./settings/TroubleshootingSection.vue";
import markUrl from "@/assets/mark.svg";
import { api } from "@/lib/api";
import { settingsDialog, type SettingsSection } from "@/lib/settingsDialog";

/** Settings as a popup: sections on the left, each saving its changes right away. */
const groups: { title: string; items: { id: SettingsSection; label: string; icon: Component; view: Component }[] }[] = [
  { title: "Display", items: [{ id: "appearance", label: "Appearance", icon: Paintbrush, view: AppearanceSection }] },
  {
    title: "Account",
    items: [
      { id: "profile", label: "Profile", icon: User, view: ProfileSection },
      { id: "social", label: "Social", icon: HeartHandshake, view: SocialSection },
    ],
  },
  { title: "Instance defaults", items: [{ id: "memory", label: "Java and memory", icon: Coffee, view: MemorySection }] },
  {
    title: "System",
    items: [
      { id: "resources", label: "Resources", icon: Gauge, view: ResourcesSection },
      { id: "troubleshooting", label: "Troubleshooting", icon: Wrench, view: TroubleshootingSection },
    ],
  },
];
const current = () => groups.flatMap((g) => g.items).find((i) => i.id === settingsDialog.section) ?? groups[0]!.items[0]!;

const info = ref<{ version: string; os: string } | null>(null);
const close = () => (settingsDialog.open = false);
function onKey(e: KeyboardEvent) {
  if (e.key === "Escape") close();
}
onMounted(async () => {
  window.addEventListener("keydown", onKey);
  info.value = await api.system.info().catch(() => null);
});
onUnmounted(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <Teleport to="body">
    <div class="backdrop" @mousedown.self="close">
      <div class="dialog settings-dialog" role="dialog" aria-label="Settings">
        <div class="dialog-head">
          <h2>Settings</h2>
          <button class="dialog-close" aria-label="Close" title="Close" @click="close"><X /></button>
        </div>
        <div class="settings-body">
          <nav class="settings-nav">
            <div class="settings-nav-list">
              <template v-for="g in groups" :key="g.title">
                <span class="settings-group">{{ g.title }}</span>
                <button
                  v-for="i in g.items"
                  :key="i.id"
                  class="settings-link"
                  :class="{ on: settingsDialog.section === i.id }"
                  :aria-current="settingsDialog.section === i.id ? 'page' : undefined"
                  @click="settingsDialog.section = i.id"
                >
                  <component :is="i.icon" :size="18" /> {{ i.label }}
                </button>
              </template>
            </div>
            <div v-if="info" class="settings-about">
              <img :src="markUrl" alt="" width="28" height="28" />
              <span>Tessera Launcher {{ info.version }}<br />{{ info.os }}</span>
            </div>
          </nav>
          <section class="settings-content">
            <component :is="current().view" :key="current().id" />
          </section>
        </div>
      </div>
    </div>
  </Teleport>
</template>
