import { invoke } from "@tauri-apps/api/core";
import type { Consent } from "./legal";

export type Loader = "vanilla" | "fabric" | "quilt" | "forge" | "neoforge";
export type ProjectType = "modpack" | "mod" | "resourcepack" | "datapack" | "shader";

export type Icon = { kind: "generated"; background: string; symbol: string } | { kind: "file" };

export interface InstalledMod {
  kind: string;
  world: string | null;
  project_id: string;
  version_id: string;
  title: string;
  version: string;
  filename: string;
  icon_url: string | null;
}

export interface Instance {
  id: string;
  name: string;
  game_version: string;
  loader: Loader;
  loader_version: string | null;
  memory_mb: number | null;
  java_path: string | null;
  jvm_args: string | null;
  resolution: { width: number; height: number } | null;
  created_at: string;
  last_played: string | null;
  mods: InstalledMod[];
  icon: Icon | null;
  pack: { project_id: string | null; name: string; version: string | null } | null;
  groups: string[];
  /** Set while this instance is shared by you. */
  share_code?: string | null;
  /** Installed from someone else's share. */
  joined?: { code: string; role: ShareRole; owner: string | null; updated_at: string | null } | null;
  /** CSCM servers made from this instance. */
  servers?: { machine_id: string; server_id: number; name: string; address: string | null }[];
}

export interface LoaderVersion {
  version: string;
  stable: boolean;
}

export interface ContentFile {
  kind: "mod" | "resourcepack" | "shader" | "datapack";
  filename: string;
  enabled: boolean;
  title: string;
  version: string | null;
  icon_url: string | null;
  project_id: string | null;
  world: string | null;
}

export interface World {
  folder: string;
  size: number;
  last_played: string | null;
  icon: string | null;
}

export interface Me {
  id: string;
  email: string;
  name: string | null;
  username: string | null;
  /** Joined with an invite (Tessera is invite-only for now). */
  member: boolean;
  is_admin: boolean;
  /** The Terms version accepted; asked again when it isn't TERMS_VERSION. */
  terms_version: string | null;
  age_declaration: "16+" | "13-15" | null;
}

export interface Invite {
  code: string;
  link: string;
  created_at: string;
  used_at: string | null;
  used_by: string | null;
}

export interface Addresses {
  /** Custom domain once ready, else the Playit address. */
  public: string | null;
  /** The Playit address when `public` is a custom domain. */
  playit: string | null;
  /** host:port on the machine's network. */
  lan: string | null;
}

export interface LinkedServer {
  machine_id: string;
  machine_name: string;
  server_id: number;
  name: string;
  addresses: Addresses;
  runtime_status: string | null;
}

export interface ServerEntry {
  name: string;
  ip: string;
}

export interface Skin {
  id: string;
  name: string;
  model: "classic" | "slim";
  cape: string | null;
  /** PNG as a data URL */
  texture: string;
  default: boolean;
}

export interface Skins {
  saved: Skin[];
  defaults: Skin[];
  selected: string | null;
  /** What the Microsoft account wears right now. */
  account: Skin | null;
}

/** One entry in the bell. */
export interface Notice {
  kind: "friend_request" | "friend_accepted" | "share_added";
  user_id: string;
  username: string;
  /** share_added: the pack */
  code: string | null;
  name: string | null;
  at: string;
}

export interface Friend {
  user_id: string;
  username: string;
  status: "friend" | "incoming" | "outgoing";
  created_at: string;
}

export interface NewsItem {
  id: number;
  title: string;
  body: string;
  url: string | null;
  image_url: string | null;
  published_at: string;
}

export interface Share {
  code: string;
  name: string;
  game_version: string;
  loader: Loader;
  size_bytes: number;
  created_at: string;
  updated_at: string;
  expires_at: string | null;
  owner: string | null;
  link?: string;
  members?: { user_id: string; username: string; role: ShareRole }[];
}

export type ShareRole = "viewer" | "editor";

