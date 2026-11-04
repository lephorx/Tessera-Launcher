<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { RouterLink } from "vue-router";
import { ArrowLeft, CreditCard, Laptop, Network, PlugZap, Server, Sparkles, Wrench } from "lucide-vue-next";
import AddMachineForm from "@/components/AddMachineForm.vue";
import CreateServerDialog from "@/components/CreateServerDialog.vue";
import HostLocal from "@/components/HostLocal.vue";
import LinuxInstall from "@/components/LinuxInstall.vue";
import { api, type Instance, type Machine } from "@/lib/api";

/** Where a new server runs. From an instance's "Create server" it's for that instance. */
const props = defineProps<{ instanceId?: string | null }>();

type Path = null | "diy" | "here" | "other" | "existing";
const step = ref<Path>(null);
const instance = ref<Instance | null>(null);
const machines = ref<Machine[]>([]);
const onMachine = ref(false);
const signedIn = computed(() => machines.value.filter((m) => m.signed_in));
// Horizon servers only run in the launcher for now
const horizon = computed(() => instance.value?.loader === "horizon");

onMounted(async () => {
  if (!props.instanceId) return;
  [instance.value, machines.value] = await Promise.all([api.instances.get(props.instanceId), api.machines.list()]);
});
const easyLink = computed(() => (props.instanceId ? `/servers/new?instance=${props.instanceId}` : "/servers/new"));
</script>

<template>
  <div class="page page-narrow enter">
    <RouterLink v-if="instanceId" :to="`/instance/${instanceId}`" class="row hint" style="margin-bottom: 14px; color: var(--tint); font-weight: 600">
      <ArrowLeft :size="14" /> {{ instance?.name ?? "Back" }}
    </RouterLink>
    <RouterLink v-else to="/servers" class="row hint" style="margin-bottom: 14px; color: var(--tint); font-weight: 600"><ArrowLeft :size="14" /> Servers</RouterLink>
    <div class="page-header">
      <div>
        <h1 class="page-title">{{ instance ? `Create a server from ${instance.name}` : "Host a server" }}</h1>
        <p class="page-sub">
          <template v-if="instance">Same Minecraft version, loader and server-side mods. Where should it run?</template>
          <template v-else>A Minecraft server for you and your friends, managed from here.</template>
        </p>
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
      <RouterLink :to="easyLink" class="card card-hover option" style="color: inherit">
        <span class="icon"><Sparkles /></span>
        <span>
          <h3>On this computer, easy <span class="pill">Recommended</span></h3>
          <span class="hint">A few clicks and it runs. The launcher sets up everything itself, no Docker or other programs. Great to start with.</span>
        </span>
      </RouterLink>
      <button v-if="instance && signedIn.length && !horizon" class="card card-hover option" @click="onMachine = true">
        <span class="icon"><Server /></span>
        <span>
          <h3>On one of my Tessera machines</h3>
          <span class="hint">{{ signedIn.map((m) => m.name).join(", ") }}. The server keeps this instance's server-side mods in sync.</span>
        </span>
      </button>
      <button class="card card-hover option" :disabled="horizon" @click="step = 'diy'">
        <span class="icon"><Wrench /></span>
        <span>
          <h3>Advanced: with Tessera <span class="pill">Free</span></h3>
          <span class="hint">
            <template v-if="horizon">Horizon servers only run in the launcher for now.</template>
            <template v-else>Tessera's dashboard with backups, your own domain and several users, on this computer (with Docker), another machine, or one you already have.</template>
          </span>
        </span>
      </button>
    </div>

    <!-- 2. where -->
    <div v-else-if="step === 'diy'" class="stack">
      <button class="card card-hover option" @click="step = 'here'">
        <span class="icon"><Laptop /></span>
        <span><h3>On this computer, with Docker</h3><span class="hint">One click installs Tessera and Docker here; then you set it up. Servers run while this computer is on.</span></span>
      </button>
      <button class="card card-hover option" @click="step = 'other'">
        <span class="icon"><Network /></span>
        <span><h3>On another machine</h3><span class="hint">A home server or old PC. Reach it over your network or Tailscale.</span></span>
      </button>
      <button class="card card-hover option" @click="step = 'existing'">
        <span class="icon"><PlugZap /></span>
        <span><h3>Add a server I already run</h3><span class="hint">Connect a Tessera that's already set up.</span></span>
      </button>
      <button class="btn btn-ghost" style="align-self: flex-start" @click="step = null">Back</button>
    </div>

    <!-- 3a. this computer -->
    <div v-else-if="step === 'here'" class="stack">
      <HostLocal />
      <button class="btn btn-ghost" style="align-self: flex-start" @click="step = 'diy'">Back</button>
    </div>

    <!-- 3b. another machine -->
    <div v-else-if="step === 'other'" class="card stack">
      <h2 style="font-size: 17px; font-weight: 700">Set up another machine</h2>
      <ol class="muted" style="margin: 0; padding-left: 18px; line-height: 1.7">
        <li>On that machine, install Tessera with the commands for its system. The package installs Docker too and shows a setup code.</li>
      </ol>
      <LinuxInstall />
      <ol class="muted" start="2" style="margin: 0; padding-left: 18px; line-height: 1.7">
        <li>Open its dashboard once and create your account with the setup code and an authenticator app.</li>
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
    <CreateServerDialog v-if="onMachine && instance" :instance="instance" @close="onMachine = false" />
  </div>
</template>
