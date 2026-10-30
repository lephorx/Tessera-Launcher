mod core;

use std::collections::{HashMap, VecDeque};
use std::process::Stdio;
use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::oneshot;

use crate::core::import::{self as importer, Found, LauncherDir};
use crate::core::instances::{self, ContentFile, Icon, Instance, Loader, Resolution, World};
use crate::core::launch::{prepare, Player, QuickPlay};
use crate::core::paths::Paths;
use crate::core::settings::{self, Settings};
use crate::core::{account, hostlocal, http_client, livesync, loaders, modrinth, mrpack, serverlist, serverpack, servers, share, skins, tapack, versions};

const MAX_LOG_LINES: usize = 5000;

struct AppState {
    paths: Paths,
    client: reqwest::Client,
    /// Running or starting instances → a channel that stops them.
    running: Mutex<HashMap<String, Option<oneshot::Sender<()>>>>,
    logs: Mutex<HashMap<String, VecDeque<String>>>,
}

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

#[derive(Serialize, Clone)]
struct ProgressEvent<'a> {
    instance: &'a str,
    stage: &'a str,
    done: usize,
    total: usize,
}

#[derive(Serialize, Clone)]
struct StateEvent<'a> {
    instance: &'a str,
    /// "preparing" | "running" | "stopped" | "error"
    state: &'a str,
    message: Option<String>,
}

#[derive(Serialize, Clone)]
struct LogEvent<'a> {
    instance: &'a str,
    line: &'a str,
}

// ── settings ──

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    settings::load(&state.paths)
}

#[tauri::command]
fn save_settings(state: State<AppState>, mut settings: Settings) -> CmdResult<Settings> {
    settings.player_name = settings.player_name.trim().to_string();
    let valid = |name: &str| (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid(&settings.player_name) || !settings.players.iter().all(|p| valid(p)) {
        return Err("Player names are 3–16 letters, digits or underscores".into());
    }
    let mut seen = std::collections::HashSet::new();
    settings.players.retain(|p| seen.insert(p.to_lowercase()));
    if !settings.players.iter().any(|p| p == &settings.player_name) {
        settings.players.insert(0, settings.player_name.clone());
    }
    settings::save(&state.paths, &settings).map_err(err)?;
    Ok(settings)
}

// ── versions ──

#[derive(Serialize)]
struct GameVersions {
    latest: String,
    versions: Vec<GameVersion>,
}

#[derive(Serialize)]
struct GameVersion {
    id: String,
    kind: String,
    fabric: bool,
}

#[tauri::command]
async fn game_versions(state: State<'_, AppState>) -> CmdResult<GameVersions> {
    let manifest = versions::manifest(&state.client).await.map_err(err)?;
    let fabric = loaders::fabric_games(&state.client).await.unwrap_or_default();
    Ok(GameVersions {
        latest: manifest.latest.release,
        versions: manifest
            .versions
            .into_iter()
            .map(|v| GameVersion { fabric: fabric.contains(&v.id), id: v.id, kind: v.kind })
            .collect(),
    })
}

#[tauri::command]
async fn loader_versions(state: State<'_, AppState>, loader: Loader, game_version: String) -> CmdResult<Vec<loaders::LoaderVersion>> {
    loaders::versions(&state.client, loader, &game_version).await.map_err(err)
}

// ── instances ──

#[tauri::command]
fn list_instances(state: State<AppState>) -> CmdResult<Vec<Instance>> {
    instances::list(&state.paths).map_err(err)
}

#[tauri::command]
fn get_instance(state: State<AppState>, id: String) -> CmdResult<Instance> {
    instances::get(&state.paths, &id).map_err(err)
}

#[tauri::command]
async fn create_instance(
    state: State<'_, AppState>,
    name: String,
    game_version: String,
    loader: Loader,
    loader_version: Option<String>,
    icon: Option<Icon>,
    icon_file: Option<String>,
) -> CmdResult<Instance> {
    let loader_version = match (loader, loader_version) {
        (Loader::Vanilla, _) => None,
        (_, Some(v)) => Some(v),
        (_, None) => {
            let list = loaders::versions(&state.client, loader, &game_version).await.map_err(err)?;
            let pick = list.iter().find(|l| l.stable).or(list.first());
            Some(pick.ok_or("This loader doesn't support that Minecraft version")?.version.clone())
        }
    };
    let mut inst = instances::create(&state.paths, &name, &game_version, loader, loader_version, icon).map_err(err)?;
    if let Some(file) = icon_file {
        instances::set_icon_file(&state.paths, &inst.id, std::path::Path::new(&file)).map_err(err)?;
        inst.icon = Some(Icon::File);
        instances::save(&state.paths, &inst).map_err(err)?;
    }
    Ok(inst)
}

#[tauri::command]
fn set_instance_icon(state: State<AppState>, id: String, icon: Option<Icon>, icon_file: Option<String>) -> CmdResult<Instance> {
    let mut inst = instances::get(&state.paths, &id).map_err(err)?;
    if let Some(file) = icon_file {
        instances::set_icon_file(&state.paths, &id, std::path::Path::new(&file)).map_err(err)?;
        inst.icon = Some(Icon::File);
    } else {
        inst.icon = icon;
    }
    instances::save(&state.paths, &inst).map_err(err)?;
    Ok(inst)
}

#[tauri::command]
fn instance_icon(state: State<AppState>, id: String) -> Option<String> {
    instances::icon_data_url(&state.paths, &id)
}

#[tauri::command]
fn image_preview(path: String) -> Option<String> {
    instances::image_data_url(std::path::Path::new(&path))
}

#[tauri::command]
fn instance_content(state: State<AppState>, id: String) -> CmdResult<Vec<ContentFile>> {
    let inst = instances::get(&state.paths, &id).map_err(err)?;
    Ok(instances::content(&state.paths, &inst))
}

#[tauri::command]
fn remove_content(app: AppHandle, state: State<AppState>, id: String, kind: String, world: Option<String>, filename: String) -> CmdResult<Instance> {
    let inst = instances::remove_content(&state.paths, &id, &kind, world.as_deref(), &filename).map_err(err)?;
    schedule_server_sync(&app, &id);
    Ok(inst)
}

#[tauri::command]
fn toggle_content(app: AppHandle, state: State<AppState>, id: String, kind: String, world: Option<String>, filename: String) -> CmdResult<()> {
    instances::toggle_content(&state.paths, &id, &kind, world.as_deref(), &filename).map_err(err)?;
    schedule_server_sync(&app, &id);
    Ok(())
}

#[tauri::command]
fn list_worlds(state: State<AppState>, id: String) -> Vec<String> {
    instances::worlds(&state.paths, &id)
}

#[derive(serde::Deserialize)]
struct InstanceSettings {
    name: String,
    memory_mb: Option<u32>,
    java_path: Option<String>,
    jvm_args: Option<String>,
    resolution: Option<Resolution>,
}

#[tauri::command]
fn update_instance(state: State<AppState>, id: String, settings: InstanceSettings) -> CmdResult<Instance> {
    let mut inst = instances::get(&state.paths, &id).map_err(err)?;
    if !settings.name.trim().is_empty() {
        inst.name = settings.name.trim().into();
    }
    inst.memory_mb = settings.memory_mb.filter(|m| *m >= 512);
    inst.java_path = settings.java_path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    inst.jvm_args = settings.jvm_args.map(|a| a.trim().to_string()).filter(|a| !a.is_empty());
    inst.resolution = settings.resolution.filter(|r| r.width >= 320 && r.height >= 240);
    instances::save(&state.paths, &inst).map_err(err)?;
    Ok(inst)
}

/// Installed RAM in MB, for the memory slider.
#[tauri::command]
fn total_memory_mb() -> u64 {
    match sysinfo::System::new() {
        Ok(mut sys) => {
            sys.refresh_memory();
            sys.total_memory() / 1024 / 1024
        }
        Err(_) => 8192,
    }
}

/// `java -version` of a Java executable, to check a custom Java before saving it.
#[tauri::command]
async fn check_java(path: String) -> CmdResult<String> {
    let out = tokio::process::Command::new(path.trim()).arg("-version").output().await.map_err(|e| format!("Can't run it: {e}"))?;
    let text = String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);
    text.lines().next().map(|l| l.trim().to_string()).filter(|l| out.status.success() && !l.is_empty()).ok_or_else(|| "That isn't a working Java".into())
}