/** A shared instance you were added to (or own no longer). */
export interface MemberStatus {
  code: string;
  role: ShareRole | null;
  owner: string | null;
  name: string | null;
  updated_at: string | null;
  update_available: boolean;
  removed: boolean;
  became_owner?: boolean;
}

export interface PackEntry {
  name: string;
  is_dir: boolean;
  size: number;
  suggested: boolean;
}

export interface FoundInstance {
  name: string;
  path: string;
  game_version: string;
  loader: Loader;
  loader_version: string | null;
}

export interface LauncherDir {
  launcher: string;
  path: string;
  instances: FoundInstance[];
}

export interface Settings {
  player_name: string;
  memory_mb: number;
  design: "modern" | "classic";
  backend_url: string;
  groups: string[];
  /** Offline names added to the launcher; player_name is the active one. */
  players: string[];
  /** The Microsoft account playing (its profile id), or null without one. */
  microsoft: string | null;
  /** Files downloaded at once, 1–16. */
  max_downloads: number;
}

export interface Privacy {
  friend_requests: "everyone" | "friends_of_friends" | "no_one";
  share_invites: "everyone" | "friends" | "no_one";
}

export interface Blocked {
  user_id: string;
  username: string;
  created_at: string;
}

export type EasyKind = "vanilla" | "paper" | "fabric" | "forge" | "neoforge";

/** A server the launcher runs itself (no Docker). */
export interface EasyServer {
  id: string;
  name: string;
  kind: EasyKind;
  game_version: string;
  loader_version: string | null;
  port: number;
  memory_mb: number;
  instance: string | null;
  created_at: string;
  state: "stopped" | "starting" | "running" | "stopping";
  players: string[];
  max_players: number;
  lan_address: string | null;
  /** While it runs */
  started_at: string | null;
  /** 100 = one core */
  cpu: number;
  /** bytes */
  memory_used: number;
  has_icon: boolean;
  backup_every_hours: number | null;
  backup_keep: number;
}

export interface EasySeen {
  name: string;
  uuid: string | null;
  first_seen: string;
  last_seen: string;
  joins: number;
  /** seconds */
  playtime: number;
  online_since: string | null;
}

export interface EasyInsights {
  players: EasySeen[];
  sessions: { name: string; joined: string; left: string }[];
  peak: number;
  peak_at: string | null;
  starts: string[];
}

export interface EasyLists {
  whitelist: string[];
  ops: string[];
  banned: { name: string; reason: string | null }[];
  whitelist_on: boolean;
}

export type EasyListKind = "whitelist" | "ops" | "banned";

export interface EasyItem {
  slot: number;
  id: string;
  count: number;
}

export interface EasyPlayer {
  name: string;
  uuid: string | null;
  online: boolean;
  seen: EasySeen | null;
  health: number | null;
  food: number | null;
  xp_level: number | null;
  gamemode: string | null;
  position: [number, number, number] | null;
  dimension: string | null;
  inventory: EasyItem[];
  ender_chest: EasyItem[];
  stats: { play_time: number; deaths: number; player_kills: number; mob_kills: number; distance: number; blocks_mined: number; jumps: number } | null;
  whitelisted: boolean;
  op: boolean;
  banned: boolean;
}

export type EasyAction =
  | { type: "kick"; reason: string | null }
  | { type: "gamemode"; mode: string }
  | { type: "heal" }
  | { type: "feed" }
  | { type: "kill" }
  | { type: "give"; item: string; count: number }
  | { type: "clear_inventory" }
  | { type: "teleport_to"; target: string }
  | { type: "message"; text: string };

export interface EasyBackup {
  name: string;
  size: number;
  created_at: string;
  automatic: boolean;
}

export interface EasyFile {
  name: string;
  dir: boolean;
  size: number;
  modified: string | null;
}

export interface EasyChanges {
  name?: string;
  motd?: string;
  backup_every_hours?: number;
  backup_keep?: number;
  memory_mb?: number;
  port?: number;
  max_players?: number;
  difficulty?: string;
  gamemode?: string;
  pvp?: boolean;
  whitelist?: boolean;
}

