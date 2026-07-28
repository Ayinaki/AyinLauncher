use std::path::PathBuf;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_opener::OpenerExt;
use serde::{Deserialize, Serialize};
use async_zip::tokio::read::fs::ZipFileReader;
use tokio::fs;
use futures_util::io::AsyncReadExt;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CFManifest {
    pub minecraft: CFMinecraft,
    pub name: String,
    pub version: String,
    pub author: Option<String>,
    pub files: Vec<CFFile>,
    pub overrides: Option<String>,
    pub image: Option<String>,
    pub logo_url: Option<String>,
    pub icon_url: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CFMinecraft {
    pub version: String,
    pub mod_loaders: Vec<CFModLoader>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CFModLoader {
    pub id: String,
    pub primary: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CFFile {
    #[serde(rename = "projectID")]
    pub project_id: u32,
    #[serde(rename = "fileID")]
    pub file_id: u32,
    #[serde(default)]
    pub required: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BlockedMod {
    pub project_id: u32,
    pub file_id: u32,
    pub expected_file_name: String,
    pub expected_size: Option<u64>,
    pub hash: Option<String>,
    pub page_url: String,
    pub class_id: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CFImportResult {
    pub staging_dir: String,
    pub blocked_mods: Vec<BlockedMod>,
    pub name: String,
    pub game_version: String,
    pub loader: String,
    pub loader_version: String,
    pub icon_url: Option<String>,
}

#[tauri::command]
pub async fn import_curseforge_modpack<R: Runtime>(app: AppHandle<R>, zip_path: String) -> Result<CFImportResult, String> {
    let file_path = PathBuf::from(zip_path);
    
    let reader = ZipFileReader::new(&file_path).await.map_err(|e| e.to_string())?;

    // Find and parse manifest.json
    let mut manifest_index = None;
    for (i, file) in reader.file().entries().iter().enumerate() {
        if file.filename().as_str().map_err(|_| "Invalid filename")? == "manifest.json" {
            manifest_index = Some(i);
            break;
        }
    }
    
    let manifest_index = manifest_index.ok_or("manifest.json not found in zip")?;
    let mut entry_reader = reader.reader_with_entry(manifest_index).await.map_err(|e| e.to_string())?;
    let mut manifest_str = String::new();
    entry_reader.read_to_string(&mut manifest_str).await.map_err(|e| e.to_string())?;

    let manifest: CFManifest = serde_json::from_str(&manifest_str).map_err(|e| e.to_string())?;

    let staging_dir = app.path().temp_dir().unwrap_or_else(|_| PathBuf::from("/tmp")).join(format!("cf_import_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&staging_dir).await.map_err(|e| e.to_string())?;
    let mods_dir = staging_dir.join("mods");
    let resourcepacks_dir = staging_dir.join("resourcepacks");
    let shaderpacks_dir = staging_dir.join("shaderpacks");
    fs::create_dir_all(&mods_dir).await.map_err(|e| e.to_string())?;

    // Extract overrides folder if present
    let reader = ZipFileReader::new(&file_path).await.map_err(|e| e.to_string())?;
    let overrides_folder = manifest.overrides.as_deref().unwrap_or("overrides");
    for i in 0..reader.file().entries().len() {
        let entry = &reader.file().entries()[i];
        let filename = entry.filename().as_str().map_err(|e| e.to_string())?.to_string();
        if filename.starts_with(overrides_folder) {
            let relative_path = filename.strip_prefix(&format!("{}/", overrides_folder)).unwrap();
            if relative_path.is_empty() || relative_path.ends_with('/') {
                continue;
            }
            let out_path = staging_dir.join(relative_path);
            if let Some(p) = out_path.parent() {
                fs::create_dir_all(p).await.map_err(|e| e.to_string())?;
            }
            let mut entry_reader = reader.reader_with_entry(i).await.map_err(|e| e.to_string())?;
            let mut buf = Vec::new();
            entry_reader.read_to_end(&mut buf).await.map_err(|e| e.to_string())?;
            fs::write(&out_path, buf).await.map_err(|e| e.to_string())?;
        }
    }

    let mut blocked_mods = Vec::new();
    let client = &theseus::REQWEST_CLIENT;
    
    // Note: obfstr only deters casual static inspection (e.g. strings, hex editors) and does not protect against a determined attacker using a debugger to dump decrypted memory at runtime.
    let cf_api_key = obfstr::obfstr!("$2a$10$YfBPFQH4RVhhIXNu0HhmLOraAXETTh6D4nBo5FNegJju8at.Xh8Py").to_string();

    // Resolve CF API URLs & identify blocked mods
    for cf_file in manifest.files {
        if !cf_file.required { continue; }
        
        let url = format!("https://api.curseforge.com/v1/mods/{}/files/{}", cf_file.project_id, cf_file.file_id);
        let resp = client.get(&url).header("x-api-key", &cf_api_key).send().await.map_err(|e| e.to_string())?;
        
        if resp.status().as_u16() == 401 || resp.status().as_u16() == 403 {
            return Err(format!("CurseForge API authentication failed ({}). The embedded API key may be invalid or rate-limited.", resp.status()));
        }

        if resp.status().is_success() {
            let file_data: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
            let download_url = file_data["data"]["downloadUrl"].as_str();
            let file_name = file_data["data"]["fileName"].as_str().unwrap_or("Unknown").to_string();
            
            let allow_distrib = file_data["data"]["allowModDistribution"].as_bool();
            let is_available = file_data["data"]["isAvailable"].as_bool();
            
            let mut page_url = format!("https://www.curseforge.com/projects/{}/files/{}", cf_file.project_id, cf_file.file_id);
            let mut class_id: Option<u64> = None;

            let mod_api_url = format!("https://api.curseforge.com/v1/mods/{}", cf_file.project_id);
            if let Ok(mod_resp) = client.get(&mod_api_url).header("x-api-key", &cf_api_key).send().await {
                if mod_resp.status().is_success() {
                    if let Ok(mod_data) = mod_resp.json::<serde_json::Value>().await {
                        class_id = mod_data["data"]["classId"].as_u64();
                        if let Some(website_url) = mod_data["data"]["links"]["websiteUrl"].as_str() {
                            page_url = format!("{}/download/{}", website_url.trim_end_matches('/'), cf_file.file_id);
                        } else if let Some(slug) = mod_data["data"]["slug"].as_str() {
                            page_url = format!("https://www.curseforge.com/minecraft/mc-mods/{}/download/{}", slug, cf_file.file_id);
                        }
                    }
                }
            }

            if download_url.is_none() || allow_distrib == Some(false) || is_available == Some(false) {
                blocked_mods.push(BlockedMod {
                    project_id: cf_file.project_id,
                    file_id: cf_file.file_id,
                    expected_file_name: file_name,
                    expected_size: file_data["data"]["fileLength"].as_u64(),
                    hash: file_data["data"]["hashes"][0]["value"].as_str().map(|s| s.to_string()),
                    page_url,
                    class_id,
                });
            } else if let Some(url) = download_url {
                let target_dir = if class_id == Some(12) || (file_name.ends_with(".zip") && class_id != Some(6) && class_id != Some(6552)) {
                    fs::create_dir_all(&resourcepacks_dir).await.map_err(|e| e.to_string())?;
                    &resourcepacks_dir
                } else if class_id == Some(6552) {
                    fs::create_dir_all(&shaderpacks_dir).await.map_err(|e| e.to_string())?;
                    &shaderpacks_dir
                } else {
                    &mods_dir
                };

                let mod_resp = client.get(url).send().await.map_err(|e| e.to_string())?;
                if mod_resp.status().is_success() {
                    let bytes = mod_resp.bytes().await.map_err(|e| e.to_string())?;
                    fs::write(target_dir.join(file_name), bytes).await.map_err(|e| e.to_string())?;
                }
            }
        }
    }
    
    let mut loader = "vanilla".to_string();
    let mut loader_version = "".to_string();

    if let Some(primary_loader) = manifest.minecraft.mod_loaders.iter().find(|l| l.primary) {
        let id = &primary_loader.id;
        if id.starts_with("forge-") {
            loader = "forge".to_string();
            loader_version = id.strip_prefix("forge-").unwrap_or("").to_string();
        } else if id.starts_with("fabric-") {
            loader = "fabric".to_string();
            loader_version = id.strip_prefix("fabric-").unwrap_or("").to_string();
        } else if id.starts_with("quilt-") {
            loader = "quilt".to_string();
            loader_version = id.strip_prefix("quilt-").unwrap_or("").to_string();
        } else if id.starts_with("neoforge-") {
            loader = "neoforge".to_string();
            loader_version = id.strip_prefix("neoforge-").unwrap_or("").to_string();
        } else {
            loader = id.to_string();
        }
    }

    Ok(CFImportResult {
        staging_dir: staging_dir.to_string_lossy().to_string(),
        blocked_mods,
        name: manifest.name,
        game_version: manifest.minecraft.version,
        loader,
        loader_version,
        icon_url: manifest.image.or(manifest.logo_url).or(manifest.icon_url),
    })
}

#[tauri::command]
pub async fn scan_folder_for_mods<R: Runtime>(app: AppHandle<R>, staging_dir: String, folder_path: Option<String>, expected: Vec<BlockedMod>) -> Result<Vec<String>, String> {
    let staging_path = PathBuf::from(&staging_dir);
    let mods_dir = staging_path.join("mods");
    let resourcepacks_dir = staging_path.join("resourcepacks");
    let shaderpacks_dir = staging_path.join("shaderpacks");
    fs::create_dir_all(&mods_dir).await.map_err(|e| e.to_string())?;
    
    let scan_dir = if let Some(ref path_str) = folder_path {
        if !path_str.trim().is_empty() {
            PathBuf::from(path_str)
        } else {
            app.path().download_dir().unwrap_or_default()
        }
    } else {
        app.path().download_dir().unwrap_or_default()
    };
    
    let mut found = Vec::new();
    if !scan_dir.exists() {
        return Ok(found);
    }
    
    let mut entries = fs::read_dir(&scan_dir).await.map_err(|e| e.to_string())?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let metadata = entry.metadata().await.map_err(|e| e.to_string())?;
        if metadata.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            
            for req in &expected {
                if found.contains(&req.expected_file_name) {
                    continue;
                }
                
                let base_expected = req.expected_file_name
                    .trim_end_matches(".jar")
                    .trim_end_matches(".zip");
                let is_match = name == req.expected_file_name 
                    || (base_expected.len() >= 3 && name.starts_with(base_expected));
                
                if is_match {
                    let target_dir = if req.class_id == Some(12) || (req.expected_file_name.ends_with(".zip") && req.class_id != Some(6) && req.class_id != Some(6552)) {
                        fs::create_dir_all(&resourcepacks_dir).await.map_err(|e| e.to_string())?;
                        &resourcepacks_dir
                    } else if req.class_id == Some(6552) {
                        fs::create_dir_all(&shaderpacks_dir).await.map_err(|e| e.to_string())?;
                        &shaderpacks_dir
                    } else {
                        &mods_dir
                    };

                    let dest = target_dir.join(&req.expected_file_name);
                    if let Ok(_) = fs::copy(entry.path(), &dest).await {
                        found.push(req.expected_file_name.clone());
                        break;
                    }
                }
            }
        }
    }
    
    Ok(found)
}

#[tauri::command]
pub fn open_cf_urls<R: Runtime>(app: AppHandle<R>, urls: Vec<String>) -> Result<(), String> {
    for url in urls {
        app.opener().open_url(&url, None::<String>).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::<R>::new("import-cf")
        .invoke_handler(tauri::generate_handler![
            import_curseforge_modpack,
            scan_folder_for_mods,
            open_cf_urls,
        ])
        .build()
}
