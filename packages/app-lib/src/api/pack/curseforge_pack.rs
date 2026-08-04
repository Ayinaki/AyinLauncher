use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use futures::stream::{self, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::launcher_user_agent;
use crate::state::{EditInstance, InstanceLink, ModLoader, State};
use crate::util::fetch::REQWEST_CLIENT;

const CURSEFORGE_MOD_URL: &str = "https://api.curseforge.com/v1/mods";
const CURSEFORGE_FILES_URL: &str = "https://api.curseforge.com/v1/mods/files";
const MODRINTH_VERSION_FILES_URL: &str = "https://api.modrinth.com/v2/version_files";

/// Dev-curated CurseForge catalog pack, as listed in the repo's
/// `curseforge-packs.json`. The launcher fetches this file live so packs can
/// be added or removed by editing the repo — no app update required.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeCatalogPack {
    pub name: String,
    pub project_id: u32,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub game_version: Option<String>,
    pub loader: Option<String>,
}

/// Raw URL of the catalog JSON in this repository. Edit the file, commit and
/// push to add/remove packs — the launcher picks it up on the next fetch.
const CURSEFORGE_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/Ayinaki/AyinLauncher/main/apps/app-frontend/src/assets/curseforge-packs.json";

/// How long a fetched catalog is reused before refetching. Matches the
/// GitHub raw CDN's own max-age, so a shorter TTL wouldn't see updates sooner.
const CATALOG_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

static CATALOG_CACHE: Mutex<Option<(Instant, Vec<CurseForgeCatalogPack>)>> = Mutex::new(None);

/// Resolves the CurseForge API key at runtime.
///
/// The key is intentionally NOT committed to source. It is read from the
/// `CURSEFORGE_API_KEY` environment variable, falling back to a local,
/// gitignored `.env` file (via dotenvy). Because the Tauri dev process runs
/// with its working directory set to `apps/app` (not the repository root),
/// several `.env` locations are probed: the current directory, its parents
/// (which reaches the repo root), and the folder containing the executable.
/// If no key is configured, a clear error is returned so callers can surface
/// a TODO instead of failing the build at compile time.
pub fn curseforge_api_key() -> crate::Result<String> {
    if let Some(key) = resolve_curseforge_api_key() {
        return Ok(key);
    }

    Err(crate::Error::from(crate::ErrorKind::InputError(
        "CURSEFORGE_API_KEY is not set. TODO: configure the CurseForge API key via the \
         CURSEFORGE_API_KEY environment variable, or a gitignored .env file at the \
         repository root (or in the app's working directory)."
            .to_string(),
    )))
}

fn resolve_curseforge_api_key() -> Option<String> {
    // 1. A real process environment variable (also covers an already-loaded .env).
    if let Some(key) = non_empty_env_var("CURSEFORGE_API_KEY") {
        return Some(key);
    }

    // 2. A local, gitignored .env file in likely locations.
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();

    // Current working directory and its ancestors (apps/app -> apps -> repo root).
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd;
        for _ in 0..4 {
            candidates.push(dir.join(".env"));
            if !dir.pop() {
                break;
            }
        }
    }

    // Next to the executable and its parent (useful for packaged builds).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(".env"));
            if let Some(parent) = dir.parent() {
                candidates.push(parent.join(".env"));
            }
        }
    }

    for path in candidates {
        if !path.is_file() {
            continue;
        }
        // Load the file without overriding already-set variables.
        if dotenvy::from_path(&path).is_ok()
            && let Some(key) = non_empty_env_var("CURSEFORGE_API_KEY")
        {
            return Some(key);
        }
    }

    // 3. Compile-time value embedded by build.rs (if present).
    option_env!("CURSEFORGE_API_KEY")
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

fn non_empty_env_var(key: &str) -> Option<String> {
    let value = std::env::var(key).ok()?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Fetches the live CurseForge catalog (cached for `CATALOG_CACHE_TTL`).
/// Returns `None` when the remote catalog is unreachable or malformed, so the
/// caller can fall back to its bundled copy. Best-effort: a catalog failure
/// must never break the Home page.
pub async fn get_curseforge_catalog() -> Option<Vec<CurseForgeCatalogPack>> {
    // Serve from cache while it's fresh.
    if let Ok(guard) = CATALOG_CACHE.lock()
        && let Some((fetched_at, packs)) = guard.as_ref()
        && fetched_at.elapsed() < CATALOG_CACHE_TTL
    {
        return Some(packs.clone());
    }

    let res = REQWEST_CLIENT
        .get(CURSEFORGE_CATALOG_URL)
        // GitHub's raw CDN is fast; keep the cap short so a hung catalog
        // fetch never stalls the Home page.
        .timeout(Duration::from_secs(4))
        .header(USER_AGENT, launcher_user_agent())
        .send()
        .await;

    let packs = match res {
        Ok(res) if res.status().is_success() => {
            match res.json::<Vec<CurseForgeCatalogPack>>().await {
                Ok(packs) => packs,
                Err(e) => {
                    tracing::warn!("Failed to parse CurseForge catalog: {e}");
                    return None;
                }
            }
        }
        Ok(res) => {
            tracing::warn!("CurseForge catalog returned status {}", res.status());
            return None;
        }
        Err(e) => {
            tracing::warn!("Failed to fetch CurseForge catalog: {e}");
            return None;
        }
    };

    if let Ok(mut guard) = CATALOG_CACHE.lock() {
        *guard = Some((Instant::now(), packs.clone()));
    }
    Some(packs)
}

/// Builds the shared request headers for CurseForge API calls.
fn cf_headers() -> crate::Result<HeaderMap> {
    let mut req_headers = HeaderMap::new();
    req_headers.insert(
        "x-api-key",
        HeaderValue::from_str(&curseforge_api_key()?).map_err(|e| {
            crate::Error::from(crate::ErrorKind::InputError(format!(
                "Invalid CurseForge API key: {e}"
            )))
        })?,
    );
    req_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    req_headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&launcher_user_agent())
            .unwrap_or_else(|_| HeaderValue::from_static("ayinlauncher")),
    );
    Ok(req_headers)
}

/// Progress payload pushed to the frontend while a catalog pack installs.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CfInstallProgress {
    pub project_id: u32,
    pub phase: String,
    pub current: u64,
    pub total: u64,
    /// Total bytes downloaded so far (across all files) so the frontend can
    /// display a throughput metric and total amount.
    pub bytes_downloaded: u64,
    /// Expected total bytes across all downloadable files (0 when unknown).
    pub total_bytes: u64,
    pub message: Option<String>,
}