#[tauri::command]
fn instance_worlds(state: State<AppState>, id: String) -> CmdResult<Vec<World>> {
    instances::worlds_detail(&state.paths, &id).map_err(err)
}

#[tauri::command]
fn delete_world(state: State<AppState>, id: String, folder: String) -> CmdResult<()> {
    if state.running.lock().unwrap().contains_key(&id) {
        return Err("Stop the game first".into());
    }
    instances::delete_world(&state.paths, &id, &folder).map_err(err)
}

#[tauri::command]
fn delete_instance(state: State<AppState>, id: String) -> CmdResult<()> {
    if state.running.lock().unwrap().contains_key(&id) {
        return Err("Stop the game first".into());
    }
    instances::delete(&state.paths, &id).map_err(err)
}

fn group_name(name: &str) -> CmdResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("Group names are 1–40 characters".into());
    }
    Ok(name.to_string())
}

#[tauri::command]
fn create_group(state: State<AppState>, name: String) -> CmdResult<Vec<String>> {
    let name = group_name(&name)?;
    let mut s = settings::load(&state.paths);
    if !s.groups.iter().any(|g| g.eq_ignore_ascii_case(&name)) {
        s.groups.push(name);
        settings::save(&state.paths, &s).map_err(err)?;
    }
    Ok(s.groups)
}

/// Removes the group; its instances stay, just ungrouped.
#[tauri::command]
fn delete_group(state: State<AppState>, name: String) -> CmdResult<Vec<String>> {
    let mut s = settings::load(&state.paths);
    s.groups.retain(|g| g != &name);
    settings::save(&state.paths, &s).map_err(err)?;
    for mut inst in instances::list(&state.paths).map_err(err)? {
        if inst.groups.contains(&name) {
            inst.groups.retain(|g| g != &name);
            instances::save(&state.paths, &inst).map_err(err)?;
        }
    }
    Ok(s.groups)
}

/// Adds (or removes) several instances to a group, creating the group if needed.
#[tauri::command]
fn set_group(state: State<AppState>, ids: Vec<String>, name: String, member: bool) -> CmdResult<()> {
    let name = group_name(&name)?;
    if member {
        let mut s = settings::load(&state.paths);
        if !s.groups.contains(&name) {
            s.groups.push(name.clone());
            settings::save(&state.paths, &s).map_err(err)?;
        }
    }
    for id in ids {
        let mut inst = instances::get(&state.paths, &id).map_err(err)?;
        inst.groups.retain(|g| g != &name);
        if member {
            inst.groups.push(name.clone());
        }
        instances::save(&state.paths, &inst).map_err(err)?;
    }
    Ok(())
}

/// Deletes several instances; running ones are skipped. Returns how many were deleted.
#[tauri::command]
fn delete_instances(state: State<AppState>, ids: Vec<String>) -> CmdResult<usize> {
    let running: Vec<String> = state.running.lock().unwrap().keys().cloned().collect();
    let mut deleted = 0;
    for id in ids.iter().filter(|id| !running.contains(id)) {
        instances::delete(&state.paths, id).map_err(err)?;
        deleted += 1;
    }
    Ok(deleted)
}

