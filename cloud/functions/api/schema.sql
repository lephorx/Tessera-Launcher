-- Tessera API tables (Neon project "tessera"). Users themselves live in neon_auth."user".
-- Idempotent: apply with `pnpm --filter @tessera/cloud neon:schema`.

create extension if not exists citext;

-- The public name friends add you by.
create table if not exists profiles (
  user_id    uuid primary key references neon_auth."user"(id) on delete cascade,
  username   citext not null unique check (username ~ '^[A-Za-z0-9_]{3,24}$'),
  created_at timestamptz not null default now()
);

-- A shared instance: a .tapack in the "packs" bucket, reachable by invite code.
create table if not exists shares (
  code         text primary key,
  owner_id     uuid not null references neon_auth."user"(id) on delete cascade,
  name         text not null,
  game_version text not null,
  loader       text not null,
  size_bytes   bigint not null,
  object_key   text not null,
  created_at   timestamptz not null default now(),
  updated_at   timestamptz not null default now(),
  -- null = the invite link never expires; friends added by name keep access either way
  expires_at   timestamptz
);
create index if not exists shares_owner_idx on shares (owner_id);

-- Friends a share was given to by username.
create table if not exists share_members (
  code     text not null references shares(code) on delete cascade,
  user_id  uuid not null references neon_auth."user"(id) on delete cascade,
  added_at timestamptz not null default now(),
  primary key (code, user_id)
);
create index if not exists share_members_user_idx on share_members (user_id);

-- Shared packs are invites (2026-11-01): a friend added to a pack gets it once they accept.
-- Everyone on a pack before then had it already, so they count as accepted.
do $$ begin
  if not exists (select 1 from information_schema.columns where table_name = 'share_members' and column_name = 'accepted_at') then
    alter table share_members add column accepted_at timestamptz;
    update share_members set accepted_at = added_at;
  end if;
end $$;

-- Friend requests and friendships. One row per pair and direction asked; accepted = friends.
create table if not exists friendships (
  requester_id uuid not null references neon_auth."user"(id) on delete cascade,
  addressee_id uuid not null references neon_auth."user"(id) on delete cascade,
  status       text not null default 'pending' check (status in ('pending', 'accepted')),
  created_at   timestamptz not null default now(),
  primary key (requester_id, addressee_id),
  check (requester_id <> addressee_id)
);
create index if not exists friendships_addressee_idx on friendships (addressee_id);

-- News shown in the launcher's sidebar. Written by hand (psql / Neon console), read publicly.
create table if not exists news (
  id           serial primary key,
  title        text not null,
  body         text not null,
  url          text,
  published_at timestamptz not null default now()
);
-- 16:9 picture shown above the text (e.g. https://tesseralauncher.com/news/<name>.png)
alter table news add column if not exists image_url text;

-- What a friend may do with a share: 'viewer' installs and gets updates; 'editor' may also
-- upload new content (mods, packs, configs), never other Minecraft or loader versions.
alter table share_members add column if not exists role text not null default 'viewer';
do $$ begin
  alter table share_members add constraint share_members_role_check check (role in ('viewer', 'editor'));
exception when duplicate_object then null; end $$;
-- The loader version of the current pack, so editors can't swap it.
alter table shares add column if not exists loader_version text;

-- Closed beta: an account counts once it has a profile, and profiles are only created by
-- redeeming an invite (single use). Admins create as many invites as they like; everyone
-- else three (revoking an unused one gives the slot back).
alter table profiles add column if not exists is_admin boolean not null default false;
create table if not exists invites (
  code        text primary key,
  created_by  uuid not null references neon_auth."user"(id) on delete cascade,
  created_at  timestamptz not null default now(),
  used_by     uuid references neon_auth."user"(id) on delete set null,
  used_at     timestamptz,
  revoked_at  timestamptz
);
create index if not exists invites_created_by_idx on invites (created_by);

-- Live sync: every upload bumps the revision; launchers poll /shares/sync and pull newer
-- revisions, and send base_revision so an upload made from an old copy is refused (409).
alter table shares add column if not exists revision integer not null default 1;
alter table shares add column if not exists updated_by uuid references neon_auth."user"(id) on delete set null;

-- Social settings: who may send me friend requests, and who may add me to a shared pack.
alter table profiles add column if not exists friend_requests text not null default 'everyone';
alter table profiles add column if not exists share_invites text not null default 'everyone';
do $$ begin
  alter table profiles add constraint profiles_friend_requests_check check (friend_requests in ('everyone', 'friends_of_friends', 'no_one'));
exception when duplicate_object then null; end $$;
do $$ begin
  alter table profiles add constraint profiles_share_invites_check check (share_invites in ('everyone', 'friends', 'no_one'));
exception when duplicate_object then null; end $$;

-- Blocked users can't send me friend requests or add me to shared packs (and I can't them).
create table if not exists blocks (
  blocker_id uuid not null references neon_auth."user"(id) on delete cascade,
  blocked_id uuid not null references neon_auth."user"(id) on delete cascade,
  created_at timestamptz not null default now(),
  primary key (blocker_id, blocked_id),
  check (blocker_id <> blocked_id)
);
create index if not exists blocks_blocked_idx on blocks (blocked_id);