/// Emits a `cf_install_progress` event to the Tauri frontend.
///
/// Best-effort: a progress-notification failure must never abort a pack
/// install, so failures are logged and swallowed.
pub async fn emit_cf_install_progress(payload: CfInstallProgress) {
    #[cfg(feature = "tauri")]
    {
        use tauri::Emitter;
        match crate::EventState::get() {
            Ok(event_state) => {
                if let Err(e) = event_state.app.emit("cf_install_progress", &payload) {
                    tracing::warn!("Failed to emit cf_install_progress event: {e}");
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to emit cf_install_progress event (event state unavailable): {e}"
                );
            }
        }
    }
    #[cfg(not(feature = "tauri"))]
    {
        let _ = payload;
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BlockedMod {
    pub name: String,
    pub website_url: String,
    pub hash: String,
    pub project_id: u32,
    pub file_id: u32,
    /// The expected final filename on disk (as listed in the pack manifest).
    /// Used to match manually-downloaded files in the user's Downloads folder.
    pub file_name: String,
    /// The content class this file belongs to (the file's own class ID, or the
    /// parent mod's class ID as a fallback): 12 = resource packs,
    /// 6552 = shader packs, 6 = mods. Used to route manually-moved files into
    /// the correct instance folder.
    pub class_id: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeImportResult {
    pub instance_id: String,
    pub blocked_mods: Vec<BlockedMod>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CfModResponse {
    pub data: CfMod,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CfMod {
    pub id: u32,
    pub name: String,
    pub summary: Option<String>,
    /// The mod's URL slug (e.g. "jei"). CurseForge's website requires this in
    /// the path — numeric-project-id URLs 404.
    pub slug: Option<String>,
    /// The mod's numeric class ID (6 = mc-mods, 12 = texture-packs, ...). Only
    /// used as a fallback for building website links when `links.websiteUrl` is
    /// unavailable.
    pub class_id: Option<u32>,
    /// API-provided website links. `websiteUrl` already contains the correct
    /// category path segment (e.g. https://www.curseforge.com/minecraft/mc-mods/jei).
    pub links: Option<CfModLinks>,
    #[serde(default)]
    pub latest_files: Vec<CfFile>,
    /// The modpack's artwork. `thumbnailUrl` is the small icon used as the
    /// instance icon; `url` is the full-size logo.
    pub logo: Option<CfModLogo>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CfModLinks {
    pub website_url: Option<String>,
}

#[derive(Serialize, Debug)]
struct CfModsRequest {
    #[serde(rename = "modIds")]
    mod_ids: Vec<u32>,
}

#[derive(Deserialize, Debug)]
struct CfModsResponse {
    data: Vec<CfMod>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CfModLogo {
    pub thumbnail_url: Option<String>,
    pub url: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CfFilesResponse {
    pub data: Vec<CfFile>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CfFile {
    pub id: u32,
    pub mod_id: u32,
    pub file_name: Option<String>,
    pub download_url: Option<String>,
    pub game_versions: Option<Vec<String>>,
    pub hashes: Option<Vec<CfHash>>,
    pub file_length: Option<u64>,
    pub dependencies: Option<Vec<CfFileDependency>>,
    /// The file's content class ID. NOTE: the CurseForge files API does not
    /// actually return a per-file `classId` today, so this is almost always
    /// `None` — destination routing falls back to the parent mod's class ID.
    pub class_id: Option<u32>,
    /// `false` means the author disallows third-party distribution — the file
    /// must be downloaded manually from CurseForge.
    pub allow_mod_distribution: Option<bool>,
    /// `false` means the file is no longer available on CurseForge.
    pub is_available: Option<bool>,
    /// ISO-8601 timestamp of when the file was published (used to pick the
    /// newest file within a release-type tier).
    pub file_date: Option<String>,
    /// CurseForge release type: 1 = Release, 2 = Beta, 3 = Alpha.
    pub release_type: Option<u32>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CfFileDependency {
    pub mod_id: u32,
    pub relation_type: u32,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CfHash {
    pub value: String,
    pub algo: serde_json::Value,
}

#[derive(Serialize, Debug)]
struct CfFilesRequest {
    #[serde(rename = "fileIds")]
    file_ids: Vec<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeManifest {
    pub minecraft: CurseForgeMinecraft,
    pub manifest_type: Option<String>,
    pub manifest_version: Option<u32>,
    pub name: String,
    pub version: String,
    pub author: Option<String>,
    pub files: Vec<CurseForgeManifestFile>,
    #[serde(default = "default_overrides")]
    pub overrides: String,
}

fn default_overrides() -> String {
    "overrides".to_string()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeMinecraft {
    pub version: String,
    pub mod_loaders: Vec<CurseForgeModLoader>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeModLoader {
    pub id: String,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CurseForgeManifestFile {
    #[serde(rename = "projectID")]
    pub project_id: u32,
    #[serde(rename = "fileID")]
    pub file_id: u32,
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Debug)]
struct ModrinthVersionFilesRequest {
    hashes: Vec<String>,
    algorithm: String,
}

#[derive(Deserialize, Debug, Clone)]
struct ModrinthVersionFileResult {
    files: Vec<ModrinthFile>,
}

#[derive(Deserialize, Debug, Clone)]
struct ModrinthFile {
    url: String,
    #[allow(dead_code)]
    size: Option<u64>,
    primary: Option<bool>,
}

/// Iterate `hashes` array in exact order and select first entry with a supported algorithm ("sha1", "md5").
pub fn select_supported_hash(hashes: &[CfHash]) -> Option<(&'static str, String)> {
    for h in hashes {
        if let Some(algo) = match_supported_algo(&h.algo) {
            return Some((algo, h.value.clone()));
        }
    }
    None
}

fn match_supported_algo(v: &serde_json::Value) -> Option<&'static str> {
    match v {
        serde_json::Value::Number(n) => match n.as_u64() {
            Some(1) => Some("sha1"),
            Some(2) => Some("md5"),
            _ => None,
        },
        serde_json::Value::String(s) => {
            let lower = s.to_lowercase();
            if lower == "sha1" || lower == "1" {
                Some("sha1")
            } else if lower == "md5" || lower == "2" {
                Some("md5")
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Ranks CurseForge release types so they can be sorted: Release (1) is
/// preferred over Beta (2), which is preferred over Alpha (3).
fn release_type_rank(file: &CfFile) -> u32 {
    match file.release_type {
        Some(1) => 0,
        Some(2) => 1,
        Some(3) => 2,
        _ => 3,
    }
}

/// Best-effort parse of a CurseForge `fileDate` (RFC 3339) into an epoch.
/// Unknown/malformed dates sort last.
fn file_date_epoch(file: &CfFile) -> i64 {
    file.file_date
        .as_deref()
        .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
        .map(|date| date.timestamp())
        .unwrap_or(i64::MIN)
}

/// Selects the file that should be treated as "latest" for a modpack.
///
/// The CurseForge API does NOT guarantee that its file lists are returned
/// newest-first, so the list is explicitly sorted: Release (releaseType 1)
/// files are preferred over Beta (2) and Alpha (3), with the newest `fileDate`
/// winning within each tier and file ID as a final tiebreaker. When
/// `game_version` is given, only files supporting that version are considered;
/// if none do, the full list is used as a fallback so callers never hard-fail
/// on a manifest version that matches nothing.
pub fn select_latest_file<'a>(
    files: &'a [CfFile],
    game_version: Option<&str>,
) -> Option<&'a CfFile> {
    if files.is_empty() {
        return None;
    }

    let matches_game_version = |file: &'a CfFile| -> bool {
        match game_version {
            Some(version) => file.game_versions.as_ref().is_some_and(|versions| {
                versions.iter().any(|v| v.eq_ignore_ascii_case(version))
            }),
            None => true,
        }
    };

    let mut candidates: Vec<&'a CfFile> =
        files.iter().filter(|f| matches_game_version(f)).collect();
    if candidates.is_empty() {
        candidates = files.iter().collect();
    }

    candidates.sort_by(|a, b| compare_files_latest(a, b));

    candidates.first().copied()
}

/// Comparator used everywhere a CurseForge file list is sorted for selection or
/// display: Release (1) > Beta (2) > Alpha (3), newest `fileDate` within each
/// tier, then newest file ID as a final tiebreaker.
fn compare_files_latest(a: &CfFile, b: &CfFile) -> std::cmp::Ordering {
    release_type_rank(a)
        .cmp(&release_type_rank(b))
        .then_with(|| file_date_epoch(b).cmp(&file_date_epoch(a)))
        .then_with(|| b.id.cmp(&a.id))
}

/// Fetches a modpack's files from the dedicated files endpoint so "latest"
/// selection is not limited to the mod endpoint's bounded `latestFiles`
/// window. The response is NOT assumed to be newest-first — callers pass it
/// through `select_latest_file`, which sorts explicitly. Falls back to
/// `fallback` on any failure.
async fn fetch_pack_files(
    project_id: u32,
    req_headers: &HeaderMap,
    fallback: Vec<CfFile>,
) -> Vec<CfFile> {
    let url = format!("{CURSEFORGE_MOD_URL}/{project_id}/files?pageSize=50");
    match REQWEST_CLIENT
        .get(&url)
        .headers(req_headers.clone())
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            match res.json::<CfFilesResponse>().await {
                Ok(resp) if !resp.data.is_empty() => resp.data,
                _ => fallback,
            }
        }
        _ => fallback,
    }
}

/// Downloads the CurseForge pack logo and caches it as the instance icon,
/// following the same icon-handling convention as Modrinth imports
/// (`write_cached_icon` into the caches dir, then `edit_icon`). Best-effort:
/// icon failures are silently ignored and never abort an install.
async fn set_cf_instance_icon(instance_id: &str, icon_url: Option<&str>) {
    let Some(icon_url) = icon_url else { return };
    let Ok(state) = State::get().await else {
        return;
    };
    let Ok(icon_bytes) = crate::util::fetch::fetch(
        icon_url,
        None,
        None,
        None,
        &state.fetch_semaphore,
        &state.pool,
    )
    .await
    else {
        return;
    };
    let Some(filename) = icon_url.rsplit('/').next() else {
        return;
    };
    let Ok(icon_path) = crate::util::fetch::write_cached_icon(
        filename,
        &state.directories.caches_dir(),
        icon_bytes,
        &state.io_semaphore,
    )
    .await
    else {
        return;
    };
    let _ = crate::api::instance::edit_icon(instance_id, Some(icon_path.as_path())).await;
}

/// Checks whether a download URL is blocked or missing/invalid.
pub fn is_blocked_url(url: Option<&str>) -> bool {
    let Some(u) = url else { return true };
    if u.trim().is_empty() {
        return true;
    }
    match Url::parse(u) {
        Ok(parsed) => parsed.scheme() != "http" && parsed.scheme() != "https",
        Err(_) => true,
    }
}

/// Returns true iff Modrinth fallback should be attempted for a blocked file (hash_type == "sha1" and non-empty hash).
pub fn should_attempt_modrinth_fallback(hash_type: Option<&str>, hash: Option<&str>) -> bool {
    matches!((hash_type, hash), (Some("sha1"), Some(h)) if !h.is_empty())
}

/// Constructs a BlockedMod entry for unresolvable files.
pub fn construct_blocked_mod(
    project_id: u32,
    file_id: u32,
    file_name: Option<&str>,
    hash: Option<&str>,
    class_id: Option<u32>,
) -> BlockedMod {
    let name = file_name
        .map(String::from)
        .unwrap_or_else(|| format!("mod-{project_id}-{file_id}.jar"));
    // Numeric fallback only — the real slug/websiteUrl is resolved later from
    // the parent mod's metadata (numeric project URLs 404 on the website).
    // The `/download/` segment triggers an immediate file download instead of
    // landing on the file's info page.
    let website_url = format!("https://www.curseforge.com/projects/{project_id}/download/{file_id}");
    let hash_str = hash.unwrap_or("").to_string();

    BlockedMod {
        name: name.clone(),
        file_name: name,
        website_url,
        hash: hash_str,
        project_id,
        file_id,
        class_id,
    }
}

/// Builds the canonical CurseForge file-page URL for a blocked mod.
///
/// CurseForge's website requires the project's slug in the path — the numeric
/// `.../projects/<id>` form 404s. The category segment (class slug) is parsed
/// from the mod's API-provided `links.websiteUrl` (authoritative, since it
/// already contains the correct category path), falling back to a known
/// class-ID mapping, and the mod slug comes from the API's `slug` field.
///
/// Example: `https://www.curseforge.com/minecraft/mc-mods/jei/download/4700651`
pub fn build_blocked_mod_website_url(
    website_url: Option<&str>,
    slug: Option<&str>,
    class_id: Option<u32>,
    project_id: u32,
    file_id: u32,
) -> String {
    if let (Some(class_slug), Some(mod_slug)) = (
        parse_class_slug(website_url)
            .or_else(|| class_id.and_then(class_slug_from_id).map(String::from)),
        slug.filter(|s| !s.is_empty()),
    ) {
        return format!(
            "https://www.curseforge.com/minecraft/{class_slug}/{mod_slug}/download/{file_id}"
        );
    }
    // Last resort: numeric project URL (only when metadata couldn't be fetched
    // at all, e.g. a network failure during enrichment).
    format!("https://www.curseforge.com/projects/{project_id}/download/{file_id}")
}

/// Parses the category segment (class slug) from a CurseForge mod's
/// `links.websiteUrl`, e.g. "mc-mods" from
/// `https://www.curseforge.com/minecraft/mc-mods/jei`. Tolerates a trailing
/// `/files/<numeric-id>` pair that the API sometimes points at the latest file
/// page.
pub fn parse_class_slug(website_url: Option<&str>) -> Option<String> {
    let mut segments: Vec<String> = Url::parse(website_url?)
        .ok()?
        .path()
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    while segments.len() >= 2
        && segments[segments.len() - 2].eq_ignore_ascii_case("files")
        && segments
            .last()
            .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
    {
        segments.truncate(segments.len() - 2);
    }
    (segments.len() >= 2).then(|| segments[segments.len() - 2].clone())
}

/// Maps a CurseForge class ID to its website category slug. Only used as a
/// fallback when `links.websiteUrl` is unavailable.
fn class_slug_from_id(class_id: u32) -> Option<&'static str> {
    match class_id {
        6 => Some("mc-mods"),
        12 => Some("texture-packs"),
        6552 => Some("shaders"),
        4471 => Some("modpacks"),
        17 => Some("worlds"),
        _ => None,
    }
}

/// Which instance subfolder a CurseForge file belongs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfContentDir {
    Mods,
    ResourcePacks,
    ShaderPacks,
}

/// Resolves the destination folder for a CurseForge file. This is the single
/// source of truth used by BOTH the automated installer and the manual
/// "Scan Downloads & Move" flow — never duplicate this classification logic.
///
/// Resolution order:
/// 1. The file's own class ID (12 = resource packs, 6552 = shader packs) when
///    the files API provides one (it currently does not).
/// 2. The parent mod's class ID, fetched from the mods endpoint (the Mod
///    object reliably carries `classId`; the File object does not).
/// 3. Filename heuristics as a last-resort cross-check when no class ID is
///    available at all.
/// 4. Mods as the safe default.
pub fn resolve_content_dir(
    file_class_id: Option<u32>,
    mod_class_id: Option<u32>,
    file_name: Option<&str>,
) -> CfContentDir {
    if let Some(id) = file_class_id.or(mod_class_id) {
        return match id {
            12 => CfContentDir::ResourcePacks,
            6552 => CfContentDir::ShaderPacks,
            _ => CfContentDir::Mods,
        };
    }
    // Last-resort filename cross-check for files whose class IDs are
    // completely unknown (older packs or failed metadata lookups).
    if let Some(name) = file_name {
        let lower = name.to_ascii_lowercase();
        if lower.contains("shader") {
            return CfContentDir::ShaderPacks;
        }
        if lower.contains("resource") || lower.contains("texture") {
            return CfContentDir::ResourcePacks;
        }
    }
    CfContentDir::Mods
}

/// Fetches metadata (slug, websiteUrl, classId) for the given CurseForge
/// project IDs via the bulk mods endpoint (up to 50 IDs per request).
/// Best-effort: failures are logged and the partial result is returned so a
/// metadata lookup never hard-fails a pack install.
async fn fetch_mod_metadata(project_ids: &[u32], req_headers: &HeaderMap) -> HashMap<u32, CfMod> {
    let mut mod_meta: HashMap<u32, CfMod> = HashMap::new();
    for chunk in project_ids.chunks(50) {
        let res = REQWEST_CLIENT
            .post(CURSEFORGE_MOD_URL)
            .headers(req_headers.clone())
            .json(&CfModsRequest {
                mod_ids: chunk.to_vec(),
            })
            .send()
            .await;
        match res {
            Ok(resp) => {
                if resp.status().is_success() {
                    if let Ok(parsed) = resp.json::<CfModsResponse>().await {
                        for m in parsed.data {
                            mod_meta.insert(m.id, m);
                        }
                    }
                } else {
                    tracing::warn!(
                        "CurseForge API returned status {} fetching metadata for {} project(s)",
                        resp.status(),
                        chunk.len()
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to fetch metadata for {} project(s): {e}",
                    chunk.len()
                );
            }
        }
    }
    mod_meta
}

/// Enriches blocked mods with the real file-page URL (and a class ID when the
/// file didn't carry one) from the already-fetched mod metadata.
fn enrich_blocked_mods(blocked_mods: &mut [BlockedMod], mod_meta: &HashMap<u32, CfMod>) {
    for bm in blocked_mods.iter_mut() {
        if let Some(meta) = mod_meta.get(&bm.project_id) {
            bm.website_url = build_blocked_mod_website_url(
                meta.links.as_ref().and_then(|l| l.website_url.as_deref()),
                meta.slug.as_deref(),
                meta.class_id,
                bm.project_id,
                bm.file_id,
            );
            if bm.class_id.is_none() {
                bm.class_id = meta.class_id;
            }
        }
    }
}

/// Computes MD5 digest for data and returns hex string.
pub fn compute_md5(data: &[u8]) -> String {
    let mut h: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
    let mut k = [0u32; 64];
    for i in 0..64 {
        k[i] = ((1u64 << 32) as f64 * ((i as f64 + 1.0).sin().abs())) as u32;
    }
    let r: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22,
        5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
        4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23,
        6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];

    let mut msg = data.to_vec();
    let bit_len = (msg.len() as u64) * 8;
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 16];
        for (i, b) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_le_bytes(b.try_into().unwrap());
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];

        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };
            let temp = d;
            d = c;
            c = b;
            b = b.wrapping_add(
                (a.wrapping_add(f).wrapping_add(k[i]).wrapping_add(w[g])).rotate_left(r[i])
            );
            a = temp;
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
    }

    format!(
        "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        (h[0] & 0xff) as u8, ((h[0] >> 8) & 0xff) as u8, ((h[0] >> 16) & 0xff) as u8, ((h[0] >> 24) & 0xff) as u8,
        (h[1] & 0xff) as u8, ((h[1] >> 8) & 0xff) as u8, ((h[1] >> 16) & 0xff) as u8, ((h[1] >> 24) & 0xff) as u8,
        (h[2] & 0xff) as u8, ((h[2] >> 8) & 0xff) as u8, ((h[2] >> 16) & 0xff) as u8, ((h[2] >> 24) & 0xff) as u8,
        (h[3] & 0xff) as u8, ((h[3] >> 8) & 0xff) as u8, ((h[3] >> 16) & 0xff) as u8, ((h[3] >> 24) & 0xff) as u8,
    )
}

/// Installs a CurseForge catalog modpack by project ID.
///
/// When `instance_id` is provided (an update of an already-installed pack),
/// the existing instance is reused instead of creating a duplicate.
pub async fn install_curseforge_catalog_pack(
    project_id: u32,
    game_version: Option<String>,
    instance_id: Option<String>,
    file_id: Option<u32>,
) -> crate::Result<CurseForgeImportResult> {
    // 1. Fetch project info from CurseForge API
    let url = format!("{CURSEFORGE_MOD_URL}/{project_id}");
    let req_headers = cf_headers()?;

    emit_cf_install_progress(CfInstallProgress {
        project_id,
        phase: "fetching_pack".to_string(),
        current: 0,
        total: 1,
        bytes_downloaded: 0,
        total_bytes: 0,
        message: Some("Fetching modpack info".to_string()),
    })
    .await;

    let res = REQWEST_CLIENT
        .get(&url)
        .headers(req_headers.clone())
        .send()
        .await
        .map_err(|e| crate::Error::from(crate::ErrorKind::InputError(format!("Network error fetching mod {project_id}: {e}"))))?;

    if !res.status().is_success() {
        return Err(crate::Error::from(crate::ErrorKind::InputError(format!(
            "CurseForge API returned status {} for project {project_id}",
            res.status()
        ))));
    }

    let cf_mod_resp: CfModResponse = res.json().await.map_err(|e| {
        crate::Error::from(crate::ErrorKind::InputError(format!("Failed to parse mod {project_id} response: {e}")))
    })?;

    let mut cf_mod = cf_mod_resp.data;

    // 2. Select the file to install: an explicit file ID wins (version
    //    switching), otherwise pick the latest. The mod endpoint's
    //    `latestFiles` is a bounded window and is not guaranteed to be
    //    newest-first, so prefer the dedicated files endpoint and sort
    //    explicitly (Release over Beta/Alpha, newest fileDate within each
    //    tier).
    let pack_files = fetch_pack_files(
        project_id,
        &req_headers,
        std::mem::take(&mut cf_mod.latest_files),
    )
    .await;
    let selected_file = match file_id {
        Some(file_id) => pack_files
            .iter()
            .find(|f| f.id == file_id)
            .ok_or_else(|| {
                crate::Error::from(crate::ErrorKind::InputError(format!(
                    "File {file_id} not found for CurseForge project {project_id}"
                )))
            })?,
        None => select_latest_file(&pack_files, game_version.as_deref()).ok_or_else(|| {
            crate::Error::from(crate::ErrorKind::InputError(format!(
                "No files found for modpack project {project_id}"
            )))
        })?,
    };

    let selected_file_id = selected_file.id;

    // 3. Fetch pack file details & download pack archive to extract manifest.json & overrides
    let pack_download_url = selected_file.download_url.as_deref().ok_or_else(|| {
        crate::Error::from(crate::ErrorKind::InputError(format!(
            "Modpack file {selected_file_id} has no download URL"
        )))
    })?;

    let pack_bytes = REQWEST_CLIENT
        .get(pack_download_url)
        .send()
        .await
        .map_err(|e| crate::Error::from(crate::ErrorKind::InputError(format!("Failed to download modpack file: {e}"))))?
        .bytes()
        .await
        .map_err(|e| crate::Error::from(crate::ErrorKind::InputError(format!("Failed to read modpack file bytes: {e}"))))?;

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&pack_bytes)).map_err(|e| {
        crate::Error::from(crate::ErrorKind::InputError(format!(
            "Failed to open modpack zip archive: {e}"
        )))
    })?;

    let manifest_content = {
        let mut manifest_file = archive.by_name("manifest.json").map_err(|_| {
            crate::Error::from(crate::ErrorKind::InputError(
                "No manifest.json found in CurseForge modpack zip".to_string(),
            ))
        })?;
        let mut content = String::new();
        manifest_file.read_to_string(&mut content)?;
        content
    };

    let manifest: CurseForgeManifest = serde_json::from_str(&manifest_content)?;

    // Parse loader & version
    let mut mod_loader = ModLoader::Vanilla;
    let mut loader_version = None;

    if let Some(loader_info) = manifest
        .minecraft
        .mod_loaders
        .iter()
        .find(|l| l.primary)
        .or_else(|| manifest.minecraft.mod_loaders.first())
    {
        let id = loader_info.id.to_lowercase();
        let parts: Vec<&str> = id.splitn(2, '-').collect();
        if parts.len() == 2 {
            match parts[0] {
                "forge" => {
                    mod_loader = ModLoader::Forge;
                    loader_version = Some(parts[1].to_string());
                }
                "fabric" => {
                    mod_loader = ModLoader::Fabric;
                    loader_version = Some(parts[1].to_string());
                }
                "neoforge" => {
                    mod_loader = ModLoader::NeoForge;
                    loader_version = Some(parts[1].to_string());
                }
                "quilt" => {
                    mod_loader = ModLoader::Quilt;
                    loader_version = Some(parts[1].to_string());
                }
                _ => {}
            }
        }
    }

    let pack_name = cf_mod.name.clone();

    // Determine the instance to install into: reuse an existing one when
    // updating, otherwise create a fresh instance.
    let (instance_id, instance_dir, _is_update) = if let Some(existing_id) = instance_id {
        // Verify the existing instance is still around before reusing it.
        crate::api::instance::get(&existing_id).await?.ok_or_else(|| {
            crate::Error::from(crate::ErrorKind::InputError(format!(
                "Instance {existing_id} not found"
            )))
        })?;
        let instance_dir = crate::api::instance::get_full_path(&existing_id).await?;
        (existing_id, instance_dir, true)
    } else {
        let instance_meta = crate::api::instance::create(
            pack_name.clone(),
            manifest.minecraft.version.clone(),
            mod_loader,
            loader_version,
            None,
            // The resolved CurseForge project/file IDs are deliberately left
            // unset here: the instance becomes visible in the sidebar the
            // moment it is created, and exposing IDs that get resolved against
            // the Modrinth API before the pack is fully installed would let
            // the UI query a version ID that isn't installed yet (surfacing
            // "Linked modpack version ... not found"). The full link is
            // persisted at the end of this function, only after everything
            // has succeeded.
            InstanceLink::ImportedModpack {
                project_id: None,
                version_id: None,
                name: Some(pack_name.clone()),
                version_number: Some(manifest.version.clone()),
                filename: selected_file.file_name.clone(),
            },
            None,
        )
        .await?;
        let instance_id = instance_meta.instance.id.clone();
        let instance_dir = crate::api::instance::get_full_path(&instance_id).await?;
        (instance_id, instance_dir, false)
    };

    // Set the pack artwork as the instance icon (best-effort) so the sidebar
    // shows the real logo instead of the default/blank icon.
    set_cf_instance_icon(
        &instance_id,
        cf_mod
            .logo
            .as_ref()
            .and_then(|logo| logo.thumbnail_url.as_deref()),
    )
    .await;

    // The final link carries the resolved CurseForge project/file IDs. It is
    // persisted only at the end of this function, after downloads + Minecraft
    // install have succeeded, so a failed install never leaves the link
    // pointing at a version that isn't actually installed, and an in-flight
    // install never exposes the not-yet-installed version ID to the UI.
    let new_link = InstanceLink::ImportedModpack {
        project_id: Some(project_id.to_string()),
        version_id: Some(selected_file_id.to_string()),
        name: Some(pack_name.clone()),
        version_number: Some(manifest.version.clone()),
        filename: selected_file.file_name.clone(),
    };

    // Extract overrides folder
    let overrides_prefix = format!("{}/", manifest.overrides.trim_end_matches('/'));
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| {
            crate::Error::from(crate::ErrorKind::InputError(format!(
                "Failed to read zip entry: {e}"
            )))
        })?;
        let name = file.name().to_string();
        if name.starts_with(&overrides_prefix) {
            let rel_path = &name[overrides_prefix.len()..];
            if rel_path.is_empty() {
                continue;
            }
            let target_path = instance_dir.join(rel_path);
            if file.is_dir() || name.ends_with('/') {
                tokio::fs::create_dir_all(&target_path).await?;
            } else {
                if let Some(parent) = target_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let mut buffer = Vec::new();
                file.read_to_end(&mut buffer)?;
                tokio::fs::write(&target_path, buffer).await?;
            }
        }
    }

    // 4. Batched file resolution via CurseForge API
    // Only required files are installed; optional entries are skipped entirely.
    let file_ids: Vec<u32> = manifest
        .files
        .iter()
        .filter(|f| f.required)
        .map(|f| f.file_id)
        .collect();
    let mut cf_files_map: HashMap<u32, CfFile> = HashMap::new();

    if !file_ids.is_empty() {
        for chunk in file_ids.chunks(50) {
            let req_body = CfFilesRequest {
                file_ids: chunk.to_vec(),
            };

            let res = REQWEST_CLIENT
                .post(CURSEFORGE_FILES_URL)
                .headers(req_headers.clone())
                .json(&req_body)
                .send()
                .await;

            if let Ok(response) = res {
                if response.status().is_success() {
                    if let Ok(cf_res) = response.json::<CfFilesResponse>().await {
                        for file in cf_res.data {
                            cf_files_map.insert(file.id, file);
                        }
                    }
                }
            }
        }
    }

    // 5. Hash selection & 6. Blocked-mod detection + Modrinth fallback for sha1
    struct PendingMod {
        project_id: u32,
        file_id: u32,
        download_url: Option<String>,
        file_name: Option<String>,
        hash_type: Option<&'static str>,
        hash_value: Option<String>,
        file_length: Option<u64>,
        class_id: Option<u32>,
        blocked: bool,
    }

    let mut pending_mods: Vec<PendingMod> = Vec::new();
    let mut sha1_to_check: Vec<String> = Vec::new();

    for mf in &manifest.files {
        if !mf.required {
            continue;
        }

        let cf_file = cf_files_map.get(&mf.file_id);
        let download_url = cf_file.and_then(|f| f.download_url.clone());
        let file_name = cf_file.and_then(|f| f.file_name.clone());
        let file_length = cf_file.and_then(|f| f.file_length);
        let class_id = cf_file.and_then(|f| f.class_id);

        let (hash_type, hash_value) = cf_file
            .and_then(|f| f.hashes.as_deref())
            .and_then(select_supported_hash)
            .map(|(t, v)| (Some(t), Some(v)))
            .unwrap_or((None, None));

        // Mods whose author disallows third-party distribution (or which are
        // no longer available on CurseForge) must be downloaded manually —
        // they never qualify for the Modrinth hash fallback either.
        let distribution_blocked = cf_file.and_then(|f| f.allow_mod_distribution) == Some(false)
            || cf_file.and_then(|f| f.is_available) == Some(false);
        let blocked = distribution_blocked || is_blocked_url(download_url.as_deref());
        if blocked && !distribution_blocked && should_attempt_modrinth_fallback(hash_type, hash_value.as_deref()) {
            if let Some(ref h) = hash_value {
                sha1_to_check.push(h.clone());
            }
        }

        pending_mods.push(PendingMod {
            project_id: mf.project_id,
            file_id: mf.file_id,
            download_url,
            file_name,
            hash_type,
            hash_value,
            file_length,
            class_id,
            blocked,
        });
    }

    let mut modrinth_resolved: HashMap<String, String> = HashMap::new();
    if !sha1_to_check.is_empty() {
        let req_body = ModrinthVersionFilesRequest {
            hashes: sha1_to_check,
            algorithm: "sha1".to_string(),
        };

        let mut mr_headers = HeaderMap::new();
        mr_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        mr_headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&launcher_user_agent()).unwrap_or_else(|_| HeaderValue::from_static("ayinlauncher")),
        );

        let res = REQWEST_CLIENT
            .post(MODRINTH_VERSION_FILES_URL)
            .headers(mr_headers)
            .json(&req_body)
            .send()
            .await;

        if let Ok(response) = res {
            if response.status().is_success() {
                if let Ok(mr_map) = response.json::<HashMap<String, ModrinthVersionFileResult>>().await {
                    for (sha1_hash, result) in mr_map {
                        if let Some(file) = result.files.iter().find(|f| f.primary.unwrap_or(false)).or_else(|| result.files.first()) {
                            modrinth_resolved.insert(sha1_hash, file.url.clone());
                        }
                    }
                }
            }
        }
    }

    for pm in &mut pending_mods {
        if pm.blocked
            && let Some(ref sha1) = pm.hash_value
            && pm.hash_type == Some("sha1")
            && let Some(mr_url) = modrinth_resolved.get(sha1)
        {
            pm.download_url = Some(mr_url.clone());
            pm.blocked = false;
        }
    }

    // Fetch the parent mod metadata for every project in the pack. The files
    // endpoint does NOT return a per-file classId, so the mod's class ID is
    // what lets us route resource packs (12) and shader packs (6552) into
    // their correct instance folders instead of dumping everything in mods/.
    let project_ids: Vec<u32> = pending_mods.iter().map(|p| p.project_id).collect();
    let mod_meta = fetch_mod_metadata(&project_ids, &req_headers).await;

    // 7. Download execution with size & hash verification
    let mods_dir = instance_dir.join("mods");
    let resourcepacks_dir = instance_dir.join("resourcepacks");
    let shaderpacks_dir = instance_dir.join("shaderpacks");
    tokio::fs::create_dir_all(&mods_dir).await?;
    tokio::fs::create_dir_all(&resourcepacks_dir).await?;
    tokio::fs::create_dir_all(&shaderpacks_dir).await?;

    let total_files = pending_mods.len().max(1) as u64;
    let mut blocked_mods: Vec<BlockedMod> = Vec::new();

    // Wrap in Arc so it can be shared across concurrent download tasks.
    let mod_meta = std::sync::Arc::new(mod_meta);
    let mods_dir = std::sync::Arc::new(mods_dir);
    let resourcepacks_dir = std::sync::Arc::new(resourcepacks_dir);
    let shaderpacks_dir = std::sync::Arc::new(shaderpacks_dir);

    // Pre-compute total expected bytes for non-blocked files so the frontend
    // can display a total-downloaded / total-expected metric. Blocked files
    // have no known size (the user downloads them manually), so they are
    // excluded.
    let known_total_bytes: u64 = pending_mods
        .iter()
        .filter(|pm| !pm.blocked)
        .filter_map(|pm| pm.file_length)
        .sum();
    let total_bytes = std::sync::Arc::new(AtomicU64::new(known_total_bytes));

    // Download files concurrently with bounded parallelism (12 concurrent
    // downloads). Each download task returns Ok(Option<BlockedMod>) — None on
    // success, Some(blocked_mod) on a non-fatal failure. Hard errors (hash
    // mismatch, size mismatch, I/O) propagate through the stream.
    let downloaded = std::sync::Arc::new(AtomicU64::new(0));
    let bytes_downloaded = std::sync::Arc::new(AtomicU64::new(0));
    let results: Vec<crate::Result<Option<BlockedMod>>> = stream::iter(pending_mods.into_iter().enumerate().map(|(_idx, pm)| pm))
        .map(|pm| {
            let mod_meta = std::sync::Arc::clone(&mod_meta);
            let mods_dir = std::sync::Arc::clone(&mods_dir);
            let resourcepacks_dir = std::sync::Arc::clone(&resourcepacks_dir);
            let shaderpacks_dir = std::sync::Arc::clone(&shaderpacks_dir);
            let downloaded = std::sync::Arc::clone(&downloaded);
            let bytes_downloaded = std::sync::Arc::clone(&bytes_downloaded);
            let total_bytes = std::sync::Arc::clone(&total_bytes);
            async move {
                if pm.blocked {
                    let bm = construct_blocked_mod(
                        pm.project_id,
                        pm.file_id,
                        pm.file_name.as_deref(),
                        pm.hash_value.as_deref(),
                        pm.class_id,
                    );
                    downloaded.fetch_add(1, Ordering::SeqCst);
                    return Ok(Some(bm));
                }

                let download_url = match pm.download_url {
                    Some(url) => url,
                    None => {
                        downloaded.fetch_add(1, Ordering::SeqCst);
                        return Ok(Some(construct_blocked_mod(
                            pm.project_id,
                            pm.file_id,
                            pm.file_name.as_deref(),
                            pm.hash_value.as_deref(),
                            pm.class_id,
                        )));
                    }
                };
                let file_name = pm.file_name.clone().unwrap_or_else(|| format!("mod-{}-{}.jar", pm.project_id, pm.file_id));
                let file_name_display = file_name.clone();

                let res = REQWEST_CLIENT.get(&download_url).send().await;
                let response = match res {
                    Ok(r) if r.status().is_success() => r,
                    _ => {
                        downloaded.fetch_add(1, Ordering::SeqCst);
                        return Ok(Some(construct_blocked_mod(
                            pm.project_id,
                            pm.file_id,
                            Some(&file_name),
                            pm.hash_value.as_deref(),
                            pm.class_id,
                        )));
                    }
                };

                let file_bytes = match response.bytes().await {
                    Ok(b) => b,
                    Err(_) => {
                        downloaded.fetch_add(1, Ordering::SeqCst);
                        return Ok(Some(construct_blocked_mod(
                            pm.project_id,
                            pm.file_id,
                            Some(&file_name),
                            pm.hash_value.as_deref(),
                            pm.class_id,
                        )));
                    }
                };

                if let Some(expected_size) = pm.file_length {
                    if expected_size > 0 && (file_bytes.len() as u64) != expected_size {
                        return Err(crate::Error::from(crate::ErrorKind::InputError(format!(
                            "Size mismatch for file {file_name}: expected {expected_size}, got {}",
                            file_bytes.len()
                        ))));
                    }
                }

                if let (Some(hash_type), Some(expected_hash)) = (pm.hash_type, &pm.hash_value) {
                    match hash_type {
                        "sha1" => {
                            let computed = sha1_smol::Sha1::from(&file_bytes).digest().to_string();
                            if !computed.eq_ignore_ascii_case(expected_hash) {
                                return Err(crate::Error::from(crate::ErrorKind::InputError(format!(
                                    "SHA1 hash mismatch for file {file_name}: expected {expected_hash}, got {computed}"
                                ))));
                            }
                        }
                        "md5" => {
                            let computed = compute_md5(&file_bytes);
                            if !computed.eq_ignore_ascii_case(expected_hash) {
                                return Err(crate::Error::from(crate::ErrorKind::InputError(format!(
                                    "MD5 hash mismatch for file {file_name}: expected {expected_hash}, got {computed}"
                                ))));
                            }
                        }
                        _ => {}
                    }
                }

                // Route via the shared resolver — single source of truth.
                let target_dir = match resolve_content_dir(
                    pm.class_id,
                    mod_meta.get(&pm.project_id).and_then(|m| m.class_id),
                    Some(&file_name),
                ) {
                    CfContentDir::ResourcePacks => resourcepacks_dir,
                    CfContentDir::ShaderPacks => shaderpacks_dir,
                    CfContentDir::Mods => mods_dir,
                };
                let file_path = target_dir.join(&file_name);
                tokio::fs::write(&file_path, &file_bytes).await?;
                let completed = downloaded.fetch_add(1, Ordering::SeqCst) + 1;
                bytes_downloaded.fetch_add(file_bytes.len() as u64, Ordering::SeqCst);
                let cur_bytes = bytes_downloaded.load(Ordering::SeqCst);
                emit_cf_install_progress(CfInstallProgress {
                    project_id,
                    phase: "downloading_mods".to_string(),
                    current: completed,
                    total: total_files,
                    bytes_downloaded: cur_bytes,
                    total_bytes: total_bytes.load(Ordering::SeqCst),
                    message: Some(format!("Downloading {file_name_display}")),
                })
                .await;
                Ok(None)
            }
        })
        .buffer_unordered(12)
        .collect::<Vec<_>>()
        .await;

    for result in results {
        match result {
            Ok(Some(bm)) => blocked_mods.push(bm),
            Err(e) => return Err(e),
            Ok(None) => {}
        }
    }
    let downloaded_files = downloaded.load(std::sync::atomic::Ordering::SeqCst);

    emit_cf_install_progress(CfInstallProgress {
        project_id,
        phase: "installing_minecraft".to_string(),
        current: downloaded_files,
        total: total_files,
        bytes_downloaded: bytes_downloaded.load(Ordering::SeqCst),
        total_bytes: total_bytes.load(Ordering::SeqCst),
        message: Some("Installing Minecraft and loader".to_string()),
    })
    .await;

    // Finish by installing Minecraft + the loader so the instance is fully launchable.
    crate::launcher::install_minecraft_for_instance_id_with_reporter(
        &instance_id,
        false,
        None,
    )
    .await?;

    // Only now that everything succeeded: persist the resolved CurseForge link
    // (on both fresh installs and updates). Before this point the link carries
    // no project/version IDs, so the UI cannot query a version ID that hasn't
    // been fully installed yet, and a failed install never leaves the link
    // pointing at a version that isn't actually installed. On updates this
    // also refreshes the link so future update checks compare against the
    // freshly installed file.
    crate::api::instance::edit(
        &instance_id,
        EditInstance {
            link: Some(new_link),
            ..EditInstance::default()
        },
    )
    .await?;

    // Enrich blocked mods with the real file-page URL (and a class ID when
    // the file didn't carry one) from the metadata fetched before the
    // downloads. Runs before the "finished" event so the UI never reports
    // completion while network calls are in flight.
    enrich_blocked_mods(&mut blocked_mods, &mod_meta);

    emit_cf_install_progress(CfInstallProgress {
        project_id,
        phase: "finished".to_string(),
        current: total_files,
        total: total_files,
        bytes_downloaded: bytes_downloaded.load(Ordering::SeqCst),
        total_bytes: total_bytes.load(Ordering::SeqCst),
        message: None,
    })
    .await;

    Ok(CurseForgeImportResult {
        instance_id,
        blocked_mods,
    })
}

