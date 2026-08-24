use std::fs;
use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

use crate::crypto::SessionCrypto;
use crate::data::AppData;
use crate::{APP_NAME, DATA_FILE_NAME, PREVIOUS_DATA_FILE_NAME};

#[derive(Debug, Default)]
pub struct DataVaultPreparation {
    /// The previous encrypted file is deliberately retained until the user has
    /// unlocked and verified the new vault on their own machine.
    pub preserved_previous_file: Option<PathBuf>,
}

pub fn data_path() -> PathBuf {
    app_data_base().join(APP_NAME).join(DATA_FILE_NAME)
}

fn previous_data_path() -> PathBuf {
    app_data_base().join(APP_NAME).join(PREVIOUS_DATA_FILE_NAME)
}

fn app_data_base() -> PathBuf {
    // Maintainer / screenshot isolation — not used in normal family installs.
    if let Ok(dir) = std::env::var("COFFERLY_DATA_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    dirs::data_local_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Write vault bytes atomically (also used by the screenshot capture helper).
pub fn save_encrypted_bytes(path: &Path, contents: &[u8]) -> Result<(), String> {
    write_atomically(path, contents)
}

/// Prepare the current vault path without risking the previous encrypted file.
///
/// When only `data.json` exists, its encrypted bytes are copied atomically to
/// `vault.cofferly` and verified byte-for-byte. The source is never deleted. If
/// both files exist, the current vault always wins and neither file is changed.
pub fn prepare_data_vault() -> Result<DataVaultPreparation, String> {
    prepare_data_vault_at(&data_path(), &previous_data_path())
}

fn prepare_data_vault_at(
    current_path: &Path,
    previous_path: &Path,
) -> Result<DataVaultPreparation, String> {
    if current_path.exists() {
        return Ok(DataVaultPreparation::default());
    }

    let Some(previous_bytes) = load_raw(previous_path)? else {
        return Ok(DataVaultPreparation::default());
    };

    if !crate::crypto::is_current_format(&previous_bytes) {
        return Err(format!(
            "{} is not a supported encrypted Cofferly file. It was left untouched and no new vault was created",
            previous_path.display()
        ));
    }

    write_new_atomically(current_path, &previous_bytes).map_err(|err| {
        format!(
            "Could not copy {} to {}: {err}. The original file was left untouched",
            previous_path.display(),
            current_path.display()
        )
    })?;

    let copied_bytes = load_raw(current_path)?
        .ok_or_else(|| format!("Copied vault is missing at {}", current_path.display()))?;
    if copied_bytes != previous_bytes {
        return Err(format!(
            "Copied vault verification failed at {}. The original file remains untouched",
            current_path.display()
        ));
    }

    Ok(DataVaultPreparation {
        preserved_previous_file: Some(previous_path.to_path_buf()),
    })
}

pub fn load_raw(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("Could not read {}: {err}", path.display())),
    }
}

/// Encrypt and write `data`, returning the ciphertext so callers can cache it
/// without a redundant disk read.
pub fn save_encrypted(
    path: &Path,
    data: &AppData,
    pin: &str,
    session: &mut Option<SessionCrypto>,
) -> Result<Vec<u8>, String> {
    let json = Zeroizing::new(
        serde_json::to_vec(data).map_err(|err| format!("Failed to serialize data: {err}"))?,
    );
    let encrypted = crate::crypto::encrypt(&json, pin, session)?;

    write_atomically(path, &encrypted)?;
    Ok(encrypted)
}

fn write_atomically(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let parent = path
        .parent()
        .ok_or_else(|| format!("Could not find parent folder for {}", path.display()))?;
    let mut temp_file = tempfile::NamedTempFile::new_in(parent).map_err(|err| err.to_string())?;
    temp_file
        .write_all(contents)
        .map_err(|err| err.to_string())?;
    temp_file
        .as_file_mut()
        .sync_all()
        .map_err(|err| err.to_string())?;
    temp_file
        .persist(path)
        .map_err(|err| err.error.to_string())?;

    Ok(())
}

/// Create a new file atomically without replacing a file another process may
/// have created after our existence check.
fn write_new_atomically(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let parent = path
        .parent()
        .ok_or_else(|| format!("Could not find parent folder for {}", path.display()))?;
    let mut temp_file = tempfile::NamedTempFile::new_in(parent).map_err(|err| err.to_string())?;
    temp_file
        .write_all(contents)
        .map_err(|err| err.to_string())?;
    temp_file
        .as_file_mut()
        .sync_all()
        .map_err(|err| err.to_string())?;
    temp_file
        .persist_noclobber(path)
        .map_err(|err| err.error.to_string())?;

    Ok(())
}

/// Reserves a fresh, unpredictably-named temp file for family data (ledger
/// exports, the recovery card). Created with owner-only permissions (0600 on
/// Unix by default via `tempfile`) and a random suffix so a shared `/tmp`
/// cannot expose the contents to other local users or a pre-planted symlink.
/// Returns the persisted path; the caller writes its content and is
/// responsible for deleting it promptly (e.g. on lock/exit) rather than
/// leaving it for the next launch's best-effort cleanup.
pub fn reserve_private_temp_path(stem: &str, ext: &str) -> Result<PathBuf, String> {
    let temp_file = tempfile::Builder::new()
        .prefix(&format!("cofferly-{stem}-"))
        .suffix(&format!(".{ext}"))
        .rand_bytes(12)
        .tempfile_in(std::env::temp_dir())
        .map_err(|err| err.to_string())?;

    let (_file, path) = temp_file.keep().map_err(|err| err.error.to_string())?;
    Ok(path)
}

