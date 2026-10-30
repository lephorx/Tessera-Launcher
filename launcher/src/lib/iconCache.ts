import { ref } from "vue";

/** Bumped whenever an uploaded icon changes, so <InstanceIcon> reloads it. */
export const iconVersion = ref(0);