/// A single available file for an installed CurseForge catalog pack, as shown
/// in the "Change version" picker.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CfPackFileInfo {
    pub file_id: u32,
    pub file_name: String,
    pub release_type: Option<u32>,
    pub file_date: Option<String>,
    pub game_versions: Option<Vec<String>>,
    pub download_url: Option<String>,
}

/// Available files for an installed CurseForge catalog pack, sorted so the
/// preferred "latest" file is first, plus the resolved latest file ID.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CfPackFilesResult {
    pub project_id: u32,
    pub latest_file_id: u32,
    pub files: Vec<CfPackFileInfo>,
}

/// Resolves the CurseForge project ID from an installed instance's link.
fn curseforge_project_id_from_link(
    link: InstanceLink,
) -> crate::Result<u32> {
    match link {
        InstanceLink::ImportedModpack {
            project_id: Some(pid),
            ..
        } => pid.parse::<u32>().map_err(|_| {
            crate::Error::from(crate::ErrorKind::InputError(format!(
                "Invalid project_id {pid} in instance link"
            )))
        }),
        _ => Err(crate::Error::from(crate::ErrorKind::InputError(
            "Instance is not linked to a CurseForge modpack".to_string(),
        ))),
    }
}

/// Lists the available files for an installed CurseForge catalog pack, sorted
/// newest-first using the same release-preference rules as the installer.
pub async fn get_curseforge_pack_files(
    instance_id: String,
) -> crate::Result<CfPackFilesResult> {
    let instance = crate::api::instance::get(&instance_id)
        .await?
        .ok_or_else(|| {
            crate::Error::from(crate::ErrorKind::InputError(format!(
                "Instance {instance_id} not found"
            )))
        })?;
    let (project_id, installed_file_id, installed_filename) = match instance.link {
        InstanceLink::ImportedModpack {
            project_id: Some(pid),
            version_id: Some(vid),
            filename,
            ..
        } => {
            let pid_u32 = pid.parse::<u32>().map_err(|_| {
                crate::Error::from(crate::ErrorKind::InputError(format!(
                    "Invalid project_id {pid} in instance link"
                )))
            })?;
            let vid_u32 = vid.parse::<u32>().map_err(|_| {
                crate::Error::from(crate::ErrorKind::InputError(format!(
                    "Invalid file_id {vid} in instance link"
                )))
            })?;
            (pid_u32, vid_u32, filename)
        }
        _ => {
            return Err(crate::Error::from(crate::ErrorKind::InputError(
                "Instance is not linked to a CurseForge modpack".to_string(),
            )))
        }
    };
    // Latest is computed against the instance's game version so the picker's
    // "Latest" badge agrees with check_curseforge_pack_update.
    let game_version = instance.applied_content_set.game_version;

    let req_headers = cf_headers()?;
    let url = format!("{CURSEFORGE_MOD_URL}/{project_id}");
    let res = REQWEST_CLIENT
        .get(&url)
        .headers(req_headers.clone())
        .send()
        .await
        .map_err(|e| crate::Error::from(crate::ErrorKind::InputError(format!("Network error fetching mod {project_id}: {e}"))))?;
    if !res.status().is_success() {
        return Err(crate::Error::from(crate::ErrorKind::InputError(format!(
            "CurseForge API returned status {} for project {project_id}",
            res.status()
        ))));
    }
    let mut cf_mod_resp: CfModResponse = res.json().await.map_err(|e| {
        crate::Error::from(crate::ErrorKind::InputError(format!(
            "Failed to parse mod {project_id} response: {e}"
        )))
    })?;

    let mut pack_files = fetch_pack_files(
        project_id,
        &req_headers,
        std::mem::take(&mut cf_mod_resp.data.latest_files),
    )
    .await;
    let latest_file_id = select_latest_file(&pack_files, Some(&game_version))
        .map(|f| f.id)
        .unwrap_or(0);
    pack_files.sort_by(compare_files_latest);

    let mut files = pack_files
        .into_iter()
        .map(|file| CfPackFileInfo {
            file_id: file.id,
            file_name: file
                .file_name
                .unwrap_or_else(|| format!("file-{}.jar", file.id)),
            release_type: file.release_type,
            file_date: file.file_date,
            game_versions: file.game_versions,
            download_url: file.download_url,
        })
        .collect::<Vec<_>>();

    // Guarantee the currently installed file is always listed so the UI can
    // show its "Currently Installed" badge even if it fell outside the
    // newest-files window returned by the API.
    if !files.iter().any(|f| f.file_id == installed_file_id) {
        files.push(CfPackFileInfo {
            file_id: installed_file_id,
            file_name: installed_filename.unwrap_or_else(|| {
                format!("file-{installed_file_id}.jar")
            }),
            release_type: None,
            file_date: None,
            game_versions: None,
            download_url: None,
        });
    }

    Ok(CfPackFilesResult {
        project_id,
        latest_file_id,
        files,
    })
}