#[tauri::command]
fn instance_folder(state: State<AppState>, id: String) -> CmdResult<String> {
    instances::get(&state.paths, &id).map_err(err)?;
    Ok(state.paths.game_dir(&id).to_string_lossy().into_owned())
}

#[tauri::command]
fn running_instances(state: State<AppState>) -> Vec<String> {
    state.running.lock().unwrap().keys().cloned().collect()
}

#[tauri::command]
fn instance_logs(state: State<AppState>, id: String) -> Vec<String> {
    state.logs.lock().unwrap().get(&id).map(|l| l.iter().cloned().collect()).unwrap_or_default()
}

fn push_log(app: &AppHandle, id: &str, line: &str) {
    let state = app.state::<AppState>();
    let mut logs = state.logs.lock().unwrap();
    let buf = logs.entry(id.into()).or_default();
    if buf.len() >= MAX_LOG_LINES {
        buf.pop_front();
    }
    buf.push_back(line.into());
    let _ = app.emit("instance-log", LogEvent { instance: id, line });
}

#[tauri::command]
fn launch_instance(app: AppHandle, state: State<AppState>, id: String, world: Option<String>, server: Option<String>) -> CmdResult<()> {
    let quick_play = match (world, server) {
        (Some(w), _) => Some(QuickPlay::World(w)),
        (None, Some(s)) if !s.trim().is_empty() => Some(QuickPlay::Server(s.trim().to_string())),
        _ => None,
    };
    let inst = instances::get(&state.paths, &id).map_err(err)?;
    {
        let mut running = state.running.lock().unwrap();
        if running.contains_key(&id) {
            return Err("Already running".into());
        }
        running.insert(id.clone(), None);
    }
    state.logs.lock().unwrap().insert(id.clone(), VecDeque::new());
    // a change that couldn't reach the server earlier (offline, CSCM too old) goes now
    schedule_server_sync(&app, &id);
    let _ = app.emit("instance-state", StateEvent { instance: &id, state: "preparing", message: None });

    tauri::async_runtime::spawn(async move {
        let result = run_instance(&app, inst, quick_play).await;
        let state = app.state::<AppState>();
        state.running.lock().unwrap().remove(&id);
        match result {
            Ok(code) => {
                let msg = code.filter(|c| *c != 0).map(|c| format!("The game exited with code {c}"));
                let _ = app.emit("instance-state", StateEvent { instance: &id, state: "stopped", message: msg });
            }
            Err(e) => {
                push_log(&app, &id, &format!("[Tessera] {e:#}"));
                let _ = app.emit("instance-state", StateEvent { instance: &id, state: "error", message: Some(format!("{e:#}")) });
            }
        }
    });
    Ok(())
}

async fn run_instance(app: &AppHandle, mut inst: Instance, quick_play: Option<QuickPlay>) -> anyhow::Result<Option<i32>> {
    let state = app.state::<AppState>();
    let settings = settings::load(&state.paths);
    let player = Player::offline(&settings.player_name);
    let id = inst.id.clone();
    let progress = |stage: &str, done: usize, total: usize| {
        let _ = app.emit("launch-progress", ProgressEvent { instance: &id, stage, done, total });
    };
    let plan = prepare(&state.client, &state.paths, &inst, &player, inst.memory_mb.unwrap_or(settings.memory_mb), quick_play.as_ref(), &progress).await?;

    // offline accounts: their chosen skin, through the instance's "Tessera Skin" resource pack
    skins::prepare_instance(&state.paths, &state.paths.game_dir(&id), &player.name);
    push_log(app, &id, &format!("[Tessera] Starting Minecraft {} as {}", inst.game_version, player.name));
    let mut child = tokio::process::Command::new(&plan.java)
        .args(&plan.args)
        .current_dir(&plan.game_dir)
        // `cargo run` / `tauri dev` set these; inherited by the game they crash older versions on macOS.
        .env_remove("DYLD_FALLBACK_LIBRARY_PATH")
        .env_remove("DYLD_LIBRARY_PATH")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let (kill_tx, kill_rx) = oneshot::channel();
    state.running.lock().unwrap().insert(id.clone(), Some(kill_tx));
    inst.last_played = Some(chrono::Utc::now());
    let _ = instances::save(&state.paths, &inst);
    let _ = app.emit("instance-state", StateEvent { instance: &id, state: "running", message: None });

    for pipe in [child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>), child.stderr.take().map(|s| Box::new(s) as _)]
        .into_iter()
        .flatten()
    {
        let app = app.clone();
        let id = id.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                push_log(&app, &id, &line);
            }
        });
    }

    tokio::select! {
        status = child.wait() => Ok(status?.code()),
        _ = kill_rx => {
            child.kill().await?;
            push_log(app, &id, "[Tessera] Stopped");
            Ok(None)
        }
    }
}

#[tauri::command]
fn stop_instance(state: State<AppState>, id: String) -> CmdResult<()> {
    match state.running.lock().unwrap().get_mut(&id) {
        Some(tx) => {
            if let Some(tx) = tx.take() {
                let _ = tx.send(());
            }
            Ok(())
        }
        None => Err("Not running".into()),
    }
}

// ── Modrinth content, modpacks, imports ──

