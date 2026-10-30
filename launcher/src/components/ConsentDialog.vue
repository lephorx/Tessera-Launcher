<script setup lang="ts">
import { computed, ref } from "vue";
import { Loader2, LogOut } from "lucide-vue-next";
import { api } from "@/lib/api";
import { account, signOut } from "@/lib/account";
import { consentOf, type AgeBand } from "@/lib/legal";
import { errorText, toast } from "@/lib/toast";
import ConsentFields from "./ConsentFields.vue";

/**
 * Shown once to members whose account is older than the current Terms. It can't be closed;
 * agreeing or signing out are the ways on. Playing works either way.
 */
const age = ref<AgeBand | null>(null);
const parental = ref(false);
const agreed = ref(false);
const consent = computed(() => consentOf(age.value, parental.value, agreed.value));
const busy = ref(false);
const error = ref("");

async function accept() {
  if (!consent.value) return;
  busy.value = true;
  error.value = "";
  try {
    account.me = await api.account.consent(consent.value);
  } catch (e) {
    error.value = errorText(e);
  } finally {
    busy.value = false;
  }
}

async function leave() {
  try {
    await signOut();
  } catch (e) {
    toast.error(errorText(e));
  }
}
</script>

<template>
  <Teleport to="body">
    <div class="backdrop">
      <div class="dialog" style="max-width: 480px" role="alertdialog" aria-modal="true" aria-labelledby="consent-title">
        <div class="dialog-head">
          <h2 id="consent-title">Before you go on</h2>
        </div>
        <form class="dialog-body stack" style="gap: 16px" @submit.prevent="accept">
          <p class="hint" style="margin: 0; font-size: 13px">
            Tessera has Terms and a Privacy Policy now. To keep using your Tessera account (friends, sharing packs), tell us
            your age and agree to them. Playing Minecraft works without it.
          </p>
          <ConsentFields v-model:age="age" v-model:parental="parental" v-model:agreed="agreed" />
          <p v-if="age === 'under13'" class="hint" style="margin: 0">
            Sign out here. A parent or guardian can ask us to delete the account at privacy@tesseralauncher.com.
          </p>
          <p v-if="error" class="error-text">{{ error }}</p>
          <div class="dialog-footer" style="margin-top: 0">
            <button type="button" class="btn btn-lg" style="margin-right: auto" @click="leave"><LogOut /> Sign out</button>
            <button class="btn btn-primary btn-lg" :disabled="busy || !consent"><Loader2 v-if="busy" class="spin" />Agree and continue</button>
          </div>
        </form>
      </div>
    </div>
  </Teleport>
</template>