export interface StorageUsage {
  unique: number;
  shared: number;
  unused: number;
}

export interface MicrosoftAccount {
  id: string;
  name: string;
}

export interface Cape {
  id: string;
  name: string;
  /** PNG as a data URL */
  texture: string;
  active: boolean;
}

export interface GameVersion {
  id: string;
  kind: "release" | "snapshot" | "old_beta" | "old_alpha";
  fabric: boolean;
}

export interface ProjectHit {
  project_id: string;
  project_type: ProjectType;
  slug: string;
  title: string;
  description: string;
  icon_url: string | null;
  downloads: number;
  follows: number;
  author: string;
  categories: string[];
  display_categories: string[];
  versions: string[];
  client_side: string | null;
  server_side: string | null;
  date_modified: string | null;
}

export interface SearchResult {
  hits: ProjectHit[];
  total_hits: number;
  offset: number;
  limit: number;
}

export interface Machine {
  id: string;
  name: string;
  url: string;
  username: string;
  signed_in: boolean;
}

export interface ApiResponse<T = unknown> {
  status: number;
  body: T;
}

/** Rust commands return `Err(String)`; invoke rejects with that string. */
function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(cmd, args).catch((e) => {
    throw new Error(typeof e === "string" ? e : String(e));
  });
}

export const api = {
  settings: {
    get: () => call<Settings>("get_settings"),
    save: (settings: Settings) => call<Settings>("save_settings", { settings }),
  },
  versions: {
    game: () => call<{ latest: string; versions: GameVersion[] }>("game_versions"),
  },
  instances: {
    list: () => call<Instance[]>("list_instances"),
    get: (id: string) => call<Instance>("get_instance", { id }),
    create: (o: { name: string; gameVersion: string; loader: Loader; loaderVersion: string | null; icon: Icon | null; iconFile: string | null }) =>
      call<Instance>("create_instance", o),
    setIcon: (id: string, icon: Icon | null, iconFile: string | null = null) => call<Instance>("set_instance_icon", { id, icon, iconFile }),
    icon: (id: string) => call<string | null>("instance_icon", { id }),
    imagePreview: (path: string) => call<string | null>("image_preview", { path }),
    content: (id: string) => call<ContentFile[]>("instance_content", { id }),
    removeContent: (id: string, f: ContentFile) =>
      call<Instance>("remove_content", { id, kind: f.kind, world: f.world, filename: f.filename }),
    toggleContent: (id: string, f: ContentFile) =>
      call<void>("toggle_content", { id, kind: f.kind, world: f.world, filename: f.filename }),
    worlds: (id: string) => call<string[]>("list_worlds", { id }),
    update: (
      id: string,
      settings: { name: string; memory_mb: number | null; java_path: string | null; jvm_args: string | null; resolution: { width: number; height: number } | null },
    ) => call<Instance>("update_instance", { id, settings }),
    worldsDetail: (id: string) => call<World[]>("instance_worlds", { id }),
    deleteWorld: (id: string, folder: string) => call<void>("delete_world", { id, folder }),
    remove: (id: string) => call<void>("delete_instance", { id }),
    removeMany: (ids: string[]) => call<number>("delete_instances", { ids }),
    setGroup: (ids: string[], name: string, member: boolean) => call<void>("set_group", { ids, name, member }),
    folder: (id: string) => call<string>("instance_folder", { id }),
    /** Opens the instance's folder (or a world's) in Explorer / Finder. */
    openFolder: (id: string, world?: string) => call<void>("instance_open_folder", { id, world: world ?? null }),
    running: () => call<string[]>("running_instances"),
    logs: (id: string) => call<string[]>("instance_logs", { id }),
    launch: (id: string, world: string | null = null, server: string | null = null) => call<void>("launch_instance", { id, world, server }),
    /** CSCM servers made from this instance (also refreshes the game's server list). */
    linkedServers: (id: string) => call<LinkedServer[]>("linked_servers", { id }),
    /** The game's multiplayer server list. */
    serverList: (id: string) => call<ServerEntry[]>("server_list", { id }),
    syncServers: (id: string) => call<void>("sync_instance_servers", { id }),
    stop: (id: string) => call<void>("stop_instance", { id }),
  },
  groups: {
    create: (name: string) => call<string[]>("create_group", { name }),
    remove: (name: string) => call<string[]>("delete_group", { name }),
  },
  loaders: {
    versions: (loader: Loader, gameVersion: string) => call<LoaderVersion[]>("loader_versions", { loader, gameVersion }),
  },
  projects: {
    search: (o: {
      query: string;
      projectType: ProjectType;
      gameVersion?: string;
      loaders?: string[];
      sort?: string;
      limit?: number;
      offset?: number;
    }) => call<SearchResult>("search_projects", o),
    install: (instanceId: string, projectId: string, kind: ProjectType, world: string | null = null) =>
      call<Instance>("install_project", { instanceId, projectId, kind, world }),
    installModpack: (projectId: string) => call<Instance>("install_modpack", { projectId }),
    /** .tapack or .mrpack */
    importPack: (path: string) => call<Instance>("import_pack", { path }),
    /** "Get local pack": a new instance that can join this server. */
    /** The instance, and mods it needs that couldn't be found (mod ids). */
    fromServer: (machineId: string, serverId: number) => call<{ instance: Instance; missing: string[] }>("server_local_pack", { machineId, serverId }),
    entries: (id: string) => call<PackEntry[]>("pack_entries", { id }),
    export: (id: string, path: string, options: { name: string; version: string; author: string; summary: string; include: string[]; format: "tapack" | "mrpack" }) =>
      call<{ linked: number; embedded: number; bytes: number }>("export_pack", { id, path, options }),
  },
  imports: {
    detect: () => call<LauncherDir[]>("detect_launchers"),
    scan: (path: string) => call<LauncherDir>("scan_launcher", { path }),
    run: (found: FoundInstance[]) => call<Instance[]>("import_instances", { found }),
  },
  machines: {
    list: () => call<Machine[]>("list_machines"),
    add: (name: string, url: string, username: string, password: string, otp: string) =>
      call<Machine>("add_machine", { name, url, username, password, otp }),
    signIn: (id: string, password: string, otp: string) => call<Machine>("sign_in_machine", { id, password, otp }),
    remove: (id: string) => call<void>("remove_machine", { id }),
    request: <T = unknown>(id: string, method: string, path: string, body?: unknown) =>
      call<ApiResponse<T>>("machine_request", { id, method, path, body }),
  },
  account: {
    status: () => call<Me | null>("account_status"),
    signIn: (email: string, password: string) => call<Me>("account_sign_in", { email, password }),
    redeem: (invite: string, username: string, consent: Consent) => call<Me>("account_redeem", { invite, username, consent }),
    /** Accepts the current Terms (accounts from before them, or when they change). */
    consent: (consent: Consent) => call<Me>("account_consent", { consent }),
    /** Saves everything Tessera stores about the account as JSON. */
    exportData: (path: string) => call<void>("account_export", { path }),
    /** Deletes the account, its shares and their packs, and signs out here. */
    remove: () => call<void>("account_delete"),
    signOut: () => call<void>("account_sign_out"),
    setUsername: (username: string) => call<void>("account_set_username", { username }),
  },
  share: {
    status: (id: string) => call<Share | null>("share_status", { id }),
    create: (id: string) => call<Share>("share_create", { id }),
    update: (id: string) => call<Share>("share_update", { id }),
    stop: (id: string) => call<void>("share_stop", { id }),
    addFriend: (code: string, username: string, role: ShareRole = "viewer") => call<unknown>("share_add_friend", { code, username, role }),
    removeFriend: (code: string, userId: string) => call<void>("share_remove_friend", { code, userId }),
    setRole: (code: string, userId: string, role: ShareRole) => call<void>("share_set_role", { code, userId, role }),
    transfer: (id: string, userId: string) => call<void>("share_transfer", { id, userId }),
    memberStatus: (id: string) => call<MemberStatus | { became_owner: true } | null>("share_member_status", { id }),
    pull: (id: string) => call<Instance>("share_pull", { id }),
    publish: (id: string) => call<void>("share_publish", { id }),
    leave: (id: string) => call<void>("share_leave", { id }),
    syncNow: (id: string) => call<void>("share_sync_now", { id }),
    setExpiry: (code: string, days: number | null) => call<unknown>("share_set_expiry", { code, days }),
    received: () => call<{ shares: Share[] }>("shares_received").then((r) => r.shares),
    join: (link: string) => call<Instance>("share_join", { link }),
  },
  notifications: {
    list: () => call<{ notifications: Notice[]; seen_at: string | null; unread: number }>("notifications_list"),
    seen: () => call<void>("notifications_seen"),
  },
  friends: {
    list: () => call<{ friends: Friend[] }>("friends_list").then((r) => r.friends),
    add: (username: string) => call<{ friend: Friend }>("friend_add", { username }).then((r) => r.friend),
    accept: (userId: string) => call<void>("friend_accept", { userId }),
    remove: (userId: string) => call<void>("friend_remove", { userId }),
  },
  easy: {
    list: () => call<EasyServer[]>("easy_servers"),
    get: (id: string) => call<EasyServer>("easy_server", { id }),
    create: (server: { name: string; kind: EasyKind; game_version: string; memory_mb: number; instance: string | null; eula: boolean }) =>
      call<EasyServer>("easy_create", { server }),
    start: (id: string) => call<void>("easy_start", { id }),
    stop: (id: string) => call<void>("easy_stop", { id }),
    restart: (id: string) => call<void>("easy_restart", { id }),
    kill: (id: string) => call<void>("easy_kill", { id }),
    send: (id: string, line: string) => call<void>("easy_send", { id, line }),
    log: (id: string) => call<string[]>("easy_log", { id }),
    properties: (id: string) => call<Record<string, string>>("easy_properties", { id }),
    setProperties: (id: string, values: Record<string, string>) => call<void>("easy_set_properties", { id, values }),
    update: (id: string, changes: EasyChanges) => call<EasyServer>("easy_update", { id, changes }),
    change: (id: string, change: { kind: EasyKind; game_version: string; loader_version: string | null }) => call<EasyServer>("easy_change", { id, change }),
    remove: (id: string) => call<void>("easy_delete", { id }),
    openFolder: (id: string) => call<void>("easy_open_folder", { id }),
    icon: (id: string) => call<string | null>("easy_icon", { id }),
    setIcon: (id: string, png: number[] | null) => call<void>("easy_set_icon", { id, png }),
    insights: (id: string) => call<EasyInsights>("easy_insights", { id }),
    lists: (id: string) => call<EasyLists>("easy_lists", { id }),
    listChange: (id: string, list: EasyListKind, name: string, add: boolean, reason: string | null = null) => call<void>("easy_list_change", { id, list, name, add, reason }),
    player: (id: string, name: string) => call<EasyPlayer>("easy_player", { id, name }),
    act: (id: string, name: string, action: EasyAction) => call<string>("easy_player_act", { id, name, action }),
    backups: (id: string) => call<EasyBackup[]>("easy_backups", { id }),
    backup: (id: string) => call<EasyBackup>("easy_backup_create", { id }),
    restore: (id: string, name: string) => call<void>("easy_backup_restore", { id, name }),
    deleteBackup: (id: string, name: string) => call<void>("easy_backup_delete", { id, name }),
    revealBackup: (id: string, name: string) => call<void>("easy_backup_reveal", { id, name }),
    files: (id: string, path: string) => call<EasyFile[]>("easy_files", { id, path }),
    readFile: (id: string, path: string) => call<string>("easy_file_read", { id, path }),
    writeFile: (id: string, path: string, text: string) => call<void>("easy_file_write", { id, path, text }),
    deleteFiles: (id: string, paths: string[]) => call<void>("easy_file_delete", { id, paths }),
    renameFile: (id: string, path: string, name: string) => call<void>("easy_file_rename", { id, path, name }),
    mkdir: (id: string, path: string, name: string) => call<void>("easy_file_mkdir", { id, path, name }),
    addFiles: (id: string, path: string, sources: string[]) => call<number>("easy_file_add", { id, path, sources }),
    openFile: (id: string, path: string) => call<void>("easy_file_open", { id, path }),
  },
  privacy: {
    get: () => call<Privacy>("privacy_get"),
    set: (p: Partial<Privacy>) => call<Privacy>("privacy_set", { friendRequests: p.friend_requests ?? null, shareInvites: p.share_invites ?? null }),
  },
  blocks: {
    list: () => call<Blocked[]>("blocks_list"),
    add: (username: string) => call<void>("block_add", { username }),
    remove: (userId: string) => call<void>("block_remove", { userId }),
  },
  news: () => call<NewsItem[]>("news"),
  invites: {
    list: () => call<{ invites: Invite[]; limit: number | null; remaining: number | null }>("invites_list"),
    create: () => call<{ invite: { code: string; link: string } }>("invites_create").then((r) => r.invite),
    revoke: (code: string) => call<void>("invites_revoke", { code }),
  },
  microsoft: {
    list: () => call<MicrosoftAccount[]>("ms_accounts"),
    /** Opens Microsoft's sign-in window; null when it was closed. */
    signIn: () => call<MicrosoftAccount | null>("ms_sign_in"),
    remove: (id: string) => call<Settings>("ms_remove", { id }),
    capes: () => call<Cape[]>("ms_capes"),
  },
  skins: {
    list: () => call<Skins>("skins_list"),
    add: (opts: { path?: string; from?: string; name: string; model: "classic" | "slim"; cape: string | null }) =>
      call<Omit<Skin, "texture" | "default">>("skins_add", { path: opts.path ?? null, from: opts.from ?? null, name: opts.name, model: opts.model, cape: opts.cape }),
    update: (id: string, name: string, model: "classic" | "slim", cape: string | null) => call<void>("skins_update", { id, name, model, cape }),
    remove: (id: string) => call<void>("skins_remove", { id }),
    select: (id: string | null) => call<void>("skins_select", { id }),
  },
  /** `eula`: the user accepted the Minecraft EULA for this server. */
  createServer: (instanceId: string, machineId: string, name: string, memGb: number, eula: boolean) =>
    call<{ server_id: number; linked: number; embedded: number; client_only: number }>("create_server_from_instance", {
      instanceId,
      machineId,
      name,
      memGb,
      eula,
    }),
  system: {
    memoryMb: () => call<number>("total_memory_mb"),
    checkJava: (path: string) => call<string>("check_java", { path }),
    info: () => call<{ version: string; os: string; data_dir: string }>("app_info"),
    storage: () => call<StorageUsage>("storage_usage"),
    clearUnused: () => call<number>("storage_clear_unused"),
    openDataDir: () => call<void>("open_data_dir"),
    clearCache: () => call<number>("clear_cache"),
    exportDebug: (path: string) => call<void>("export_debug", { path }),
  },
  hostLocal: {
    status: () =>
      call<{
        supported: boolean;
        docker_installed: boolean;
        docker_running: boolean;
        docker_version: string | null;
        git: boolean;
        installed: { dir: string; data_dir: string; port: number } | null;
        reachable: boolean;
        setup_required: boolean;
      }>("host_local_status"),
    install: () => call<{ dir: string; data_dir: string; port: number }>("host_local_install"),
    start: () => call<void>("host_local_start"),
    startDocker: () => call<void>("host_local_start_docker"),
    openSetup: () => call<void>("host_local_open_setup"),
  },
  docker: () => call<{ installed: boolean; running: boolean; version?: string }>("docker_status"),
};
