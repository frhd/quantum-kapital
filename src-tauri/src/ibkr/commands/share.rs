//! Save the trade-review share-card image to disk via the native save
//! dialog. The frontend renders the card to PNG bytes via `html-to-image`
//! and hands them to this command — the dialog + filesystem write live
//! on the Rust side because Tauri webviews intercept `<a download>` and
//! the JS clipboard API rejects image MIME types.

use std::path::Path;

use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, FilePath};
use tokio::sync::oneshot;

#[tauri::command]
pub async fn save_share_image_png(
    app: AppHandle,
    date: String,
    bytes: Vec<u8>,
) -> Result<Option<String>, String> {
    let (tx, rx) = oneshot::channel::<Option<FilePath>>();

    app.dialog()
        .file()
        .set_title("Save trade review")
        .set_file_name(format!("trade-review-{date}.png"))
        .add_filter("PNG", &["png"])
        .save_file(move |path| {
            let _ = tx.send(path);
        });

    let chosen = rx
        .await
        .map_err(|e| format!("dialog channel closed: {e}"))?;
    let Some(file_path) = chosen else {
        return Ok(None);
    };

    let local = file_path
        .into_path()
        .map_err(|e| format!("path resolution failed: {e}"))?;

    write_png_bytes(&local, &bytes).await?;

    Ok(Some(local.to_string_lossy().into_owned()))
}

async fn write_png_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    tokio::fs::write(path, bytes)
        .await
        .map_err(|e| format!("write failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn writes_bytes_to_target_path() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let target = tmp.path().join("trade-review.png");
        write_png_bytes(&target, &[0x89, b'P', b'N', b'G'])
            .await
            .expect("write");
        let got = std::fs::read(&target).expect("read");
        assert_eq!(got, vec![0x89, b'P', b'N', b'G']);
    }

    #[tokio::test]
    async fn write_png_bytes_returns_error_on_invalid_path() {
        let bad = Path::new("/nonexistent-dir-xyz/no/such/place/file.png");
        let err = write_png_bytes(bad, &[0u8]).await.expect_err("must fail");
        assert!(err.starts_with("write failed:"), "got: {err}");
    }
}