/// Best-effort cleanup of previous print artifacts under the OS temp directory.
pub fn cleanup_temp_print_artifacts() {
    let temp = std::env::temp_dir();
    let Ok(entries) = fs::read_dir(&temp) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with("cofferly-") && (name.ends_with(".html") || name.ends_with(".csv")) {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::default_app_data;
    use tempfile::tempdir;

    fn encrypted_fixture(pin: &str) -> Vec<u8> {
        let serialized = serde_json::to_vec(&default_app_data()).unwrap();
        let mut session = None;
        crate::crypto::encrypt(&serialized, pin, &mut session).unwrap()
    }

    #[test]
    fn stores_current_data_in_vault_file() {
        assert_eq!(DATA_FILE_NAME, "vault.cofferly");
        assert_eq!(PREVIOUS_DATA_FILE_NAME, "data.json");
    }

    #[test]
    fn copies_previous_encrypted_file_and_preserves_the_backup() {
        let test_dir = tempdir().unwrap();
        let previous_path = test_dir.path().join(PREVIOUS_DATA_FILE_NAME);
        let current_path = test_dir.path().join(DATA_FILE_NAME);
        let previous_bytes = encrypted_fixture("2468");
        fs::write(&previous_path, &previous_bytes).unwrap();

        let preparation = prepare_data_vault_at(&current_path, &previous_path).unwrap();

        assert_eq!(
            preparation.preserved_previous_file.as_deref(),
            Some(previous_path.as_path())
        );
        assert_eq!(fs::read(&current_path).unwrap(), previous_bytes);
        assert_eq!(fs::read(&previous_path).unwrap(), previous_bytes);
        let (plaintext, _) = crate::crypto::decrypt(&previous_bytes, "2468").unwrap();
        let copied_data: AppData = serde_json::from_slice(&plaintext).unwrap();
        assert_eq!(copied_data.wallets.len(), default_app_data().wallets.len());
        assert_eq!(copied_data.wallets[0].child_name, "Child 1");
    }

    #[test]
    fn existing_current_vault_takes_precedence_without_touching_either_file() {
        let test_dir = tempdir().unwrap();
        let previous_path = test_dir.path().join(PREVIOUS_DATA_FILE_NAME);
        let current_path = test_dir.path().join(DATA_FILE_NAME);
        let previous_bytes = encrypted_fixture("1111");
        let current_bytes = encrypted_fixture("2222");
        fs::write(&previous_path, &previous_bytes).unwrap();
        fs::write(&current_path, &current_bytes).unwrap();

        let preparation = prepare_data_vault_at(&current_path, &previous_path).unwrap();

        assert!(preparation.preserved_previous_file.is_none());
        assert_eq!(fs::read(&current_path).unwrap(), current_bytes);
        assert_eq!(fs::read(&previous_path).unwrap(), previous_bytes);
    }

    #[test]
    fn unsupported_previous_file_is_left_untouched_without_creating_a_vault() {
        let test_dir = tempdir().unwrap();
        let previous_path = test_dir.path().join(PREVIOUS_DATA_FILE_NAME);
        let current_path = test_dir.path().join(DATA_FILE_NAME);
        let unsupported = br#"{"wallets":[]}"#;
        fs::write(&previous_path, unsupported).unwrap();

        let error = prepare_data_vault_at(&current_path, &previous_path).unwrap_err();

        assert!(error.contains("not a supported encrypted Cofferly file"));
        assert!(!current_path.exists());
        assert_eq!(fs::read(&previous_path).unwrap(), unsupported);
    }

    #[test]
    fn no_existing_files_is_a_clean_first_run() {
        let test_dir = tempdir().unwrap();
        let previous_path = test_dir.path().join(PREVIOUS_DATA_FILE_NAME);
        let current_path = test_dir.path().join(DATA_FILE_NAME);

        let preparation = prepare_data_vault_at(&current_path, &previous_path).unwrap();

        assert!(preparation.preserved_previous_file.is_none());
        assert!(!current_path.exists());
    }

    #[test]
    fn load_raw_returns_none_for_a_missing_file() {
        let test_dir = tempdir().unwrap();
        let path = test_dir.path().join(APP_NAME).join(DATA_FILE_NAME);

        assert_eq!(load_raw(&path).unwrap(), None);
    }

    #[test]
    fn save_encrypted_replaces_existing_file() {
        let test_dir = tempdir().unwrap();
        let path = test_dir.path().join(APP_NAME).join(DATA_FILE_NAME);
        let mut data = default_app_data();
        let pin = "1234";
        let mut session = None;

        save_encrypted(&path, &data, pin, &mut session).unwrap();
        let first_raw = load_raw(&path).unwrap().unwrap();

        data.wallets[0].child_name = "Encrypted Child".to_owned();
        save_encrypted(&path, &data, pin, &mut session).unwrap();
        let second_raw = load_raw(&path).unwrap().unwrap();
        let (decrypted, _) = crate::crypto::decrypt(&second_raw, pin).unwrap();
        let loaded = serde_json::from_slice::<AppData>(&decrypted).unwrap();

        assert_ne!(first_raw, second_raw);
        assert_eq!(loaded.wallets[0].child_name, "Encrypted Child");
        // Second save should not need a new Argon2 wrap of a different key — same session.
        let header_len = 1 + 16 + 24 + 48;
        assert_eq!(&first_raw[..header_len], &second_raw[..header_len]);
    }

    #[test]
    fn write_atomically_replaces_existing_bytes() {
        let test_dir = tempdir().unwrap();
        let path = test_dir.path().join("nested").join("data.bin");

        write_atomically(&path, b"first").unwrap();
        write_atomically(&path, b"second").unwrap();

        assert_eq!(load_raw(&path).unwrap().unwrap(), b"second");
    }
}
