import { invoke } from "@tauri-apps/api/core";

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
  /** The Microsoft account playing (its profile id), or null for an offline name. */
  microsoft: string | null;
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
    signUp: (email: string, invite: string, username: string, password: string) => call<Me>("account_sign_up", { email, invite, username, password }),
    redeem: (invite: string, username: string) => call<Me>("account_redeem", { invite, username }),
    signOut: () => call<void>("account_sign_out"),
    setUsername: (username: string) => call<void>("account_set_username", { username }),
  },
  share: {
    status: (id: string) => call<Share | null>("share_status", { id }),
    create: (id: string) => call<Share>("share_create", { id }),
    update: (id: string) => call<Share>("share_update", { id }),
    stop: (id: string) => call<void>("share_stop", { id }),
    addFriend: (code: string, username: string) => call<unknown>("share_add_friend", { code, username }),
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
  friends: {
    list: () => call<{ friends: Friend[] }>("friends_list").then((r) => r.friends),
    add: (username: string) => call<{ friend: Friend }>("friend_add", { username }).then((r) => r.friend),
    accept: (userId: string) => call<void>("friend_accept", { userId }),
    remove: (userId: string) => call<void>("friend_remove", { userId }),
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
  createServer: (instanceId: string, machineId: string, name: string, memGb: number) =>
    call<{ server_id: number; linked: number; embedded: number; client_only: number }>("create_server_from_instance", {
      instanceId,
      machineId,
      name,
      memGb,
    }),
  system: {
    memoryMb: () => call<number>("total_memory_mb"),
    checkJava: (path: string) => call<string>("check_java", { path }),
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
