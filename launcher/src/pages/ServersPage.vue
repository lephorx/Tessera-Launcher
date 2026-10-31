<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { RouterLink } from "vue-router";
import { listen } from "@tauri-apps/api/event";
import { Laptop, Plus, Server } from "lucide-vue-next";
import { api, type EasyServer, type Machine } from "@/lib/api";

const machines = ref<Machine[] | null>(null);
const local = ref<EasyServer[] | null>(null);
const KIND: Record<string, string> = { vanilla: "Vanilla", paper: "Paper", fabric: "Fabric", forge: "Forge", neoforge: "NeoForge" };
const STATE: Record<string, { key: string; label: string }> = {
  stopped: { key: "idle", label: "Stopped" },
  starting: { key: "preparing", label: "Starting…" },
  running: { key: "running", label: "Online" },
  stopping: { key: "preparing", label: "Stopping…" },
};

let unlisten: (() => void) | undefined;
onMounted(async () => {
  [machines.value, local.value] = await Promise.all([api.machines.list(), api.easy.list()]);
  unlisten = await listen<{ id: string; line: string | null }>("easy-server", async (e) => {
    if (e.payload.line === null) local.value = await api.easy.list();
  });
});
onUnmounted(() => unlisten?.());
</script>

<template>
  <div class="page enter">
    <div class="page-header">
      <div>
        <h1 class="page-title">Servers</h1>
        <p class="page-sub">Servers on this computer, and machines running Tessera.</p>
      </div>
      <RouterLink to="/servers/host" class="btn btn-primary btn-lg"><Plus /> Host a server</RouterLink>
    </div>

    <template v-if="local && local.length">
      <h2 class="section-title" style="margin-top: 0">On this computer</h2>
      <div class="grid" style="margin-bottom: 24px">
        <RouterLink v-for="s in local" :key="s.id" :to="`/servers/local/${s.id}`" class="card card-hover stack" style="color: inherit">
          <div class="row" style="gap: 12px">
            <span class="tile" style="--type: #7dd87d"><Laptop /></span>
            <div style="min-width: 0">
              <div class="ellipsis" style="font-weight: 700; font-size: 16px">{{ s.name }}</div>
              <div class="hint" style="margin-top: 2px">{{ KIND[s.kind] }} {{ s.game_version }}</div>
            </div>
          </div>
          <div class="row">
            <span class="badge" :data-state="STATE[s.state]!.key"><span class="dot" />{{ STATE[s.state]!.label }}</span>
            <span class="spacer" />
            <span v-if="s.state === 'running'" class="hint num">{{ s.players.length }} online</span>
          </div>
        </RouterLink>
      </div>
    </template>

    <h2 v-if="local && local.length && machines && machines.length" class="section-title">Tessera machines</h2>
    <div v-if="machines && machines.length" class="grid">
      <RouterLink v-for="m in machines" :key="m.id" :to="`/servers/${m.id}`" class="card card-hover stack" style="color: inherit">
        <div class="row" style="gap: 12px">
          <span class="tile" style="--type: #959eef"><Server /></span>
          <div style="min-width: 0">
            <div style="font-weight: 700; font-size: 16px">{{ m.name }}</div>
            <div class="hint mono" style="margin-top: 2px">{{ m.url }}</div>
          </div>
        </div>
        <div class="row">
          <span class="badge" :data-state="m.signed_in ? 'running' : 'idle'"><span class="dot" />{{ m.signed_in ? "Signed in" : "Sign in again" }}</span>
          <span class="spacer" />
          <span class="hint">as {{ m.username }}</span>
        </div>
      </RouterLink>
    </div>
    <div v-else-if="machines && local && !local.length" class="card empty">
      <Server />
      <h3>No servers yet</h3>
      <p>Start one on this computer in a few clicks, or connect a machine running Tessera.</p>
      <RouterLink to="/servers/host" class="btn btn-primary"><Plus /> Host a server</RouterLink>
    </div>
  </div>
</template>
