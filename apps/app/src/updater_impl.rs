use crate::api::Result;
use futures_util::StreamExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::http::HeaderValue;
use tauri::http::header::ACCEPT;
use tauri::{Manager, ResourceId, Runtime, Webview};
use tauri_plugin_http::reqwest;
use tauri_plugin_http::reqwest::ClientBuilder;
use tauri_plugin_updater::Error;
use tauri_plugin_updater::Update;
use theseus::{
    LoadingBarType, emit_loading, init_loading, launcher_user_agent,
};
use tokio::io::AsyncWriteExt;
use tokio::time::Instant;

#[derive(Clone)]
pub struct SharedHttpClient(pub reqwest::Client);

impl Default for SharedHttpClient {
    fn default() -> Self {
        let client = ClientBuilder::new()
            .user_agent(launcher_user_agent())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self(client)
    }
}

#[derive(Default)]
pub struct PendingUpdateData(pub Mutex<Option<(Arc<Update>, PathBuf)>>);

// Reimplementation of Update::download mostly, minus the actual download part
#[tauri::command]
pub async fn get_update_size<R: Runtime>(
    webview: Webview<R>,
    rid: ResourceId,
) -> Result<Option<u64>> {
    let update = webview.resources_table().get::<Update>(rid)?;

    let mut headers = update.headers.clone();
    if !headers.contains_key(ACCEPT) {
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/octet-stream"),
        );
    }

    let client = webview.state::<SharedHttpClient>().inner().0.clone();
    let mut request = client.head(update.download_url.clone()).headers(headers);
    if let Some(timeout) = update.timeout {
        request = request.timeout(timeout);
    }
    let response = request.send().await?;

    if !response.status().is_success() {
        return Err(Error::Network(format!(
            "Download request failed with status: {}",
            response.status()
        ))
        .into());
    }

    let content_length = response
        .headers()
        .get("Content-Length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok());

    Ok(content_length)
}

#[tauri::command]
pub async fn enqueue_update_for_installation<R: Runtime>(
    webview: Webview<R>,
    rid: ResourceId,
) -> Result<()> {
    let pending_data = webview.state::<PendingUpdateData>().inner();

    let update = webview.resources_table().get::<Update>(rid)?;

    let progress = init_loading(
        LoadingBarType::LauncherUpdate {
            version: update.version.clone(),
            current_version: update.current_version.clone(),
        },
        1.0,
        "Downloading update...",
    )
    .await?;

    let download_start = Instant::now();

    let client = webview.state::<SharedHttpClient>().inner().0.clone();

    let mut headers = update.headers.clone();
    if !headers.contains_key(ACCEPT) {
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/octet-stream"),
        );
    }

    let mut req_builder =
        client.get(update.download_url.clone()).headers(headers);
    if let Some(timeout) = update.timeout {
        req_builder = req_builder.timeout(timeout);
    }

    let response = req_builder.send().await?;

    if !response.status().is_success() {
        return Err(Error::Network(format!(
            "Download request failed with status: {}",
            response.status()
        ))
        .into());
    }

    let total_size = response
        .headers()
        .get("Content-Length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    let temp_dir = std::env::temp_dir();
    let temp_file_path =
        temp_dir.join(format!("ayin_update_{}.tmp", uuid::Uuid::new_v4()));

    let mut file = tokio::fs::File::create(&temp_file_path).await?;
    let mut stream = response.bytes_stream();
    let mut downloaded_bytes: u64 = 0;

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|e| Error::Network(e.to_string()))?;
        file.write_all(&chunk).await?;
        downloaded_bytes += chunk.len() as u64;

        if let Some(total) = total_size
            && let Err(e) = emit_loading(
                &progress,
                downloaded_bytes as f64 / total as f64,
                None,
            )
        {
            tracing::error!("Failed to update download progress bar: {e}");
        }
    }

    file.flush().await?;

    let download_duration = download_start.elapsed();
    tracing::info!(
        "Downloaded update to {temp_file_path:?} in {download_duration:?}"
    );

    pending_data
        .0
        .lock()
        .unwrap()
        .replace((update, temp_file_path));

    Ok(())
}

#[tauri::command]
pub fn remove_enqueued_update<R: Runtime>(webview: Webview<R>) {
    let pending_data = webview.state::<PendingUpdateData>().inner();
    if let Some((_, temp_path)) = pending_data.0.lock().unwrap().take() {
        tauri::async_runtime::spawn(async move {
            let _ = tokio::fs::remove_file(temp_path).await;
        });
    }
}
