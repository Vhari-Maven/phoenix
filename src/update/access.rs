//! Pre-flight access checks for game updates.

use anyhow::Result;
use std::path::Path;

use crate::app_data::game_config;

/// Check if we have write access to the game installation before updating.
///
/// This prevents update failures when:
/// - The game is running
/// - Files are open in another program (e.g., JSON file open in editor)
/// - Antivirus is scanning files
///
/// Returns Ok(()) if we can proceed, or an error explaining why not.
pub async fn check_installation_access(game_dir: &Path) -> Result<()> {
    // Check if game executables can be renamed (indicates they're not in use)
    for exe_name in &game_config().executables.names {
        let exe_path = game_dir.join(exe_name);
        if exe_path.exists() {
            // Try to open with exclusive write access
            // On Windows, this fails if the file is running or locked
            match std::fs::OpenOptions::new().write(true).open(&exe_path) {
                Ok(_) => {
                    // File opened successfully, we have access
                    tracing::debug!("Access check passed for {}", exe_name);
                }
                Err(e) => {
                    // Common Windows error codes:
                    // - 32: ERROR_SHARING_VIOLATION (file in use)
                    // - 5: ERROR_ACCESS_DENIED
                    let hint = if e.raw_os_error() == Some(32) {
                        "The game appears to be running. Please close it before updating."
                    } else if e.raw_os_error() == Some(5) {
                        "Access denied. Try running the launcher as administrator, or check if antivirus is blocking access."
                    } else {
                        "The file may be in use by another program."
                    };
                    anyhow::bail!(
                        "Cannot update: {} is locked.\n\n{}\n\nError: {}",
                        exe_name,
                        hint,
                        e
                    );
                }
            }
        }
    }

    // Quick check that we can write to the game directory
    let test_file = game_dir.join(".phoenix_write_test");
    match tokio::fs::write(&test_file, b"test").await {
        Ok(_) => {
            let _ = tokio::fs::remove_file(&test_file).await;
        }
        Err(e) => {
            anyhow::bail!(
                "Cannot write to game directory.\n\nPlease check folder permissions.\n\nError: {}",
                e
            );
        }
    }

    Ok(())
}
