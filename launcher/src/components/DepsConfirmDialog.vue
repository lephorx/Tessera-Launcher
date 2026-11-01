<script setup lang="ts">
import { computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Download } from "lucide-vue-next";
import { depsQuestions } from "@/lib/store";
import { errorText, toast } from "@/lib/toast";
import AppDialog from "./AppDialog.vue";

// Mods a mod needs that Modrinth only found by name. Anyone can publish a project with a
// look-alike name, so nothing is installed until the player says yes (a launch waits here).
const question = computed(() => depsQuestions[0] ?? null);

async function answer(install: boolean) {
  const q = depsQuestions.shift();
  if (!q) return;
  try {
    await invoke("deps_confirm_answer", { id: q.id, install });
  } catch (e) {
    toast.error(errorText(e));
  }
}

const count = (n: number | null) => (n == null ? "" : n >= 1_000_000 ? `${(n / 1_000_000).toFixed(1)}M downloads` : n >= 1000 ? `${Math.round(n / 1000)}k downloads` : `${n} downloads`);
</script>

<template>
  <AppDialog v-if="question" title="Install missing mods?" :width="520" @close="answer(false)">
    <div class="stack" style="gap: 12px">
      <p class="muted">
        {{ question.name ? `${question.name} needs` : "These mods need" }} mods that aren't there. Tessera found these on Modrinth by
        their name only, so check they're the right ones before installing them (with what they need).
      </p>
      <div class="inset stack" style="gap: 8px; max-height: 260px; overflow-y: auto">
        <div v-for="m in question.items" :key="m.project_id" class="stack" style="gap: 2px">
          <span><b>{{ m.title }}</b> <span class="hint mono">modrinth.com/mod/{{ m.slug }}</span></span>
          <span class="hint">
            For {{ m.needed_by }} (needs "{{ m.mod_id }}")<template v-if="m.author"> · by {{ m.author }}</template
            ><template v-if="m.downloads != null"> · {{ count(m.downloads) }}</template>
          </span>
        </div>
      </div>
      <div class="dialog-footer" style="margin-top: 0">
        <button class="btn" @click="answer(false)">Don't install</button>
        <button class="btn btn-primary" @click="answer(true)"><Download /> Install</button>
      </div>
    </div>
  </AppDialog>
</template>
