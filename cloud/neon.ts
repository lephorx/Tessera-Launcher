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
      // Email through Proton Mail SMTP (see functions/api/mail.ts). The secrets come from
      // .env.mail (not in git): `neon deploy --env .env.mail`. Without them no email goes out.
      env: {
        SMTP_HOST: process.env.SMTP_HOST ?? "smtp.protonmail.ch",
        SMTP_PORT: process.env.SMTP_PORT ?? "587",
        SMTP_USER: process.env.SMTP_USER ?? "",
        SMTP_PASS: process.env.SMTP_PASS ?? "",
        MAIL_FROM: process.env.MAIL_FROM ?? "",
        MAIL_SECRET: process.env.MAIL_SECRET ?? "",
        // Shared with the website (Vercel env, same name): signs the visitor address its pages
        // send and its calls to /v1/site/limit. Also from .env.mail.
        TESSERA_SITE_SECRET: process.env.TESSERA_SITE_SECRET ?? "",
      },
    },
  },
});
