use std::fs;
use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

use crate::crypto::SessionCrypto;
use crate::data::AppData;
use crate::{APP_NAME, DATA_FILE_NAME};

pub fn data_path() -> PathBuf {
    app_data_base().join(APP_NAME).join(DATA_FILE_NAME)
}

fn app_data_base() -> PathBuf {
    dirs::data_local_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
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
        if name.starts_with("cofferly-") && name.ends_with(".html") {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::default_app_data;
    use tempfile::tempdir;

    #[test]
    fn stores_current_data_in_generic_file_name() {
        assert_eq!(DATA_FILE_NAME, "data.json");
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
