<script setup lang="ts">
import { ref, useId, watch } from "vue";
import { LEGAL, ageBandOf, openLegal, type AgeBand } from "@/lib/legal";

/**
 * Age and Terms, asked at sign-up, when joining with an invite and when the Terms change.
 * Nothing is ticked or chosen up front. Under 13 can't make an account; 13–15 needs a parent
 * or guardian to agree. The birthdate only picks the age band here; it's never sent or stored.
 */
const age = defineModel<AgeBand | null>("age", { required: true });
const parental = defineModel<boolean>("parental", { required: true });
const agreed = defineModel<boolean>("agreed", { required: true });

const id = useId();
const birthdate = ref("");
const today = new Date().toISOString().slice(0, 10);
watch(birthdate, (v) => (age.value = ageBandOf(v)));
</script>

<template>
  <div class="consent stack" style="gap: 10px">
    <div class="field">
      <label :for="`${id}-birthdate`">Date of birth</label>
      <input :id="`${id}-birthdate`" v-model="birthdate" class="input consent-date" type="date" min="1900-01-01" :max="today" required />
    </div>
    <p v-if="age === 'under13'" class="inset hint" role="alert" style="margin: 0; font-size: 13px">
      Sorry, Tessera accounts are for ages 13 and up. You can still play: offline, or with your Microsoft account,
      without a Tessera account.
    </p>
    <label v-if="age === '13-15'" class="consent-check">
      <input v-model="parental" type="checkbox" required />
      <span>A parent or guardian agreed to me making an account</span>
    </label>
    <label v-if="age !== 'under13'" class="consent-check">
      <input v-model="agreed" type="checkbox" required />
      <span>
        I agree to the
        <a class="link" :href="LEGAL.terms" @click.prevent="openLegal('terms')">Terms</a>
        and the
        <a class="link" :href="LEGAL.privacy" @click.prevent="openLegal('privacy')">Privacy Policy</a>
      </span>
    </label>
  </div>
</template>

<style scoped>
.consent {
  min-width: 0;
}
/* the native picker follows the app's light/dark look */
.consent-date {
  color-scheme: light dark;
}
.consent-check {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  font-size: 13px;
  cursor: pointer;
}
.consent-check input {
  margin: 2px 0 0;
  flex-shrink: 0;
}
</style>