#[derive(Serialize, Clone)]
struct PackProgress<'a> {
    stage: &'a str,
    done: usize,
    total: usize,
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn search_projects(
    state: State<'_, AppState>,
    query: String,
    project_type: String,
    game_version: Option<String>,
    loaders: Option<Vec<String>>,
    sort: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> CmdResult<modrinth::SearchResult> {
    let loaders = loaders.unwrap_or_default();
    modrinth::search(
        &state.client,
        modrinth::Query {
            text: &query,
            project_type: &project_type,
            game_version: game_version.as_deref(),
            loaders: &loaders,
            sort: sort.as_deref().unwrap_or("relevance"),
            limit: limit.unwrap_or(20),
            offset: offset.unwrap_or(0),
        },
    )
    .await
    .map_err(err)
}

#[tauri::command]
async fn install_project(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    project_id: String,
    kind: String,
    world: Option<String>,
) -> CmdResult<Instance> {
    let inst = modrinth::install(&state.client, &state.paths, &instance_id, &project_id, &kind, world.as_deref()).await.map_err(err)?;
    schedule_server_sync(&app, &instance_id);
    Ok(inst)
}

#[tauri::command]
async fn install_modpack(app: AppHandle, state: State<'_, AppState>, project_id: String) -> CmdResult<Instance> {
    let progress = |stage: &str, done: usize, total: usize| {
        let _ = app.emit("pack-progress", PackProgress { stage, done, total });
    };
    let result = modrinth::install_modpack(&state.client, &state.paths, &project_id, &progress).await.map_err(err);
    let _ = app.emit("pack-progress", PackProgress { stage: "", done: 0, total: 0 });
    result
}

/// Imports a .tapack or .mrpack file as a new instance.
#[tauri::command]
async fn import_pack(app: AppHandle, state: State<'_, AppState>, path: String) -> CmdResult<Instance> {
    let progress = |stage: &str, done: usize, total: usize| {
        let _ = app.emit("pack-progress", PackProgress { stage, done, total });
    };
    let file = std::path::Path::new(&path);
    let result = if path.to_lowercase().ends_with(".mrpack") {
        let origin = mrpack::Origin { project_id: None, version_id: None, icon_url: None };
        mrpack::import(&state.client, &state.paths, file, origin, &progress).await
    } else {
        tapack::import(&state.client, &state.paths, file, &progress).await
    };
    let _ = app.emit("pack-progress", PackProgress { stage: "", done: 0, total: 0 });
    result.map_err(err)
}

#[tauri::command]
fn pack_entries(state: State<AppState>, id: String) -> CmdResult<Vec<tapack::Entry>> {
    tapack::entries(&state.paths, &id).map_err(err)
}

#[tauri::command]
async fn export_pack(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    path: String,
    options: tapack::ExportOptions,
) -> CmdResult<tapack::ExportReport> {
    let progress = |stage: &str, done: usize, total: usize| {
        let _ = app.emit("pack-progress", PackProgress { stage, done, total });
    };
    let mut target = std::path::PathBuf::from(path);
    let ext = if options.format == tapack::PackFormat::Mrpack { "mrpack" } else { "tapack" };
    if target.extension().is_none_or(|e| e != ext) {
        target.set_extension(ext);
    }
    let result = tapack::export(&state.client, &state.paths, &id, &target, options, &progress).await.map_err(err);
    let _ = app.emit("pack-progress", PackProgress { stage: "", done: 0, total: 0 });
    result
}

#[tauri::command]
fn detect_launchers() -> Vec<LauncherDir> {
    importer::detect()
}

#[tauri::command]
fn scan_launcher(path: String) -> LauncherDir {
    importer::scan("Custom", std::path::Path::new(&path))
}

#[tauri::command]
async fn import_instances(state: State<'_, AppState>, found: Vec<Found>) -> CmdResult<Vec<Instance>> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || found.iter().map(|f| importer::import(&paths, f)).collect::<anyhow::Result<Vec<_>>>())
        .await
        .map_err(err)?
        .map_err(err)
}

// ── Tessera account and sharing ──

#[tauri::command]
async fn account_status(state: State<'_, AppState>) -> CmdResult<Option<account::Me>> {
    Ok(account::current(&state.client).await)
}

#[tauri::command]
async fn account_sign_in(state: State<'_, AppState>, email: String, password: String) -> CmdResult<account::Me> {
    account::sign_in(&state.client, &email, &password).await.map_err(err)
}

#[tauri::command]
async fn account_sign_up(state: State<'_, AppState>, email: String, invite: String, username: String, password: String) -> CmdResult<account::Me> {
    account::sign_up(&state.client, &email, &invite, &username, &password).await.map_err(err)
}

#[tauri::command]
async fn account_redeem(state: State<'_, AppState>, invite: String, username: String) -> CmdResult<account::Me> {
    account::redeem(&state.client, &invite, &username).await.map_err(err)?;
    account::current(&state.client).await.ok_or_else(|| "Sign in again".into())
}

#[tauri::command]
async fn invites_list(state: State<'_, AppState>) -> CmdResult<Value> {
    account::api(&state.client, reqwest::Method::GET, "/invites", None, None).await.map_err(err)
}

#[tauri::command]
async fn invites_create(state: State<'_, AppState>) -> CmdResult<Value> {
    account::api(&state.client, reqwest::Method::POST, "/invites", None, None).await.map_err(err)
}

#[tauri::command]
async fn invites_revoke(state: State<'_, AppState>, code: String) -> CmdResult<()> {
    let code = account::invite_code(&code).map_err(err)?;
    account::api(&state.client, reqwest::Method::DELETE, &format!("/invites/{code}"), None, None).await.map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn account_sign_out(state: State<'_, AppState>) -> CmdResult<()> {
    account::sign_out(&state.client).await;
    Ok(())
}

#[tauri::command]
async fn account_set_username(state: State<'_, AppState>, username: String) -> CmdResult<()> {
    account::set_username(&state.client, &username).await.map_err(err)
}

fn pack_progress(app: &AppHandle) -> impl Fn(&str, usize, usize) + Send + Sync + '_ {
    move |stage: &str, done: usize, total: usize| {
        let _ = app.emit("pack-progress", PackProgress { stage, done, total });
    }
}

fn pack_done(app: &AppHandle) {
    let _ = app.emit("pack-progress", PackProgress { stage: "", done: 0, total: 0 });
}

#[tauri::command]
async fn share_status(state: State<'_, AppState>, id: String) -> CmdResult<Value> {
    share::status(&state.client, &state.paths, &id).await.map_err(err)
}

#[tauri::command]
async fn share_create(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<Value> {
    let r = share::create(&state.client, &state.paths, &id, &pack_progress(&app)).await.map_err(err);
    pack_done(&app);
    r
}

#[tauri::command]
async fn share_update(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<Value> {
    let r = share::update(&state.client, &state.paths, &id, &pack_progress(&app)).await.map_err(err);
    pack_done(&app);
    r
}

#[tauri::command]
async fn share_stop(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    share::stop(&state.client, &state.paths, &id).await.map_err(err)
}

#[tauri::command]
async fn share_add_friend(state: State<'_, AppState>, code: String, username: String) -> CmdResult<Value> {
    account::api(&state.client, reqwest::Method::POST, &format!("/shares/{code}/members"), Some(serde_json::json!({ "username": username.trim() })), None)
        .await
        .map_err(err)
}

#[tauri::command]
async fn share_remove_friend(state: State<'_, AppState>, code: String, user_id: String) -> CmdResult<()> {
    account::api(&state.client, reqwest::Method::DELETE, &format!("/shares/{code}/members/{user_id}"), None, None).await.map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn share_set_expiry(state: State<'_, AppState>, code: String, days: Option<u32>) -> CmdResult<Value> {
    account::api(&state.client, reqwest::Method::PATCH, &format!("/shares/{code}"), Some(serde_json::json!({ "expires_in_days": days })), None)
        .await
        .map_err(err)
}

#[tauri::command]
async fn shares_received(state: State<'_, AppState>) -> CmdResult<Value> {
    account::api(&state.client, reqwest::Method::GET, "/shares/received", None, None).await.map_err(err)
}

// ── friends and news ──

#[tauri::command]
async fn friends_list(state: State<'_, AppState>) -> CmdResult<Value> {
    account::api(&state.client, reqwest::Method::GET, "/friends", None, None).await.map_err(err)
}

#[tauri::command]
async fn friend_add(state: State<'_, AppState>, username: String) -> CmdResult<Value> {
    account::api(&state.client, reqwest::Method::POST, "/friends", Some(serde_json::json!({ "username": username.trim() })), None)
        .await
        .map_err(err)
}

/// Friend ids go into the URL path: only accept what a uuid can contain.
fn user_id(id: &str) -> CmdResult<&str> {
    if !id.is_empty() && id.len() <= 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        Ok(id)
    } else {
        Err("Unknown friend".into())
    }
}

#[tauri::command]
async fn friend_accept(state: State<'_, AppState>, user_id: String) -> CmdResult<()> {
    let id = self::user_id(&user_id)?;
    account::api(&state.client, reqwest::Method::POST, &format!("/friends/{id}/accept"), None, None).await.map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn friend_remove(state: State<'_, AppState>, user_id: String) -> CmdResult<()> {
    let id = self::user_id(&user_id)?;
    account::api(&state.client, reqwest::Method::DELETE, &format!("/friends/{id}"), None, None).await.map_err(err)?;
    Ok(())
}

/// Launcher news; public, no sign-in needed.
#[tauri::command]
async fn news(state: State<'_, AppState>) -> CmdResult<Value> {
    let res = state
        .client
        .get(format!("{}/news", account::API_URL))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Can't reach Tessera: {}", e.without_url()))?;
    if !res.status().is_success() {
        return Err("Couldn't load the news".into());
    }
    let body: Value = res.json().await.map_err(err)?;
    Ok(body["news"].clone())
}

/// One live-sync round for an instance right now (the Share tab's "Sync now").
#[tauri::command]
async fn share_sync_now(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let running: std::collections::HashSet<String> = state.running.lock().unwrap().keys().cloned().collect();
    for e in livesync::tick(&state.client, &state.paths, &running, Some(&id)).await {
        let _ = app.emit("share-sync", e);
    }
    Ok(())
}

/// Background live sync of every shared instance, every few seconds. Started by the window
/// once it has loaded: spawning it during setup kept the release build's window from ever
/// loading its page (blank window).
#[tauri::command]
fn start_live_sync(app: AppHandle) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(4)).await;
            let state = app.state::<AppState>();
            let running: std::collections::HashSet<String> = state.running.lock().unwrap().keys().cloned().collect();
            for e in livesync::tick(&state.client, &state.paths, &running, None).await {
                let _ = app.emit("share-sync", e);
            }
        }
    });
}

