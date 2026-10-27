//! Tauri command surface — the complete IPC contract used by the React UI.

use networld_core::config::{AppConfig, SavedPayload, ServerProfile, Settings};
use networld_core::engines;
use networld_core::links;
use networld_core::logging::LogEntry;
use networld_core::payload::{PayloadContext, PayloadPreset, PayloadTemplate};
use networld_core::speedtest::{SpeedTestOptions, SpeedTestResult};
use networld_core::stats::DataUsageLedger;
use networld_core::{StatusSnapshot, VpnManager};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

pub struct AppState {
    pub manager: Arc<VpnManager>,
}

type R<T> = std::result::Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------
// App / status
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub core_version: String,
    pub platform: String,
    pub store_build: bool,
    pub config_path: String,
    pub bin_path: String,
    pub engines: Vec<EngineInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub kind: String,
    pub label: String,
    pub udp: bool,
    pub tun_mode: bool,
    pub requires_elevation: bool,
    pub sidecar: Option<String>,
    pub available: bool,
    pub reason: String,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    let store_build = cfg!(feature = "store");
    let engines = engines::capability_matrix()
        .into_iter()
        .map(|(kind, caps)| {
            let blocked_by_store = store_build && !caps.store_safe;
            let missing_sidecar = caps
                .sidecar
                .map(|b| state.manager.paths.binary(b).is_err())
                .unwrap_or(false);
            EngineInfo {
                kind: format!("{kind:?}").to_lowercase(),
                label: kind.label().to_string(),
                udp: caps.udp,
                tun_mode: caps.tun_mode,
                requires_elevation: caps.requires_elevation,
                sidecar: caps.sidecar.map(|s| s.to_string()),
                available: !blocked_by_store && !missing_sidecar,
                reason: if blocked_by_store {
                    "Not available in the Microsoft Store build (needs administrator rights)".into()
                } else if missing_sidecar {
                    format!(
                        "Engine binary '{}' is missing from resources/bin",
                        caps.sidecar.unwrap_or("?")
                    )
                } else {
                    String::new()
                },
            }
        })
        .collect();

    AppInfo {
        name: "Networld VPN".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        core_version: networld_core::VERSION.into(),
        platform: std::env::consts::OS.into(),
        store_build,
        config_path: state.manager.paths.config_file.display().to_string(),
        bin_path: state.manager.paths.bin_dir.display().to_string(),
        engines,
    }
}

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> R<StatusSnapshot> {
    Ok(state.manager.status().await)
}

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> R<AppConfig> {
    Ok(state.manager.config().await)
}

