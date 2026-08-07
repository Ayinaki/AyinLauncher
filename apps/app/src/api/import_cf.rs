use crate::api::Result;
use theseus::pack::curseforge_pack::{
    self, BlockedMod, BlockedModScanResult, CfPackFilesResult,
    CurseForgeCatalogPack, CurseForgeImportResult,
};

pub fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("import-cf")
        .invoke_handler(tauri::generate_handler![
            install_curseforge_catalog_pack,
            check_curseforge_pack_update,
            get_curseforge_pack_files,
            change_curseforge_pack_version,
            scan_downloads_for_blocked_mods,
            get_curseforge_catalog,
        ])
        .build()
}

#[tauri::command]
pub async fn get_curseforge_catalog()
-> Result<Option<Vec<CurseForgeCatalogPack>>> {
    Ok(curseforge_pack::get_curseforge_catalog().await)
}

#[tauri::command]
pub async fn install_curseforge_catalog_pack(
    project_id: u32,
    game_version: Option<String>,
    instance_id: Option<String>,
    file_id: Option<u32>,
) -> Result<CurseForgeImportResult> {
    Ok(curseforge_pack::install_curseforge_catalog_pack(
        project_id,
        game_version,
        instance_id,
        file_id,
    )
    .await?)
}

#[tauri::command]
pub async fn check_curseforge_pack_update(instance_id: String) -> Result<bool> {
    Ok(curseforge_pack::check_curseforge_pack_update(instance_id).await?)
}

#[tauri::command]
pub async fn get_curseforge_pack_files(
    instance_id: String,
) -> Result<CfPackFilesResult> {
    Ok(curseforge_pack::get_curseforge_pack_files(instance_id).await?)
}

#[tauri::command]
pub async fn change_curseforge_pack_version(
    instance_id: String,
    file_id: u32,
) -> Result<CurseForgeImportResult> {
    Ok(
        curseforge_pack::change_curseforge_pack_version(instance_id, file_id)
            .await?,
    )
}

#[tauri::command]
pub async fn scan_downloads_for_blocked_mods(
    instance_id: String,
    blocked_mods: Vec<BlockedMod>,
) -> Result<BlockedModScanResult> {
    Ok(curseforge_pack::scan_downloads_for_blocked_mods(
        instance_id,
        blocked_mods,
    )
    .await?)
}