#[tauri::command]
async fn share_member_status(state: State<'_, AppState>, id: String) -> CmdResult<Value> {
    share::member_status(&state.client, &state.paths, &id).await.map_err(err)
}

#[tauri::command]
async fn share_pull(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<Instance> {
    if state.running.lock().unwrap().contains_key(&id) {
        return Err("Stop the game first".into());
    }
    let r = share::pull(&state.client, &state.paths, &id, &pack_progress(&app)).await.map_err(err);
    pack_done(&app);
    r
}

#[tauri::command]
async fn share_publish(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let r = share::publish(&state.client, &state.paths, &id, &pack_progress(&app)).await.map_err(err);
    pack_done(&app);
    r
}

#[tauri::command]
async fn share_leave(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    share::leave(&state.client, &state.paths, &id).await.map_err(err)
}

#[tauri::command]
async fn share_transfer(state: State<'_, AppState>, id: String, user_id: String) -> CmdResult<()> {
    share::transfer(&state.client, &state.paths, &id, &user_id).await.map_err(err)
}

#[tauri::command]
async fn share_set_role(state: State<'_, AppState>, code: String, user_id: String, role: String) -> CmdResult<()> {
    let id = self::user_id(&user_id)?;
    if !code.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("Unknown share".into());
    }
    account::api(&state.client, reqwest::Method::PATCH, &format!("/shares/{code}/members/{id}"), Some(serde_json::json!({ "role": role })), None)
        .await
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn share_join(app: AppHandle, state: State<'_, AppState>, link: String) -> CmdResult<Instance> {
    let r = share::join(&state.client, &state.paths, &link, &pack_progress(&app)).await.map_err(err);
    pack_done(&app);
    r
}

// ── CSCM machines ──

#[tauri::command]
fn list_machines(state: State<AppState>) -> Vec<servers::MachineView> {
    servers::list(&state.paths)
}

#[tauri::command]
async fn add_machine(
    state: State<'_, AppState>,
    name: String,
    url: String,
    username: String,
    password: String,
    otp: String,
) -> CmdResult<servers::MachineView> {
    servers::add(&state.client, &state.paths, &name, &url, &username, &password, &otp).await.map_err(err)
}

#[tauri::command]
async fn sign_in_machine(state: State<'_, AppState>, id: String, password: String, otp: String) -> CmdResult<servers::MachineView> {
    servers::sign_in(&state.client, &state.paths, &id, &password, &otp).await.map_err(err)
}

#[tauri::command]
fn remove_machine(state: State<AppState>, id: String) -> CmdResult<()> {
    servers::remove(&state.paths, &id).map_err(err)
}

#[tauri::command]
async fn machine_request(
    state: State<'_, AppState>,
    id: String,
    method: String,
    path: String,
    body: Option<Value>,
) -> CmdResult<servers::ApiResponse> {
    servers::request(&state.client, &state.paths, &id, &method, &path, body).await.map_err(err)
}

#[derive(Serialize)]
struct CreatedServer {
    server_id: i64,
    linked: usize,
    embedded: usize,
    client_only: usize,
}

/// Builds a server pack from an instance and creates a modpack server from it on a machine.
#[tauri::command]
async fn create_server_from_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    machine_id: String,
    name: String,
    mem_gb: u32,
) -> CmdResult<CreatedServer> {
    let progress = pack_progress(&app);
    let result = async {
        let built = serverpack::build(&state.client, &state.paths, &instance_id, &progress).await?;
        let handle = app.clone();
        let upload: servers::UploadProgress = std::sync::Arc::new(move |stage: &str, done: usize, total: usize| {
            let _ = handle.emit("pack-progress", PackProgress { stage, done, total });
        });
        let hash = serverpack::fingerprint(&built.bytes).ok();
        let server_id = servers::create_from_pack(&state.client, &state.paths, &machine_id, &name, mem_gb, built.bytes, upload).await?;
        let mut inst = instances::get(&state.paths, &instance_id)?;
        inst.servers.push(instances::LinkedServer { machine_id: machine_id.clone(), server_id, name: name.clone(), pack_hash: hash, address: None, port: None });
        instances::save(&state.paths, &inst)?;
        anyhow::Ok(CreatedServer { server_id, linked: built.linked, embedded: built.embedded, client_only: built.client_only })
    }
    .await
    .map_err(err);
    pack_done(&app);
    result
}