/// Switches an installed CurseForge catalog pack instance to a specific file.
///
/// Reuses the exact same install pipeline as a fresh install (download,
/// blocked-mod detection, overrides extraction, Minecraft/loader install),
/// and only persists the link pointing at the new file after everything has
/// succeeded — a failed switch never leaves the link pointing at a version
/// that isn't actually installed.
pub async fn change_curseforge_pack_version(
    instance_id: String,
    file_id: u32,
) -> crate::Result<CurseForgeImportResult> {
    let instance = crate::api::instance::get(&instance_id)
        .await?
        .ok_or_else(|| {
            crate::Error::from(crate::ErrorKind::InputError(format!(
                "Instance {instance_id} not found"
            )))
        })?;
    let project_id = curseforge_project_id_from_link(instance.link)?;

    install_curseforge_catalog_pack(project_id, None, Some(instance_id), Some(file_id)).await
}

/// Outcome of the Downloads-folder scan for a single blocked mod, so the UI
/// can render a per-item status.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BlockedModScanItem {
    pub file_id: u32,
    /// "moved", "not_found", or "conflict" (a file with the expected name
    /// already existed in the destination folder and was replaced).
    pub status: String,
    /// The instance subfolder the file was moved into — "mods",
    /// "resourcepacks", or "shaderpacks" (set for moved/conflict items).
    pub destination: Option<String>,
}

