<script setup lang="ts">
import { LEGAL, openLegal, type AgeBand } from "@/lib/legal";

/**
 * Age and Terms, asked at sign-up, when joining with an invite and when the Terms change.
 * Nothing is ticked or chosen up front. Under 13 can't make an account; 13–15 needs a parent
 * or guardian to agree. We keep the age band, never a birthdate.
 */
const age = defineModel<AgeBand | null>("age", { required: true });
const parental = defineModel<boolean>("parental", { required: true });
const agreed = defineModel<boolean>("agreed", { required: true });

const bands: { id: AgeBand; label: string }[] = [
  { id: "16+", label: "I'm 16 or older" },
  { id: "13-15", label: "I'm 13–15" },
  { id: "under13", label: "I'm under 13" },
];
</script>

<template>
  <fieldset class="consent stack" style="gap: 10px">
    <legend class="label">How old are you?</legend>
    <div class="choices">
      <label v-for="b in bands" :key="b.id" class="choice consent-age" :class="{ on: age === b.id }">
        <input v-model="age" class="sr-only" type="radio" name="tessera-age" :value="b.id" required />
        {{ b.label }}
      </label>
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
  </fieldset>
</template>

<style scoped>
.consent {
  margin: 0;
  padding: 0;
  border: none;
  min-width: 0;
}
.consent legend {
  padding: 0;
  margin-bottom: 8px;
}
.consent-age {
  height: 32px;
  padding: 0 12px;
}
.consent-age:focus-within {
  outline: 2px solid color-mix(in srgb, var(--ring) 60%, transparent);
  outline-offset: 2px;
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
