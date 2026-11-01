use tauri::{Emitter, Manager};
#[cfg(target_os = "linux")]
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

#[derive(serde::Serialize, Clone)]
pub struct UpdateInfo {
    pub available: bool,
    pub version: Option<String>,
    pub body: Option<String>,
}

#[derive(Default)]
pub(crate) struct UpdateState {
    pub(crate) operation: tokio::sync::Mutex<()>,
}

#[tauri::command]
pub async fn open_update_review(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    anchor: Option<f64>,
) -> Result<(), String> {
    crate::ensure_toolbar_access(&webview)?;
    crate::local_overlay::toggle(&app, "update-review", "update-review.html", anchor).await
}

#[tauri::command]
pub async fn check_update(app: tauri::AppHandle) -> Result<UpdateInfo, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(UpdateInfo {
            available: true,
            version: Some(update.version),
            body: update.body,
        }),
        Ok(None) => Ok(UpdateInfo {
            available: false,
            version: None,
            body: None,
        }),
        Err(e) => Err(e.to_string()),
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateInstallOutcome {
    #[cfg(target_os = "linux")]
    ReleasePage,
    NoUpdate,
    UpdateChanged {
        version: String,
        body: Option<String>,
    },
}

pub(crate) fn changed_update_review(
    expected: &str,
    version: &str,
    body: Option<String>,
) -> Option<UpdateInstallOutcome> {
    (expected != version).then(|| UpdateInstallOutcome::UpdateChanged {
        version: version.to_owned(),
        body,
    })
}

#[tauri::command]
pub async fn install_update(
    app: tauri::AppHandle,
    webview: tauri::Webview,
    expected_version: String,
) -> Result<UpdateInstallOutcome, String> {
    crate::ensure_local_window(&webview, "update-review")?;
    let state = app.state::<UpdateState>();
    let _operation = state
        .operation
        .try_lock()
        .map_err(|_| "Обновление уже устанавливается")?;
    let result = install_reviewed_update(&app, &expected_version).await;
    let _ = app.emit("update-idle", ());
    result
}

async fn install_reviewed_update(
    app: &tauri::AppHandle,
    expected_version: &str,
) -> Result<UpdateInstallOutcome, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    if let Some(update) = update {
        if let Some(changed) =
            changed_update_review(expected_version, &update.version, update.body.clone())
        {
            return Ok(changed);
        }
        #[cfg(target_os = "linux")]
        if std::env::var_os("APPIMAGE").is_none() {
            app.opener()
                .open_url(
                    "https://github.com/AXELL2022/Anivox-Wraper/releases/latest",
                    None::<&str>,
                )
                .map_err(|err| err.to_string())?;
            return Ok(UpdateInstallOutcome::ReleasePage);
        }
        let _ = app.emit("update-started", ());
        let mut downloaded: usize = 0;
        let app_progress = app.clone();
        let app_finished = app.clone();
        update
            .download_and_install(
                move |chunk_length, content_length| {
                    downloaded += chunk_length;
                    let percent = content_length
                        .map(|total| {
                            if total > 0 {
                                ((downloaded as f64 / total as f64) * 100.0).min(100.0) as u32
                            } else {
                                0
                            }
                        })
                        .unwrap_or(0);

                    let _ = app_progress.emit(
                        "update-progress",
                        serde_json::json!({
                            "downloaded": downloaded,
                            "total": content_length,
                            "percent": percent
                        }),
                    );
                },
                move || {
                    let _ = app_finished.emit("update-finished", ());
                },
            )
            .await
            .map_err(|e| e.to_string())?;

        app.restart();
    }
    Ok(UpdateInstallOutcome::NoUpdate)
}