// ── servers made from an instance: server list entry and keeping their mods in sync ──

#[derive(Serialize, Clone)]
struct ServerSyncEvent<'a> {
    instance: &'a str,
    server_id: i64,
    /// "syncing" | "synced" | "failed"
    state: &'a str,
    message: Option<String>,
}

/// Latest change per instance; a sync waits a few seconds so several changes go up as one.
static SYNC_GENERATION: std::sync::LazyLock<Mutex<HashMap<String, u64>>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn schedule_server_sync(app: &AppHandle, id: &str) {
    let state = app.state::<AppState>();
    if instances::get(&state.paths, id).map(|i| i.servers.is_empty()).unwrap_or(true) {
        return;
    }
    let generation = {
        let mut map = SYNC_GENERATION.lock().unwrap();
        let g = map.entry(id.to_string()).or_insert(0);
        *g += 1;
        *g
    };
    let (app, id) = (app.clone(), id.to_string());
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        if SYNC_GENERATION.lock().unwrap().get(&id) != Some(&generation) {
            return; // a newer change will sync
        }
        sync_servers(&app, &id, false).await;
    });
}

/// Builds the server pack and sends it to each linked server that doesn't have it yet
/// (or to all of them with `force`).
async fn sync_servers(app: &AppHandle, id: &str, force: bool) {
    let state = app.state::<AppState>();
    let Ok(inst) = instances::get(&state.paths, id) else { return };
    if inst.servers.is_empty() {
        return;
    }
    let emit = |server_id: i64, st: &str, message: Option<String>| {
        let _ = app.emit("server-sync", ServerSyncEvent { instance: id, server_id, state: st, message });
    };
    let built = match serverpack::build(&state.client, &state.paths, id, &|_, _, _| {}).await {
        Ok(b) => b,
        Err(e) => {
            for s in &inst.servers {
                emit(s.server_id, "failed", Some(format!("{e:#}")));
            }
            return;
        }
    };
    let hash = serverpack::fingerprint(&built.bytes).ok();
    for linked in &inst.servers {
        if !force && hash.is_some() && linked.pack_hash == hash {
            continue;
        }
        emit(linked.server_id, "syncing", None);
        match servers::update_pack(&state.client, &state.paths, &linked.machine_id, linked.server_id, built.bytes.clone()).await {
            Ok(restarted) => {
                // fresh copy: the instance may have changed meanwhile
                if let Ok(mut now) = instances::get(&state.paths, id) {
                    if let Some(l) = now.servers.iter_mut().find(|l| l.machine_id == linked.machine_id && l.server_id == linked.server_id) {
                        l.pack_hash = hash.clone();
                    }
                    let _ = instances::save(&state.paths, &now);
                }
                let note = if restarted { "It restarts to install them." } else { "They're installed when it starts." };
                emit(linked.server_id, "synced", Some(format!("{} got the new mods. {note}", linked.name)));
            }
            Err(e) => emit(linked.server_id, "failed", Some(format!("{e:#}"))),
        }
    }
}

