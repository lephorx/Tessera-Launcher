import { defineConfig } from "@neon/config/v1";

// Tessera on Neon (project "tessera", Frankfurt). Accounts are Neon Auth (managed Better Auth);
// shared packs live in the private "packs" bucket; the Tessera API function is the only thing
// holding DATABASE_URL and the bucket credentials. The launcher and website never get them.
export default defineConfig({
  auth: true,
  buckets: {
    packs: { access: "private" },
  },
  functions: {
    api: {
      name: "Tessera API",
      source: "functions/api/index.ts",
    },
  },
});
