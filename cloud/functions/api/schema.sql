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