#[tauri::command]
async fn sync_instance_servers(app: AppHandle, id: String) -> CmdResult<()> {
    sync_servers(&app, &id, true).await;
    Ok(())
}

#[derive(Serialize)]
struct LinkedServerView {
    machine_id: String,
    machine_name: String,
    server_id: i64,
    name: String,
    addresses: servers::Addresses,
    runtime_status: Option<String>,
}

/// The instance's CSCM servers with their current address and state. Also keeps the game's
/// server list up to date (adds new servers, follows address changes, drops deleted ones).
#[tauri::command]
async fn linked_servers(state: State<'_, AppState>, id: String) -> CmdResult<Vec<LinkedServerView>> {
    let mut inst = instances::get(&state.paths, &id).map_err(err)?;
    let game_dir = state.paths.game_dir(&id);
    // the running game rewrites servers.dat from memory; update it next time instead
    let game_open = state.running.lock().unwrap().contains_key(&id);
    let machines = servers::list(&state.paths);
    let mut views = Vec::new();
    let mut keep = Vec::new();
    let mut changed = false;
    for mut linked in inst.servers.clone() {
        let machine_name = machines.iter().find(|m| m.id == linked.machine_id).map(|m| m.name.clone()).unwrap_or_else(|| "Unknown machine".into());
        let url = servers::machine_url(&state.paths, &linked.machine_id).unwrap_or_default();
        let res = servers::request(&state.client, &state.paths, &linked.machine_id, "GET", &format!("/servers/{}", linked.server_id), None).await;
        let (addresses, runtime) = match res {
            Ok(r) if r.status == 404 && !game_open => {
                // deleted in CSCM: forget it and take it off the server list
                if let Some(ip) = &linked.address {
                    let _ = serverlist::remove(&game_dir, ip);
                }
                changed = true;
                continue;
            }
            Ok(r) if r.status < 400 => {
                // CSCM answers { success, server: {...} }
                let server = if r.body["server"].is_object() { &r.body["server"] } else { &r.body };
                if let Some(port) = server["port"].as_i64() {
                    if linked.port != Some(port) {
                        linked.port = Some(port);
                        changed = true;
                    }
                }
                (servers::addresses(server, &url), server["runtime_status"].as_str().map(String::from))
            }
            // offline or signed out: what we know, and the machine's IP with the server's port
            _ => {
                let lan = reqwest::Url::parse(&url).ok().and_then(|u| u.host_str().map(String::from)).zip(linked.port).map(|(h, p)| if p == 25565 { h } else { format!("{h}:{p}") });
                (servers::Addresses { public: linked.address.clone().filter(|a| Some(a) != lan.as_ref()), playit: None, lan }, None)
            }
        };
        if let Some(best) = addresses.best().filter(|_| !game_open) {
            if linked.address.as_deref() != Some(best) {
                serverlist::upsert(&game_dir, linked.address.as_deref(), &linked.name, best).map_err(err)?;
                linked.address = Some(best.to_string());
                changed = true;
            }
        }
        views.push(LinkedServerView {
            machine_id: linked.machine_id.clone(),
            machine_name,
            server_id: linked.server_id,
            name: linked.name.clone(),
            addresses,
            runtime_status: runtime,
        });
        keep.push(linked);
    }
    if changed {
        inst.servers = keep;
        instances::save(&state.paths, &inst).map_err(err)?;
    }
    Ok(views)
}

/// The game's multiplayer server list for this instance.
#[tauri::command]
fn server_list(state: State<AppState>, id: String) -> Vec<serverlist::Entry> {
    serverlist::list(&state.paths.game_dir(&id))
}

// ── skins ──

#[tauri::command]
async fn skins_list(state: State<'_, AppState>) -> CmdResult<skins::Skins> {
    // the nine default skins come from a Minecraft jar; a failure just leaves them out
    let _ = skins::ensure_defaults(&state.client, &state.paths).await;
    let player = settings::load(&state.paths).player_name;
    Ok(skins::list(&state.paths, &player))
}

#[tauri::command]
fn skins_add(state: State<AppState>, path: Option<String>, from: Option<String>, name: String, model: skins::Model, cape: Option<String>) -> CmdResult<skins::SavedSkin> {
    skins::add(&state.paths, path.as_deref().map(std::path::Path::new), from.as_deref(), &name, model, cape).map_err(err)
}

#[tauri::command]
fn skins_update(state: State<AppState>, id: String, name: String, model: skins::Model, cape: Option<String>) -> CmdResult<()> {
    skins::update(&state.paths, &id, &name, model, cape).map_err(err)
}

#[tauri::command]
fn skins_remove(state: State<AppState>, id: String) -> CmdResult<()> {
    skins::remove(&state.paths, &id).map_err(err)
}

/// The skin the active player uses (None = Minecraft's default for their name).
#[tauri::command]
fn skins_select(state: State<AppState>, id: Option<String>) -> CmdResult<()> {
    let player = settings::load(&state.paths).player_name;
    skins::select(&state.paths, &player, id.as_deref()).map_err(err)
}

// ── host on this computer ──

#[derive(Serialize, Clone)]
struct HostInstallEvent {
    step: Option<String>,
    line: Option<String>,
}

