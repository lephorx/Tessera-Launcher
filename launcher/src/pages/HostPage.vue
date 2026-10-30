<script setup lang="ts">
import { onMounted, ref } from "vue";
import { RouterLink } from "vue-router";
import { ArrowLeft, CircleCheck, CircleX, CreditCard, Laptop, Network, PlugZap, Wrench } from "lucide-vue-next";
import AddMachineForm from "@/components/AddMachineForm.vue";
import { api } from "@/lib/api";

type Path = null | "diy" | "here" | "other" | "existing";
const step = ref<Path>(null);
const docker = ref<{ installed: boolean; running: boolean; version?: string } | null>(null);

onMounted(async () => {
  docker.value = await api.docker();
});
</script>

<template>
  <div class="page page-narrow enter">
    <RouterLink to="/servers" class="row hint" style="margin-bottom: 14px; color: var(--tint); font-weight: 600"><ArrowLeft :size="14" /> Servers</RouterLink>
    <div class="page-header">
      <div>
        <h1 class="page-title">Host a server</h1>
        <p class="page-sub">A Minecraft server for you and your friends, managed from here.</p>
      </div>
    </div>

    <!-- 1. hosted or yourself -->
    <div v-if="step === null" class="stack">
      <button class="card option" disabled>
        <span class="icon"><CreditCard /></span>
        <span>
          <h3>Rent one from us <span class="pill">Coming soon</span></h3>
          <span class="hint">Click, pay, play. Runs on our machines, sleeps when nobody is online.</span>
        </span>
      </button>
      <button class="card card-hover option" @click="step = 'diy'">
        <span class="icon"><Wrench /></span>
        <span>
          <h3>Do it yourself <span class="pill">Free</span></h3>
          <span class="hint">Run CSCM on your own hardware: this computer, another machine, or one you already have.</span>
        </span>
      </button>
    </div>

    <!-- 2. where -->
    <div v-else-if="step === 'diy'" class="stack">
      <button class="card card-hover option" @click="step = 'here'">
        <span class="icon"><Laptop /></span>
        <span><h3>On this computer</h3><span class="hint">One click: installs Docker and CSCM here. The server runs while this computer is on.</span></span>
      </button>
      <button class="card card-hover option" @click="step = 'other'">
        <span class="icon"><Network /></span>
        <span><h3>On another machine</h3><span class="hint">A home server or old PC. Reach it over your network or Tailscale.</span></span>
      </button>
      <button class="card card-hover option" @click="step = 'existing'">
        <span class="icon"><PlugZap /></span>
        <span><h3>Add a server I already run</h3><span class="hint">Connect a CSCM that's already set up.</span></span>
      </button>
      <button class="btn btn-ghost" style="align-self: flex-start" @click="step = null">Back</button>
    </div>

    <!-- 3a. this computer -->
    <div v-else-if="step === 'here'" class="card stack">
      <h2 style="font-size: 17px; font-weight: 700">Install CSCM on this computer</h2>
      <div class="inset stack" style="gap: 8px">
        <div class="row">
          <CircleCheck v-if="docker?.running" :size="16" style="color: var(--status-green)" />
          <CircleX v-else :size="16" style="color: var(--status-red)" />
          <span v-if="!docker">Checking Docker…</span>
          <span v-else-if="docker.running">Docker {{ docker.version }} is running</span>
          <span v-else-if="docker.installed">Docker is installed but not running. Start Docker Desktop.</span>
          <span v-else>Docker is not installed yet</span>
        </div>
      </div>
      <p class="muted">
        The one-click installer (Docker, CSCM, Playit for a public address) is the next thing being built. Until then,
        install CSCM with its script, then add it here with the address <span class="mono">http://localhost:3000</span>:
      </p>
      <pre class="inset mono selectable" style="margin: 0; font-size: 12px; white-space: pre-wrap">curl -fsSL https://lephor.com/cscm/install.sh | sh</pre>
      <AddMachineForm placeholder="http://localhost:3000" />
      <button class="btn btn-ghost" style="align-self: flex-start" @click="step = 'diy'">Back</button>
    </div>

    <!-- 3b. another machine -->
    <div v-else-if="step === 'other'" class="card stack">
      <h2 style="font-size: 17px; font-weight: 700">Set up another machine</h2>
      <ol class="muted" style="margin: 0; padding-left: 18px; line-height: 1.7">
        <li>On that machine, install CSCM: <span class="mono selectable">curl -fsSL https://lephor.com/cscm/install.sh | sh</span></li>
        <li>Open its dashboard once and create your account (with an authenticator app).</li>
        <li>Not on the same network? Install <b>Tailscale</b> on both computers and sign in to the same account.</li>
        <li>Enter its address below: the LAN IP or the Tailscale name, with port 3000.</li>
      </ol>
      <AddMachineForm placeholder="http://my-server.tailnet-name.ts.net:3000" />
      <button class="btn btn-ghost" style="align-self: flex-start" @click="step = 'diy'">Back</button>
    </div>

    <!-- 3c. existing -->
    <div v-else class="card stack">
      <h2 style="font-size: 17px; font-weight: 700">Add a server you already run</h2>
      <AddMachineForm />
      <button class="btn btn-ghost" style="align-self: flex-start" @click="step = 'diy'">Back</button>
    </div>
  </div>
</template>
