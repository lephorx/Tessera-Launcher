<script setup lang="ts">
import { onMounted, ref } from "vue";
import { RouterLink } from "vue-router";
import { Compass, Plus, Server } from "lucide-vue-next";
import InstanceCard from "@/components/InstanceCard.vue";
import CreateInstanceDialog from "@/components/CreateInstanceDialog.vue";
import { api, type Instance } from "@/lib/api";
import { player } from "@/lib/player";

const recent = ref<Instance[] | null>(null);
const creating = ref(false);

onMounted(async () => {
  recent.value = (await api.instances.list()).slice(0, 3);
});
</script>

<template>
  <div class="page enter">
    <div class="page-header">
      <div>
        <h1 class="page-title">Welcome back{{ player.name ? `, ${player.name}` : "" }}</h1>
        <p class="page-sub">Jump back in, find mods, or start a server for your friends.</p>
      </div>
    </div>

    <h2 class="section-title">Jump back in</h2>
    <div v-if="recent && recent.length" class="grid">
      <InstanceCard v-for="inst in recent" :key="inst.id" :instance="inst" />
    </div>
    <div v-else-if="recent" class="card empty">
      <h3>No instances yet</h3>
      <p>An instance is a Minecraft version with its own mods, worlds and settings.</p>
      <button class="btn btn-primary" @click="creating = true"><Plus /> New instance</button>
    </div>

    <h2 class="section-title">Get started</h2>
    <div class="grid">
      <button class="card card-hover option" @click="creating = true">
        <span class="icon"><Plus /></span>
        <span><h3>New instance</h3><span class="hint">Pick a version and a loader. Java is installed for you.</span></span>
      </button>
      <RouterLink to="/discover" class="card card-hover option">
        <span class="icon"><Compass /></span>
        <span><h3>Discover mods</h3><span class="hint">Search Modrinth and add mods with their dependencies.</span></span>
      </RouterLink>
      <RouterLink to="/servers/host" class="card card-hover option">
        <span class="icon"><Server /></span>
        <span><h3>Host a server</h3><span class="hint">On this computer, another machine, or one you already run.</span></span>
      </RouterLink>
    </div>

    <CreateInstanceDialog v-if="creating" @close="creating = false" />
  </div>
</template>
