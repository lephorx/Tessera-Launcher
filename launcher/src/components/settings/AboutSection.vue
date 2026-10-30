<script setup lang="ts">
import { onMounted, ref } from "vue";
import { ExternalLink } from "lucide-vue-next";
import markUrl from "@/assets/mark.svg";
import { api } from "@/lib/api";
import { LEGAL, openLegal } from "@/lib/legal";

/** Version, the legal pages (on the website) and the Mojang disclaimer. */
const info = ref<{ version: string; os: string } | null>(null);
onMounted(async () => (info.value = await api.system.info().catch(() => null)));

const links: { page: keyof typeof LEGAL; label: string }[] = [
  { page: "privacy", label: "Privacy Policy" },
  { page: "terms", label: "Terms" },
  { page: "imprint", label: "Imprint" },
  { page: "licenses", label: "Open-source licenses" },
  { page: "eula", label: "Minecraft EULA" },
];
</script>

<template>
  <div class="set-block">
    <div class="row" style="gap: 12px">
      <img :src="markUrl" alt="" width="40" height="40" />
      <div>
        <h3 class="set-title">Tessera Launcher</h3>
        <p v-if="info" class="set-hint num">Version {{ info.version }} · {{ info.os }}</p>
      </div>
    </div>
  </div>

  <div class="set-block">
    <h3 class="set-title">Legal</h3>
    <div class="set-table">
      <a v-for="l in links" :key="l.page" class="set-row about-link" :href="LEGAL[l.page]" @click.prevent="openLegal(l.page)">
        <span style="font-weight: 600">{{ l.label }}</span>
        <span class="spacer" />
        <ExternalLink :size="15" class="muted" />
      </a>
    </div>
    <p class="set-hint">Questions about your data: privacy@tesseralauncher.com</p>
  </div>

  <div class="set-block">
    <p class="set-hint">
      Tessera is not an official Minecraft product. Not approved by or associated with Mojang or Microsoft.
    </p>
    <p class="set-hint">Mod listings come from Modrinth; Tessera is not affiliated with Modrinth.</p>
  </div>
</template>

<style scoped>
.about-link {
  color: var(--foreground);
  text-decoration: none;
}
.about-link:hover {
  background: var(--fill);
}
</style>