#[tauri::command]
async fn host_local_status(state: State<'_, AppState>) -> CmdResult<hostlocal::Status> {
    Ok(hostlocal::status(&state.client, &state.paths).await)
}

/// One-click install of Tessera here; progress comes as "host-install" events.
#[tauri::command]
async fn host_local_install(app: AppHandle, state: State<'_, AppState>) -> CmdResult<hostlocal::LocalServer> {
    let step = |s: &str| {
        let _ = app.emit("host-install", HostInstallEvent { step: Some(s.into()), line: None });
    };
    let log = |l: &str| {
        let _ = app.emit("host-install", HostInstallEvent { step: None, line: Some(l.into()) });
    };
    hostlocal::install(&state.client, &state.paths, &step, &log).await.map_err(|e| format!("{e:#}"))
}

/// Starts an installed Tessera again (after a failed start, or when it was stopped).
#[tauri::command]
async fn host_local_start(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let local = hostlocal::installed(&state.paths).ok_or("Tessera isn't installed on this computer")?;
    let log = |l: &str| {
        let _ = app.emit("host-install", HostInstallEvent { step: None, line: Some(l.into()) });
    };
    if !hostlocal::status(&state.client, &state.paths).await.docker_running {
        hostlocal::start_docker().await.map_err(err)?;
    }
    hostlocal::start(&local, &log).await.map_err(|e| format!("{e:#}"))?;
    hostlocal::wait_ready(&state.client, local.port).await.map_err(err)
}

#[tauri::command]
async fn host_local_start_docker() -> CmdResult<()> {
    hostlocal::start_docker().await.map_err(err)
}

/// Tessera's own first-run setup (account, authenticator app, Playit) in a launcher window.
/// Async on purpose: creating a window from a synchronous command deadlocks WebView2 on
/// Windows (the window stays white and stops responding).
#[tauri::command]
async fn host_local_open_setup(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let local = hostlocal::installed(&state.paths).ok_or("Tessera isn't installed on this computer")?;
    if let Some(w) = app.get_webview_window("tessera-setup") {
        let _ = w.set_focus();
        return Ok(());
    }
    let url: tauri::Url = format!("http://localhost:{}/", local.port).parse().map_err(err)?;
    let port = local.port;
    // Tessera itself stays in the window; links out (playit.gg, Cloudflare, help pages) and
    // "open in new tab" links go to the browser instead of being silently dropped.
    let is_tessera = move |u: &tauri::Url| matches!(u.host_str(), Some("localhost" | "127.0.0.1")) && u.port() == Some(port);
    tauri::WebviewWindowBuilder::new(&app, "tessera-setup", tauri::WebviewUrl::External(url))
        .on_navigation(move |u| {
            if is_tessera(u) || u.scheme() == "about" {
                return true;
            }
            let _ = tauri_plugin_opener::open_url(u.as_str(), None::<&str>);
            false
        })
        .on_new_window(|u, _| {
            let _ = tauri_plugin_opener::open_url(u.as_str(), None::<&str>);
            tauri::webview::NewWindowResponse::Deny
        })
        .title("Tessera setup")
        .inner_size(1100.0, 820.0)
        .build()
        .map_err(err)?;
    Ok(())
}

/// Whether Docker is installed, for "Host on this computer".
#[tauri::command]
async fn docker_status() -> serde_json::Value {
    let out = tokio::process::Command::new("docker").arg("info").arg("--format").arg("{{.ServerVersion}}").output().await;
    match out {
        Ok(o) if o.status.success() => serde_json::json!({ "installed": true, "running": true, "version": String::from_utf8_lossy(&o.stdout).trim() }),
        Ok(_) => serde_json::json!({ "installed": true, "running": false }),
        Err(_) => serde_json::json!({ "installed": false, "running": false }),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let root = app.path().app_data_dir()?;
            std::fs::create_dir_all(&root)?;
            account::init(&root);
            app.manage(AppState {
                paths: Paths::new(root),
                client: http_client(),
                running: Mutex::new(HashMap::new()),
                logs: Mutex::new(HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            game_versions,
            loader_versions,
            list_instances,
            get_instance,
            create_instance,
            update_instance,
            delete_instance,
            instance_folder,
            create_group,
            delete_group,
            set_group,
            delete_instances,
            running_instances,
            instance_logs,
            launch_instance,
            stop_instance,
            set_instance_icon,
            instance_icon,
            image_preview,
            check_java,
            total_memory_mb,
            instance_worlds,
            delete_world,
            instance_content,
            remove_content,
            toggle_content,
            list_worlds,
            search_projects,
            install_project,
            install_modpack,
            import_pack,
            pack_entries,
            export_pack,
            detect_launchers,
            scan_launcher,
            import_instances,
            account_status,
            account_sign_in,
            account_sign_up,
            account_redeem,
            invites_list,
            invites_create,
            invites_revoke,
            account_sign_out,
            account_set_username,
            share_status,
            share_create,
            share_update,
            share_stop,
            share_add_friend,
            share_remove_friend,
            share_set_expiry,
            shares_received,
            share_join,
            share_member_status,
            share_sync_now,
            start_live_sync,
            share_pull,
            share_publish,
            share_leave,
            share_transfer,
            share_set_role,
            friends_list,
            friend_add,
            friend_accept,
            friend_remove,
            news,
            sync_instance_servers,
            skins_list,
            skins_add,
            skins_update,
            skins_remove,
            skins_select,
            linked_servers,
            server_list,
            list_machines,
            add_machine,
            sign_in_machine,
            remove_machine,
            machine_request,
            docker_status,
            host_local_status,
            host_local_install,
            host_local_start,
            host_local_start_docker,
            host_local_open_setup,
            create_server_from_instance,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tessera Launcher");
}