-- Terms and age (2026-10-31): which Terms version a member accepted and when, and their age
-- band ('16+' or '13-15', the latter with a parent's or guardian's agreement). No birthdate.
alter table profiles add column if not exists terms_version text;
alter table profiles add column if not exists terms_accepted_at timestamptz;
alter table profiles add column if not exists age_declaration text;
do $$ begin
  alter table profiles add constraint profiles_age_declaration_check check (age_declaration in ('16+', '13-15'));
exception when duplicate_object then null; end $$;

-- New members: friend requests from anyone (members aged 13-15 start with friends of friends,
-- set when they join), shared packs only from friends. Existing members keep what they have.
alter table profiles alter column friend_requests set default 'everyone';
alter table profiles alter column share_invites set default 'friends';

-- Notifications (2026-11-01): when a friend request was accepted, and up to when a member
-- has seen their notifications.
alter table friendships add column if not exists accepted_at timestamptz;
alter table profiles add column if not exists notifications_seen_at timestamptz;

-- Email (2026-10-31): notices for friend requests, accepted requests and packs shared with
-- you (on by default, off per kind; only to verified addresses), and the log that keeps them
-- to one per person and thing a day.
alter table profiles add column if not exists email_friends boolean not null default true;
alter table profiles add column if not exists email_shares boolean not null default true;
create table if not exists email_log (
  user_id uuid not null references neon_auth."user"(id) on delete cascade,
  kind    text not null,
  ref     text not null,
  sent_at timestamptz not null default now(),
  primary key (user_id, kind, ref)
);

-- Two-step sign-in by email (2026-10-31): with it on, a device only gets past "who am I"
-- after entering a code sent to the account's email; then it's trusted for 30 days.
-- Codes and device tokens are stored as SHA-256 hashes only.
alter table profiles add column if not exists two_factor_email boolean not null default false;
-- When the member last turned it on or off themselves (2026-11-01); until then it goes on by
-- itself once the email is verified (POST /v1/me/2fa/auto).
alter table profiles add column if not exists two_factor_set_at timestamptz;
create table if not exists email_codes (
  user_id    uuid not null references neon_auth."user"(id) on delete cascade,
  purpose    text not null check (purpose in ('sign_in', 'enable', 'disable')),
  code_hash  text not null,
  sent_at    timestamptz not null default now(),
  expires_at timestamptz not null,
  attempts   integer not null default 0,
  primary key (user_id, purpose)
);
create table if not exists trusted_devices (
  id           uuid primary key default gen_random_uuid(),
  user_id      uuid not null references neon_auth."user"(id) on delete cascade,
  token_hash   text not null unique,
  label        text,
  created_at   timestamptz not null default now(),
  last_used_at timestamptz not null default now(),
  expires_at   timestamptz not null
);
create index if not exists trusted_devices_user_idx on trusted_devices (user_id);

-- Deleting the account with two-step sign-in on takes an emailed code too (2026-11-01).
alter table email_codes drop constraint if exists email_codes_purpose_check;
alter table email_codes add constraint email_codes_purpose_check check (purpose in ('sign_in', 'enable', 'disable', 'delete_account'));

-- Rate limits that guard passwords and codes (2026-11-01): shared by every API instance and kept
-- across restarts, unlike the in-memory ones. The key is a SHA-256 hash (it holds addresses).
create table if not exists rate_limits (
  key      text primary key,
  hits     integer not null,
  reset_at timestamptz not null
);

-- Accounts whose email was verified at some point (2026-11-01): the public email fix
-- (/v1/email/change) only works for addresses that never were. The API adds to it when it sees
-- a verified account; this catches every account verified by now.
create table if not exists verified_users (
  user_id  uuid primary key references neon_auth."user"(id) on delete cascade,
  first_at timestamptz not null default now()
);
insert into verified_users (user_id) select id from neon_auth."user" where "emailVerified" on conflict do nothing;

-- Servers linked to a shared pack (2026-11-04): each person on it publishes the servers they
-- linked to their copy, and everyone else's launcher puts them in the game's server list.
create table if not exists share_servers (
  code     text not null references shares(code) on delete cascade,
  added_by uuid not null references neon_auth."user"(id) on delete cascade,
  name     text not null check (char_length(name) between 1 and 64),
  address  text not null check (char_length(address) between 1 and 260),
  added_at timestamptz not null default now(),
  primary key (code, added_by, address)
);