#[tauri::command]
pub async fn update_settings(state: State<'_, AppState>, settings: Settings) -> R<()> {
    state.manager.update_settings(settings).await.map_err(err)
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_profiles(state: State<'_, AppState>) -> R<Vec<ServerProfile>> {
    Ok(state.manager.config().await.profiles)
}

#[tauri::command]
pub async fn save_profile(state: State<'_, AppState>, profile: ServerProfile) -> R<ServerProfile> {
    let mut profile = profile;
    if profile.id.trim().is_empty() {
        profile.id = uuid_v4();
    }
    state
        .manager
        .upsert_profile(profile.clone())
        .await
        .map_err(err)?;
    Ok(profile)
}

#[tauri::command]
pub async fn delete_profile(state: State<'_, AppState>, id: String) -> R<bool> {
    state.manager.delete_profile(&id).await.map_err(err)
}

#[tauri::command]
pub async fn duplicate_profile(state: State<'_, AppState>, id: String) -> R<ServerProfile> {
    let cfg = state.manager.config().await;
    let src = cfg
        .get(&id)
        .cloned()
        .ok_or_else(|| "Profile not found".to_string())?;
    let mut copy = src.clone();
    copy.id = uuid_v4();
    copy.name = format!("{} (copy)", src.name);
    copy.created_at_ms = networld_core::logging::now_ms();
    state
        .manager
        .upsert_profile(copy.clone())
        .await
        .map_err(err)?;
    Ok(copy)
}

#[tauri::command]
pub async fn set_active_profile(state: State<'_, AppState>, id: Option<String>) -> R<()> {
    state.manager.set_active_profile(id).await.map_err(err)
}

#[tauri::command]
pub async fn test_profile(state: State<'_, AppState>, id: String) -> R<String> {
    let cfg = state.manager.config().await;
    let p = cfg
        .get(&id)
        .cloned()
        .ok_or_else(|| "Profile not found".to_string())?;
    state.manager.test_profile(&p).await.map_err(err)
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn connect(state: State<'_, AppState>, id: Option<String>) -> R<StatusSnapshot> {
    let mgr = state.manager.clone();
    mgr.connect(id).await.map_err(err)?;
    Ok(mgr.status().await)
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> R<StatusSnapshot> {
    let mgr = state.manager.clone();
    mgr.disconnect().await.map_err(err)?;
    Ok(mgr.status().await)
}

// ---------------------------------------------------------------------------
// Import / export
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub imported: usize,
    pub profiles: Vec<ServerProfile>,
    pub errors: Vec<String>,
}

#[tauri::command]
pub async fn import_links(state: State<'_, AppState>, text: String) -> R<ImportReport> {
    let (profiles, errors) = links::parse_bulk(&text);
    for p in &profiles {
        state.manager.upsert_profile(p.clone()).await.map_err(err)?;
    }
    Ok(ImportReport {
        imported: profiles.len(),
        profiles,
        errors,
    })
}

#[tauri::command]
pub async fn import_nw(
    state: State<'_, AppState>,
    blob: String,
    password: Option<String>,
) -> R<ImportReport> {
    let profiles = links::import_nw(&blob, password.as_deref()).map_err(err)?;
    for p in &profiles {
        state.manager.upsert_profile(p.clone()).await.map_err(err)?;
    }
    Ok(ImportReport {
        imported: profiles.len(),
        profiles,
        errors: vec![],
    })
}

#[tauri::command]
pub async fn export_nw(
    state: State<'_, AppState>,
    ids: Vec<String>,
    password: Option<String>,
) -> R<String> {
    let cfg = state.manager.config().await;
    let selected: Vec<ServerProfile> = if ids.is_empty() {
        cfg.profiles.clone()
    } else {
        cfg.profiles
            .iter()
            .filter(|p| ids.contains(&p.id))
            .cloned()
            .collect()
    };
    links::export_nw(&selected, password.as_deref()).map_err(err)
}

#[tauri::command]
pub async fn profile_to_link(state: State<'_, AppState>, id: String, lossless: bool) -> R<String> {
    let cfg = state.manager.config().await;
    let p = cfg
        .get(&id)
        .cloned()
        .ok_or_else(|| "Profile not found".to_string())?;
    Ok(if lossless {
        links::to_networld_link(&p)
    } else {
        links::to_link(&p)
    })
}

// ---------------------------------------------------------------------------
// Payload generator
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetInfo {
    pub id: String,
    pub label: String,
    pub template: String,
}

#[tauri::command]
pub fn payload_presets() -> Vec<PresetInfo> {
    PayloadPreset::all()
        .iter()
        .map(|p| PresetInfo {
            id: serde_json::to_value(p)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default(),
            label: p.label().to_string(),
            template: p.template().as_str().to_string(),
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRequest {
    pub template: String,
    pub host: String,
    pub port: u16,
    pub bug_host: Option<String>,
    pub method: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewResponse {
    pub preview: String,
    pub segments: Vec<PreviewSegment>,
    pub total_bytes: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewSegment {
    pub text: String,
    pub bytes: usize,
    pub delay_ms: u32,
}

#[tauri::command]
pub fn preview_payload(req: PreviewRequest) -> PreviewResponse {
    let mut ctx = PayloadContext::new(req.host, req.port).with_bug_host(req.bug_host.as_deref());
    if let Some(m) = req.method {
        if !m.is_empty() {
            ctx.method = m;
        }
    }
    let tpl = PayloadTemplate::new(req.template);
    let chunks = tpl.render(&ctx);
    PreviewResponse {
        preview: tpl.preview(&ctx),
        total_bytes: chunks.iter().map(|c| c.data.len()).sum(),
        segments: chunks
            .iter()
            .map(|c| PreviewSegment {
                text: String::from_utf8_lossy(&c.data)
                    .replace('\r', "\\r")
                    .replace('\n', "\\n"),
                bytes: c.data.len(),
                delay_ms: c.delay_ms,
            })
            .collect(),
    }
}

#[tauri::command]
pub async fn save_payload(
    state: State<'_, AppState>,
    payload: SavedPayload,
) -> R<Vec<SavedPayload>> {
    let mut cfg = state.manager.config().await;
    let mut payload = payload;
    if payload.id.trim().is_empty() {
        payload.id = uuid_v4();
    }
    match cfg.payload_library.iter_mut().find(|p| p.id == payload.id) {
        Some(slot) => *slot = payload,
        None => cfg.payload_library.push(payload),
    }
    let library = cfg.payload_library.clone();
    // Persist via the manager so the write stays atomic.
    state
        .manager
        .replace_payload_library(library.clone())
        .await
        .map_err(err)?;
    Ok(library)
}

#[tauri::command]
pub async fn delete_payload(state: State<'_, AppState>, id: String) -> R<Vec<SavedPayload>> {
    let cfg = state.manager.config().await;
    let library: Vec<SavedPayload> = cfg
        .payload_library
        .into_iter()
        .filter(|p| p.id != id)
        .collect();
    state
        .manager
        .replace_payload_library(library.clone())
        .await
        .map_err(err)?;
    Ok(library)
}

// ---------------------------------------------------------------------------
// Logs / stats / tools
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_logs(state: State<'_, AppState>) -> Vec<LogEntry> {
    state.manager.logs.snapshot()
}

#[tauri::command]
pub fn clear_logs(state: State<'_, AppState>) {
    state.manager.logs.clear();
}

#[tauri::command]
pub fn export_logs(state: State<'_, AppState>) -> String {
    state.manager.logs.export_text()
}

#[tauri::command]
pub async fn speed_test(
    state: State<'_, AppState>,
    options: Option<SpeedTestOptions>,
) -> R<SpeedTestResult> {
    state
        .manager
        .speed_test(options.unwrap_or_default())
        .await
        .map_err(err)
}

#[tauri::command]
pub async fn get_data_usage(state: State<'_, AppState>) -> R<DataUsageLedger> {
    Ok(state.manager.config().await.data_usage)
}

#[tauri::command]
pub fn open_config_dir(state: State<'_, AppState>) -> R<()> {
    let dir = state.manager.paths.config_dir.clone();
    std::fs::create_dir_all(&dir).map_err(err)?;
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(dir)
            .spawn()
            .map_err(err)?;
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
    }
    Ok(())
}

fn uuid_v4() -> String {
    // The core crate already depends on uuid; reuse its generator through a
    // tiny helper so the Tauri layer does not need the dependency itself.
    networld_core::config::ServerProfile::new(
        "",
        networld_core::config::ProfileKind::Ssh(Default::default()),
    )
    .id
}