/// Result of a Downloads-folder scan for manually-downloaded blocked mods.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BlockedModScanResult {
    /// How many files were found in the Downloads folder and moved into the
    /// instance's mods folder.
    pub moved: u32,
    /// Blocked mods that could not be found in the Downloads folder.
    pub remaining: Vec<BlockedMod>,
    /// Per-item outcome for every blocked mod passed in, keyed by file_id.
    pub items: Vec<BlockedModScanItem>,
}

/// Scans the user's OS Downloads folder (resolved via the standard platform
/// downloads directory, never a hardcoded path) for files matching the
/// expected filename of any given blocked mod (case-insensitive, tolerant of
/// minor punctuation/version variance and browser duplicate-download suffixes),
/// and MOVES the matches into the instance's mods folder. Returns how many
/// were moved plus the mods that remain missing. This is an on-demand, one-time
/// scan — there is no folder watching.
pub async fn scan_downloads_for_blocked_mods(
    instance_id: String,
    blocked_mods: Vec<BlockedMod>,
) -> crate::Result<BlockedModScanResult> {
    crate::api::instance::get(&instance_id)
        .await?
        .ok_or_else(|| {
            crate::Error::from(crate::ErrorKind::InputError(format!(
                "Instance {instance_id} not found"
            )))
        })?;
    let instance_dir = crate::api::instance::get_full_path(&instance_id).await?;
    let mods_dir = instance_dir.join("mods");
    let resourcepacks_dir = instance_dir.join("resourcepacks");
    let shaderpacks_dir = instance_dir.join("shaderpacks");
    tokio::fs::create_dir_all(&mods_dir).await?;
    tokio::fs::create_dir_all(&resourcepacks_dir).await?;
    tokio::fs::create_dir_all(&shaderpacks_dir).await?;

    // Per-item outcomes for the UI (every blocked mod gets an entry so the
    // dialog can render a status badge for each row).
    let mut items: Vec<BlockedModScanItem> = blocked_mods
        .iter()
        .map(|bm| BlockedModScanItem {
            file_id: bm.file_id,
            status: "not_found".to_string(),
            destination: None,
        })
        .collect();

    let downloads_dir = dirs::download_dir().ok_or_else(|| {
        crate::Error::from(crate::ErrorKind::InputError(
            "Could not resolve the OS Downloads folder".to_string(),
        ))
    })?;
    if !downloads_dir.is_dir() {
        return Ok(BlockedModScanResult {
            moved: 0,
            remaining: blocked_mods,
            items,
        });
    }

    let mut matched_file_ids: HashSet<u32> = HashSet::new();
    let mut moved = 0u32;

    let mut entries = tokio::fs::read_dir(&downloads_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(candidate_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // Match the candidate against the first still-unmatched blocked mod.
        let Some(idx) = blocked_mods
            .iter()
            .position(|bm| {
                !matched_file_ids.contains(&bm.file_id)
                    && filename_matches(&bm.file_name, candidate_name)
            })
        else {
            continue;
        };

        // Sanitize the destination filename (final path component only) so an
        // API-provided name can never escape the mods folder.
        let Some(dest_name) =
            std::path::Path::new(&blocked_mods[idx].file_name).file_name()
        else {
            continue;
        };
        // Resolve the destination folder with the exact same class-based
        // routing as the automated installer (single source of truth — the
        // blocked mod already carries its resolved class ID; filename
        // heuristics catch anything still unknown).
        let dest_dir = match resolve_content_dir(
            blocked_mods[idx].class_id,
            None,
            Some(&blocked_mods[idx].file_name),
        ) {
            CfContentDir::Mods => &mods_dir,
            CfContentDir::ResourcePacks => &resourcepacks_dir,
            CfContentDir::ShaderPacks => &shaderpacks_dir,
        };
        // The destination always uses the blocked mod's EXPECTED filename —
        // the found Downloads file (which can keep a slightly different name
        // than the manifest's fileName) is renamed to the expected name on
        // move, so the placed file has the correct mod identity. A file that
        // already exists there (e.g. from a previous scan) is replaced and
        // reported as a conflict.
        let dest = dest_dir.join(dest_name);
        let conflict = dest.exists();
        if conflict {
            crate::util::io::remove_file(&dest).await?;
        }
        crate::util::io::rename_or_move(&path, &dest).await?;
        matched_file_ids.insert(blocked_mods[idx].file_id);
        let dest_label = dest_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("mods")
            .to_string();
        items[idx].status = if conflict { "conflict" } else { "moved" }.to_string();
        items[idx].destination = Some(dest_label);
        moved += 1;
    }

    let remaining: Vec<BlockedMod> = blocked_mods
        .into_iter()
        .filter(|bm| !matched_file_ids.contains(&bm.file_id))
        .collect();

    Ok(BlockedModScanResult {
        moved,
        remaining,
        items,
    })
}

/// Returns true if a file found in the Downloads folder should be treated as a
/// match for the expected CurseForge filename. Matching is case-insensitive
/// and tolerates minor punctuation differences (spaces, dashes, underscores,
/// dots), browser duplicate-download suffixes (" (1)", " - Copy", ...) and
/// differing version strings (e.g. "quark-r1.4.jar" vs "quark-r1.5.jar") as
/// long as the mod's base name and file extension agree.
pub fn filename_matches(expected: &str, candidate: &str) -> bool {
    let (expected_stem, expected_ext) = split_stem_and_ext(expected);
    let (candidate_stem, candidate_ext) = split_stem_and_ext(candidate);

    // Extensions must agree (e.g. .jar vs .zip).
    if !expected_ext.eq_ignore_ascii_case(candidate_ext) {
        return false;
    }

    // 1. Exact (case-insensitive) stem match.
    if expected_stem.eq_ignore_ascii_case(candidate_stem) {
        return true;
    }

    // 2. Punctuation-tolerant match.
    if normalize_stem(expected_stem) == normalize_stem(candidate_stem) {
        return true;
    }

    // 3. Version-tolerant match on the mod's base name (also catches browser
    //    duplicate-download suffixes like " (1)").
    let expected_base = version_stripped(expected_stem);
    let candidate_base = version_stripped(candidate_stem);
    if expected_base.len() >= 3 && expected_base == candidate_base {
        return true;
    }

    false
}

/// Splits a filename into its stem and extension (e.g. "quark.jar" ->
/// ("quark", "jar")). Files without an extension compare on the whole name.
fn split_stem_and_ext(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() && ext.len() <= 10 => (stem, ext),
        _ => (name, ""),
    }
}

