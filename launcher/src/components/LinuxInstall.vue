<script setup lang="ts">
import { computed, ref } from "vue";
import { Copy } from "lucide-vue-next";
import { toast } from "@/lib/toast";

/** The commands that install Tessera on another machine, for its Linux distribution
 *  (the packages from github.com/lephorx/Tessera bring in Docker and start Tessera). */
const RELEASES = "https://github.com/lephorx/Tessera/releases/latest/download";
const INSTALLER = "curl -fsSL https://raw.githubusercontent.com/lephorx/Tessera/main/scripts/install.sh | sh";

const DISTROS = [
  {
    id: "debian",
    label: "Debian / Ubuntu",
    note: "Also Linux Mint, Pop!_OS and other Debian-based systems.",
    commands: [`curl -fsSLO ${RELEASES}/tessera.deb`, "sudo apt update && sudo apt install ./tessera.deb"],
  },
  { id: "fedora", label: "Fedora", note: "", commands: [`sudo dnf install ${RELEASES}/tessera.rpm`] },
  {
    id: "rhel",
    label: "RHEL / Rocky / Alma",
    note: "These don't ship Docker, so the first lines add Docker's repository (on RHEL, use rhel instead of centos in the URL).",
    commands: [
      "sudo dnf install -y dnf-plugins-core",
      "sudo dnf config-manager --add-repo https://download.docker.com/linux/centos/docker-ce.repo",
      "sudo dnf install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin",
      `sudo dnf install ${RELEASES}/tessera.rpm`,
    ],
  },
  {
    id: "opensuse",
    label: "openSUSE",
    note: "",
    commands: [`curl -fsSLO ${RELEASES}/tessera.rpm`, "sudo zypper install --allow-unsigned-rpm ./tessera.rpm"],
  },
  {
    id: "arch",
    label: "Arch",
    note: "Also Manjaro and EndeavourOS.",
    commands: [`curl -fsSLO ${RELEASES}/tessera.pkg.tar.zst`, "sudo pacman -U tessera.pkg.tar.zst"],
  },
  {
    id: "other",
    label: "Other / macOS",
    note: "Install Docker (with Compose) and Git first. On Linux, choose “Something else” when the installer asks.",
    commands: [INSTALLER],
  },
];

const id = ref("debian");
const distro = computed(() => DISTROS.find((d) => d.id === id.value)!);
const text = computed(() => distro.value.commands.join("\n"));

async function copy() {
  await navigator.clipboard.writeText(text.value);
  toast.success("Commands copied");
}
</script>

<template>
  <div class="stack" style="gap: 10px">
    <div class="tabs" role="tablist" aria-label="Linux distribution" style="margin-bottom: 0">
      <button v-for="d in DISTROS" :key="d.id" role="tab" :aria-selected="d.id === id" :class="{ active: d.id === id }" @click="id = d.id">
        {{ d.label }}
      </button>
    </div>
    <div class="inset row" style="align-items: flex-start; gap: 10px">
      <pre class="mono selectable" style="flex: 1; min-width: 0; margin: 0; overflow-x: auto; font-size: 12.5px; line-height: 1.7">{{ text }}</pre>
      <button class="btn btn-sm" @click="copy"><Copy /> Copy</button>
    </div>
    <p v-if="distro.note" class="hint" style="margin: 0">{{ distro.note }}</p>
  </div>
</template>
