<script setup lang="ts">
import { onMounted, ref } from "vue";
import { RouterLink } from "vue-router";
import { Plus, Server } from "lucide-vue-next";
import { api, type Machine } from "@/lib/api";

const machines = ref<Machine[] | null>(null);
onMounted(async () => {
  machines.value = await api.machines.list();
});
</script>

<template>
  <div class="page enter">
    <div class="page-header">
      <div>
        <h1 class="page-title">Servers</h1>
        <p class="page-sub">Machines running CSCM that this launcher manages.</p>
      </div>
      <RouterLink to="/servers/host" class="btn btn-primary btn-lg"><Plus /> Host a server</RouterLink>
    </div>

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
    <div v-else-if="machines" class="card empty">
      <Server />
      <h3>No servers yet</h3>
      <p>Host one on this computer, on another machine, or add a CSCM you already run.</p>
      <RouterLink to="/servers/host" class="btn btn-primary"><Plus /> Host a server</RouterLink>
    </div>
  </div>
</template>