/// Lowercases a stem and drops every non-alphanumeric character, so
/// "Quark_R1.4-121" and "quark r1.4 121" compare equal.
fn normalize_stem(stem: &str) -> String {
    stem.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Strips trailing version-like tokens from a mod file stem (and any browser
/// duplicate-download marker first), e.g. "quark-r1.4-121" -> "quark",
/// "jei_1.12.2" -> "jei", "OptiFine (1)" -> "OptiFine".
fn version_stripped(mut stem: &str) -> &str {
    stem = strip_browser_suffix(stem);
    loop {
        let Some(sep_idx) = stem.rfind(|c: char| matches!(c, '-' | '_' | '.' | ' ')) else {
            return stem;
        };
        let (base, tail) = stem.split_at(sep_idx + 1);
        let tail_trimmed = tail.trim_start_matches(['v', 'V', 'r', 'R']);
        if tail_trimmed.starts_with(|c: char| c.is_ascii_digit()) {
            stem = base.trim_end_matches(['-', '_', '.', ' ']);
        } else {
            return stem;
        }
    }
}

/// Strips a trailing browser duplicate-download marker from a filename stem,
/// e.g. "quark (1)" -> "quark", "quark - Copy" -> "quark", "quark(2)" ->
/// "quark".
fn strip_browser_suffix(mut stem: &str) -> &str {
    loop {
        let trimmed = stem.trim_end();
        let Some((marker_start, marker_len)) = duplicate_marker(trimmed) else {
            return trimmed;
        };
        let prefix = trimmed[..marker_start].trim_end();
        if prefix.is_empty() {
            return trimmed;
        }
        stem = prefix;
        let _ = marker_len;
    }
}

/// Returns (start_index, length) of a duplicate-download marker at the end of
/// `name`, e.g. " (1)", " - Copy", "-copy", if present. The returned range
/// includes any preceding separator characters (spaces and hyphens) so the
/// remaining prefix is always a clean filename stem.
fn duplicate_marker(name: &str) -> Option<(usize, usize)> {
    // Parenthesized counter: "(1)", "(2)", ...
    if let Some(open) = name.rfind('(') {
        let tail = &name[open..];
        if tail.len() >= 3
            && tail.ends_with(')')
            && tail[1..tail.len() - 1].chars().all(|c| c.is_ascii_digit())
        {
            return Some((open, tail.len()));
        }
    }
    // "copy" / "duplicate" (any separator layout, e.g. " - Copy", "-copy",
    // " copy"), expanded leftwards over spaces and hyphens so "quark - Copy"
    // strips cleanly to "quark" instead of "quark -".
    let lower = name.to_ascii_lowercase();
    for word in ["copy", "duplicate"] {
        if let Some(word_start) = lower.rfind(word) {
            if word_start + word.len() == lower.len() {
                let bytes = lower.as_bytes();
                let mut start = word_start;
                while start > 0 && (bytes[start - 1] == b' ' || bytes[start - 1] == b'-') {
                    start -= 1;
                }
                return Some((start, word_start + word.len() - start));
            }
        }
    }
    None
}

/// Checks if an update is available for an installed CurseForge catalog modpack instance.
pub async fn check_curseforge_pack_update(instance_id: String) -> crate::Result<bool> {
    let instance = crate::api::instance::get(&instance_id)
        .await?
        .ok_or_else(|| {
            crate::Error::from(crate::ErrorKind::InputError(format!(
                "Instance {instance_id} not found"
            )))
        })?;

    let (project_id, installed_file_id) = match instance.link {
        InstanceLink::ImportedModpack {
            project_id: Some(pid),
            version_id: Some(vid),
            ..
        } => {
            let pid_u32 = pid.parse::<u32>().map_err(|_| {
                crate::Error::from(crate::ErrorKind::InputError(format!(
                    "Invalid project_id {pid} in instance link"
                )))
            })?;
            let vid_u32 = vid.parse::<u32>().map_err(|_| {
                crate::Error::from(crate::ErrorKind::InputError(format!(
                    "Invalid file_id {vid} in instance link"
                )))
            })?;
            (pid_u32, vid_u32)
        }
        _ => return Ok(false),
    };

    let url = format!("{CURSEFORGE_MOD_URL}/{project_id}");
    let req_headers = cf_headers()?;

    let res = REQWEST_CLIENT
        .get(&url)
        .headers(req_headers.clone())
        .send()
        .await
        .map_err(|e| crate::Error::from(crate::ErrorKind::InputError(format!("Network error checking updates for mod {project_id}: {e}"))))?;

    if !res.status().is_success() {
        return Ok(false);
    }

    let mut cf_mod_resp: CfModResponse = res.json().await.map_err(|e| {
        crate::Error::from(crate::ErrorKind::InputError(format!("Failed to parse mod response: {e}")))
    })?;

    // Backfill: older CurseForge installs predate icon capture. If this
    // instance has no icon yet, opportunistically fetch and cache the pack
    // logo (best-effort; never fail the update check over it).
    if instance.instance.icon_path.as_deref().map_or(true, str::is_empty) {
        if let Some(logo) = cf_mod_resp.data.logo.as_ref() {
            set_cf_instance_icon(
                &instance_id,
                logo.thumbnail_url.as_deref().or(logo.url.as_deref()),
            )
            .await;
        }
    }

    // Same selection rules as the installer: prefer the dedicated files
    // endpoint and sort explicitly by release type + file date.
    let pack_files = fetch_pack_files(
        project_id,
        &req_headers,
        std::mem::take(&mut cf_mod_resp.data.latest_files),
    )
    .await;
    let latest_file = select_latest_file(
        &pack_files,
        Some(&instance.applied_content_set.game_version),
    );

    let Some(latest_file) = latest_file else {
        return Ok(false);
    };

    Ok(latest_file.id != installed_file_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_selection_chooses_first_supported_algorithm() {
        let hashes = vec![
            CfHash {
                value: "unsupported_murmur2_val".to_string(),
                algo: serde_json::json!(3),
            },
            CfHash {
                value: "unsupported_string_algo".to_string(),
                algo: serde_json::json!("murmur2"),
            },
            CfHash {
                value: "a1b2c3d4e5f678901234567890abcdef12345678".to_string(),
                algo: serde_json::json!(1), // sha1
            },
            CfHash {
                value: "1234567890abcdef1234567890abcdef".to_string(),
                algo: serde_json::json!(2), // md5
            },
        ];

        let selected = select_supported_hash(&hashes);
        assert!(selected.is_some(), "Expected a supported hash to be selected");
        let (algo, val) = selected.unwrap();
        assert_eq!(algo, "sha1");
        assert_eq!(val, "a1b2c3d4e5f678901234567890abcdef12345678");

        let hashes_md5_first = vec![
            CfHash {
                value: "ignore_unknown".to_string(),
                algo: serde_json::json!(999),
            },
            CfHash {
                value: "fedcba9876543210fedcba9876543210".to_string(),
                algo: serde_json::json!("md5"),
            },
            CfHash {
                value: "a1b2c3d4e5f678901234567890abcdef12345678".to_string(),
                algo: serde_json::json!("sha1"),
            },
        ];

        let selected_md5 = select_supported_hash(&hashes_md5_first);
        assert!(selected_md5.is_some());
        let (algo_md5, val_md5) = selected_md5.unwrap();
        assert_eq!(algo_md5, "md5");
        assert_eq!(val_md5, "fedcba9876543210fedcba9876543210");
    }

    #[test]
    fn test_blocked_mod_triggers_modrinth_fallback_only_for_sha1() {
        assert!(should_attempt_modrinth_fallback(
            Some("sha1"),
            Some("a1b2c3d4e5f678901234567890abcdef12345678")
        ));

        assert!(!should_attempt_modrinth_fallback(
            Some("md5"),
            Some("1234567890abcdef1234567890abcdef")
        ));

        assert!(!should_attempt_modrinth_fallback(None, Some("hash_val")));
        assert!(!should_attempt_modrinth_fallback(Some("sha1"), Some("")));
        assert!(!should_attempt_modrinth_fallback(Some("sha1"), None));
    }

    #[test]
    fn test_file_failing_all_resolution_paths_appears_in_blocked_mods_list() {
        let project_id = 12345;
        let file_id = 67890;
        let file_name = Some("custom-blocked-mod.jar");
        let hash = Some("sha1_hash_abc");

        let blocked = construct_blocked_mod(project_id, file_id, file_name, hash, Some(6));

        assert_eq!(blocked.project_id, 12345);
        assert_eq!(blocked.file_id, 67890);
        assert_eq!(blocked.name, "custom-blocked-mod.jar");
        assert_eq!(blocked.file_name, "custom-blocked-mod.jar");
        assert_eq!(blocked.class_id, Some(6));
        assert_eq!(
            blocked.website_url,
            "https://www.curseforge.com/projects/12345/download/67890"
        );
        assert_eq!(blocked.hash, "sha1_hash_abc");

        let blocked_no_name = construct_blocked_mod(111, 222, None, None, None);
        assert_eq!(blocked_no_name.name, "mod-111-222.jar");
        assert_eq!(blocked_no_name.file_name, "mod-111-222.jar");
        assert_eq!(blocked_no_name.class_id, None);
        assert_eq!(
            blocked_no_name.website_url,
            "https://www.curseforge.com/projects/111/download/222"
        );
        assert_eq!(blocked_no_name.hash, "");
    }

    #[test]
    fn test_resolve_content_dir_routes_resource_and_shader_packs() {
        // File-level class ID wins when the API provides one.
        assert_eq!(resolve_content_dir(Some(12), None, None), CfContentDir::ResourcePacks);
        assert_eq!(resolve_content_dir(Some(6552), None, None), CfContentDir::ShaderPacks);
        assert_eq!(resolve_content_dir(Some(6), None, None), CfContentDir::Mods);

        // Missing file class ID falls back to the parent mod's class ID — the
        // real-world case, since the files API omits per-file classId.
        assert_eq!(resolve_content_dir(None, Some(12), None), CfContentDir::ResourcePacks);
        assert_eq!(resolve_content_dir(None, Some(6552), None), CfContentDir::ShaderPacks);
        assert_eq!(resolve_content_dir(None, Some(6), None), CfContentDir::Mods);

        // No class IDs at all -> filename heuristics, then Mods default.
        assert_eq!(
            resolve_content_dir(None, None, Some("BSL Shaders.zip")),
            CfContentDir::ShaderPacks
        );
        assert_eq!(
            resolve_content_dir(None, None, Some("BetterVanillaTextures.zip")),
            CfContentDir::ResourcePacks
        );
        assert_eq!(
            resolve_content_dir(None, None, Some("just-a-mod.jar")),
            CfContentDir::Mods
        );
        assert_eq!(resolve_content_dir(None, None, None), CfContentDir::Mods);
    }

    #[test]
    fn test_build_blocked_mod_website_url_uses_real_slug_and_class_segment() {
        // websiteUrl already contains the authoritative category segment.
        let url = build_blocked_mod_website_url(
            Some("https://www.curseforge.com/minecraft/mc-mods/jei"),
            Some("jei"),
            Some(6),
            238222,
            4700651,
        );
        assert_eq!(
            url,
            "https://www.curseforge.com/minecraft/mc-mods/jei/download/4700651"
        );

        // A trailing /files/<numeric-id> on websiteUrl is tolerated.
        let url = build_blocked_mod_website_url(
            Some("https://www.curseforge.com/minecraft/texture-packs/faithful/files/123456"),
            Some("faithful"),
            Some(12),
            123,
            123456,
        );
        assert_eq!(
            url,
            "https://www.curseforge.com/minecraft/texture-packs/faithful/download/123456"
        );

        // Missing websiteUrl falls back to the class-ID mapping.
        let url = build_blocked_mod_website_url(None, Some("jei"), Some(6), 238222, 4700651);
        assert_eq!(
            url,
            "https://www.curseforge.com/minecraft/mc-mods/jei/download/4700651"
        );

        // No slug at all -> numeric fallback (still better than nothing).
        let url = build_blocked_mod_website_url(None, None, None, 238222, 4700651);
        assert_eq!(
            url,
            "https://www.curseforge.com/projects/238222/download/4700651"
        );
    }

    #[test]
    fn test_filename_matching_is_case_insensitive_and_version_tolerant() {
        assert!(filename_matches(
            "quark-r1.4-121.jar",
            "Quark_R1.4-121.jar"
        ));
        assert!(filename_matches("quark-r1.4-121.jar", "quark-r1.5.jar"));
        assert!(filename_matches("quark-r1.4-121.jar", "quark (1).jar"));
        assert!(filename_matches("jei_1.12.2.jar", "JEI 1.12.2.jar"));
        assert!(filename_matches("quark.jar", "quark - Copy.jar"));
        assert!(!filename_matches(
            "quark-r1.4-121.jar",
            "quake-r1.4-121.jar"
        ));
        assert!(!filename_matches("jei_1.12.2.jar", "jei_1.12.2.zip"));
    }

    #[test]
    fn test_select_latest_file_prefers_release_over_newer_alpha() {
        // The API may return files in any order. The alpha here has BOTH a
        // later file ID and a later publish date than the release, and is
        // listed first — the release must still win because of release type.
        let files = vec![
            CfFile {
                id: 900,
                mod_id: 1,
                file_name: Some("RLCraft Alpha v2.0.zip".to_string()),
                download_url: None,
                game_versions: Some(vec!["1.12.2".to_string()]),
                hashes: None,
                file_length: None,
                dependencies: None,
                class_id: None,
                allow_mod_distribution: None,
                is_available: None,
                file_date: Some("2026-01-15T00:00:00Z".to_string()),
                release_type: Some(3),
            },
            CfFile {
                id: 700,
                mod_id: 1,
                file_name: Some("RLCraft Release v2.9.3.zip".to_string()),
                download_url: None,
                game_versions: Some(vec!["1.12.2".to_string()]),
                hashes: None,
                file_length: None,
                dependencies: None,
                class_id: None,
                allow_mod_distribution: None,
                is_available: None,
                file_date: Some("2025-06-01T00:00:00Z".to_string()),
                release_type: Some(1),
            },
            CfFile {
                id: 800,
                mod_id: 1,
                file_name: Some("RLCraft Beta v2.5.zip".to_string()),
                download_url: None,
                game_versions: Some(vec!["1.12.2".to_string()]),
                hashes: None,
                file_length: None,
                dependencies: None,
                class_id: None,
                allow_mod_distribution: None,
                is_available: None,
                file_date: Some("2025-09-01T00:00:00Z".to_string()),
                release_type: Some(2),
            },
        ];

        // Release beats the newer Beta and Alpha, regardless of list order.
        let selected = select_latest_file(&files, Some("1.12.2")).unwrap();
        assert_eq!(selected.id, 700);
        assert_eq!(
            selected.file_name.as_deref(),
            Some("RLCraft Release v2.9.3.zip")
        );

        // When no Release exists, the newest Beta wins over the newest Alpha.
        let no_release: Vec<CfFile> = files
            .iter()
            .filter(|f| f.release_type != Some(1))
            .cloned()
            .collect();
        let selected = select_latest_file(&no_release, Some("1.12.2")).unwrap();
        assert_eq!(selected.release_type, Some(2));
        assert_eq!(selected.id, 800);

        // With only Alpha files, the newest Alpha wins.
        let only_alpha: Vec<CfFile> = files
            .iter()
            .filter(|f| f.release_type == Some(3))
            .cloned()
            .collect();
        let selected = select_latest_file(&only_alpha, Some("1.12.2")).unwrap();
        assert_eq!(selected.id, 900);

        // Game-version filter: a version with no matches falls back to the
        // full list (existing behaviour) but still prefers the Release.
        let selected = select_latest_file(&files, Some("1.16.5")).unwrap();
        assert_eq!(selected.id, 700);

        // Empty list -> None.
        assert!(select_latest_file(&[], None).is_none());
    }

    #[test]
    fn test_select_latest_file_newest_date_wins_within_same_tier() {
        // Two Releases listed oldest-first: the newer one must win once the
        // release-type tier is equal (date-descending half of the sort).
        let files = vec![
            CfFile {
                id: 100,
                mod_id: 1,
                file_name: Some("Pack v1.0.zip".to_string()),
                download_url: None,
                game_versions: None,
                hashes: None,
                file_length: None,
                dependencies: None,
                class_id: None,
                allow_mod_distribution: None,
                is_available: None,
                file_date: Some("2024-01-01T00:00:00Z".to_string()),
                release_type: Some(1),
            },
            CfFile {
                id: 200,
                mod_id: 1,
                file_name: Some("Pack v1.1.zip".to_string()),
                download_url: None,
                game_versions: None,
                hashes: None,
                file_length: None,
                dependencies: None,
                class_id: None,
                allow_mod_distribution: None,
                is_available: None,
                file_date: Some("2025-01-01T00:00:00Z".to_string()),
                release_type: Some(1),
            },
            // Unparsable date sorts last within the same tier.
            CfFile {
                id: 300,
                mod_id: 1,
                file_name: Some("Pack v1.2.zip".to_string()),
                download_url: None,
                game_versions: None,
                hashes: None,
                file_length: None,
                dependencies: None,
                class_id: None,
                allow_mod_distribution: None,
                is_available: None,
                file_date: Some("not-a-date".to_string()),
                release_type: Some(1),
            },
        ];

        let selected = select_latest_file(&files, None).unwrap();
        assert_eq!(selected.id, 200);
        assert_eq!(selected.file_name.as_deref(), Some("Pack v1.1.zip"));
    }

    #[tokio::test]
    async fn test_live_curseforge_api_request() {
        use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};

        // This test needs a real API key and network access; skip it (rather than
        // fail the build) when the key has not been configured.
        let Ok(api_key) = curseforge_api_key() else {
            println!("Skipping live CurseForge API test: CURSEFORGE_API_KEY not set");
            return;
        };

        let mut req_headers = HeaderMap::new();
        req_headers.insert(
            "x-api-key",
            HeaderValue::from_str(&api_key).unwrap(),
        );
        req_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        req_headers.insert(USER_AGENT, HeaderValue::from_static("ayinlauncher/1.0.0"));

        let test_projects = [
            (925200, "All the Mods 10"),
            (715572, "All the Mods 9"),
            (285109, "RLCraft"),
            (876781, "Better MC [FORGE]"),
            (844844, "Create Flavored"),
        ];

        println!("\n=== ALL 5 CATALOG ENTRIES RESOLUTION CHECK ===");
        for (project_id, expected_name) in test_projects {
            let mod_url = format!("{CURSEFORGE_MOD_URL}/{project_id}");
            let res = REQWEST_CLIENT
                .get(&mod_url)
                .headers(req_headers.clone())
                .send()
                .await;

            if let Ok(resp) = res {
                assert!(resp.status().is_success());
                let body = resp.text().await.unwrap_or_default();
                let parsed: CfModResponse = serde_json::from_str(&body).unwrap();
                // Use the same release-preference logic the installer uses so
                // this printout reflects what would actually be installed
                // (e.g. RLCraft's newer Alpha must not beat its Release).
                let latest_file =
                    select_latest_file(&parsed.data.latest_files, None).unwrap();
                let (hash_type, hash_val) = select_supported_hash(latest_file.hashes.as_deref().unwrap_or(&[])).unwrap_or(("none", "none".to_string()));
                println!("Catalog Name: {}", expected_name);
                println!("API Project ID: {} | Returned Name: {}", parsed.data.id, parsed.data.name);
                println!(
                    "Latest File ID: {} | File Name: {} | Release Type: {}",
                    latest_file.id,
                    latest_file.file_name.as_deref().unwrap_or(""),
                    latest_file
                        .release_type
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                );
                println!("Download URL: {}", latest_file.download_url.as_deref().unwrap_or("<BLOCKED>"));
                println!("Hash ({hash_type}): {hash_val}");
                println!("---");
            }
        }
        println!("===============================================\n");
    }
}
