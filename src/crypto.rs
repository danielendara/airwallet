use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305,
};
use rand::{rngs::SysRng, TryRng};
use zeroize::Zeroizing;

/// Envelope format: `[version=2][salt 16][wrap_nonce 24][wrapped_key][payload_nonce 24][ciphertext]`.
/// The PIN-derived key only wraps a random data key; the ledger is encrypted with that
/// data key so saves after unlock do not re-run Argon2id.
pub const ENCRYPTED_VERSION: u8 = 2;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;
/// XChaCha20-Poly1305 ciphertext overhead for a 32-byte key.
const WRAPPED_KEY_LEN: usize = KEY_LEN + 16;

/// Cached per unlock so saves avoid Argon2id. Dropped on lock / wrong PIN.
#[derive(Debug)]
pub struct SessionCrypto {
    data_key: Zeroizing<[u8; KEY_LEN]>,
    /// Salt + wrapped data key under the current PIN. Reused across saves with
    /// the same PIN so encryption never re-derives the PIN key.
    envelope: EnvelopeHeader,
}

#[derive(Clone, Debug)]
struct EnvelopeHeader {
    salt: [u8; SALT_LEN],
    wrap_nonce: [u8; NONCE_LEN],
    wrapped_key: [u8; WRAPPED_KEY_LEN],
}

/// Derives a 32-byte key from the PIN using Argon2id.
/// The salt must be unique per file (stored alongside the ciphertext).
/// The returned key is zeroized when dropped so it does not linger in memory.
fn derive_key(pin: &str, salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>, String> {
    if salt.len() != SALT_LEN {
        return Err("invalid salt length".to_string());
    }

    // Reasonable parameters for a desktop app:
    // 64 MiB memory, 3 iterations, 1 parallelism.
    let params =
        Params::new(64 * 1024, 3, 1, Some(KEY_LEN)).map_err(|e| format!("argon2 params: {e}"))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon2
        .hash_password_into(pin.as_bytes(), salt, key.as_mut())
        .map_err(|e| format!("key derivation failed: {e}"))?;

    Ok(key)
}

fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut rng = SysRng;
    let mut bytes = [0u8; N];
    rng.try_fill_bytes(&mut bytes)
        .map_err(|e| format!("random generation failed: {e}"))?;
    Ok(bytes)
}

fn encrypt_with_key(
    key: &[u8; KEY_LEN],
    plaintext: &[u8],
) -> Result<([u8; NONCE_LEN], Vec<u8>), String> {
    let nonce = random_bytes::<NONCE_LEN>()?;
    let cipher = XChaCha20Poly1305::new(key.into());
    let ciphertext = cipher
        .encrypt((&nonce).into(), plaintext)
        .map_err(|e| format!("encryption failed: {e}"))?;
    Ok((nonce, ciphertext))
}

fn decrypt_with_key(
    key: &[u8; KEY_LEN],
    nonce: &[u8; NONCE_LEN],
    ciphertext: &[u8],
) -> Result<Zeroizing<Vec<u8>>, String> {
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(nonce.into(), ciphertext)
        .map(Zeroizing::new)
        .map_err(|_| "decryption failed — wrong PIN or data has been tampered with".to_string())
}

fn wrap_data_key(
    pin_key: &[u8; KEY_LEN],
    data_key: &[u8; KEY_LEN],
) -> Result<([u8; NONCE_LEN], [u8; WRAPPED_KEY_LEN]), String> {
    let (nonce, ciphertext) = encrypt_with_key(pin_key, data_key)?;
    let wrapped: [u8; WRAPPED_KEY_LEN] = ciphertext
        .try_into()
        .map_err(|_| "unexpected wrapped key length".to_string())?;
    Ok((nonce, wrapped))
}

fn unwrap_data_key(
    pin_key: &[u8; KEY_LEN],
    wrap_nonce: &[u8; NONCE_LEN],
    wrapped_key: &[u8],
) -> Result<Zeroizing<[u8; KEY_LEN]>, String> {
    let plain = decrypt_with_key(pin_key, wrap_nonce, wrapped_key)?;
    if plain.len() != KEY_LEN {
        return Err("invalid wrapped data key".to_string());
    }
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    key.copy_from_slice(&plain);
    Ok(key)
}

fn build_envelope_header(pin: &str, data_key: &[u8; KEY_LEN]) -> Result<EnvelopeHeader, String> {
    let salt = random_bytes::<SALT_LEN>()?;
    let pin_key = derive_key(pin, &salt)?;
    let (wrap_nonce, wrapped_key) = wrap_data_key(&pin_key, data_key)?;
    Ok(EnvelopeHeader {
        salt,
        wrap_nonce,
        wrapped_key,
    })
}

impl SessionCrypto {
    /// Fresh session: random data key wrapped under the PIN (runs Argon2id once).
    pub fn establish(pin: &str) -> Result<Self, String> {
        let data_key = Zeroizing::new(random_bytes::<KEY_LEN>()?);
        let envelope = build_envelope_header(pin, &data_key)?;
        Ok(Self { data_key, envelope })
    }

    /// Re-wrap the existing data key under a new PIN (PIN change). Runs Argon2id once.
    pub fn rewrap_for_pin(&mut self, pin: &str) -> Result<(), String> {
        self.envelope = build_envelope_header(pin, &self.data_key)?;
        Ok(())
    }

    fn encrypt_payload(&self, plaintext: &[u8]) -> Result<Vec<u8>, String> {
        let (payload_nonce, ciphertext) = encrypt_with_key(&self.data_key, plaintext)?;
        let env = &self.envelope;

        let mut out = Vec::with_capacity(
            1 + SALT_LEN + NONCE_LEN + WRAPPED_KEY_LEN + NONCE_LEN + ciphertext.len(),
        );
        out.push(ENCRYPTED_VERSION);
        out.extend_from_slice(&env.salt);
        out.extend_from_slice(&env.wrap_nonce);
        out.extend_from_slice(&env.wrapped_key);
        out.extend_from_slice(&payload_nonce);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }
}

/// Encrypts the plaintext using the current envelope format.
///
/// When `session` already holds a key for the current PIN, only a fresh payload
/// nonce is generated — Argon2id is not run. On first use (or after a PIN change
/// that cleared the envelope), Argon2id runs once to wrap the data key.
pub fn encrypt(
    plaintext: &[u8],
    pin: &str,
    session: &mut Option<SessionCrypto>,
) -> Result<Vec<u8>, String> {
    if session.is_none() {
        *session = Some(SessionCrypto::establish(pin)?);
    }
    session
        .as_ref()
        .expect("session established above")
        .encrypt_payload(plaintext)
}

/// True when the bytes begin with the current encrypted-file version.
pub fn is_current_format(data: &[u8]) -> bool {
    data.first().copied() == Some(ENCRYPTED_VERSION)
}

/// Decrypts a current-format blob and returns the plaintext plus a session ready
/// for subsequent saves without re-running Argon2id (except on PIN change).
pub fn decrypt(encrypted: &[u8], pin: &str) -> Result<(Zeroizing<Vec<u8>>, SessionCrypto), String> {
    if !is_current_format(encrypted) {
        return Err("unsupported or corrupted data format".to_string());
    }
    let min_len = 1 + SALT_LEN + NONCE_LEN + WRAPPED_KEY_LEN + NONCE_LEN;
    if encrypted.len() < min_len {
        return Err("truncated encrypted data".to_string());
    }

    let mut offset = 1;
    let salt: [u8; SALT_LEN] = encrypted[offset..offset + SALT_LEN]
        .try_into()
        .map_err(|_| "invalid salt length".to_string())?;
    offset += SALT_LEN;

    let wrap_nonce: [u8; NONCE_LEN] = encrypted[offset..offset + NONCE_LEN]
        .try_into()
        .map_err(|_| "invalid wrap nonce length".to_string())?;
    offset += NONCE_LEN;

    let wrapped_key: [u8; WRAPPED_KEY_LEN] = encrypted[offset..offset + WRAPPED_KEY_LEN]
        .try_into()
        .map_err(|_| "invalid wrapped key length".to_string())?;
    offset += WRAPPED_KEY_LEN;

    let payload_nonce: [u8; NONCE_LEN] = encrypted[offset..offset + NONCE_LEN]
        .try_into()
        .map_err(|_| "invalid payload nonce length".to_string())?;
    offset += NONCE_LEN;

    let ciphertext = &encrypted[offset..];

    let pin_key = derive_key(pin, &salt)?;
    let data_key = unwrap_data_key(&pin_key, &wrap_nonce, &wrapped_key)?;
    let plaintext = decrypt_with_key(&data_key, &payload_nonce, ciphertext)?;

    let session = SessionCrypto {
        data_key,
        envelope: EnvelopeHeader {
            salt,
            wrap_nonce,
            wrapped_key,
        },
    };

    Ok((plaintext, session))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encrypt_decrypt() {
        let data = b"important kid money data 12345";
        let pin = "1234";

        let mut session = None;
        let encrypted = encrypt(data, pin, &mut session).unwrap();
        assert_eq!(encrypted[0], ENCRYPTED_VERSION);
        let (decrypted, session2) = decrypt(&encrypted, pin).unwrap();

        assert_eq!(decrypted.as_slice(), data);
        // Session from decrypt can re-encrypt without needing a new establish for the same PIN.
        let mut session2 = Some(session2);
        let again = encrypt(data, pin, &mut session2).unwrap();
        let (decrypted_again, _) = decrypt(&again, pin).unwrap();
        assert_eq!(decrypted_again.as_slice(), data);
    }

    #[test]
    fn second_encrypt_reuses_session_without_changing_data_key_wrap_semantics() {
        let data = b"ledger";
        let pin = "1234";
        let mut session = None;
        let first = encrypt(data, pin, &mut session).unwrap();
        let second = encrypt(b"ledger2", pin, &mut session).unwrap();

        // Same salt and wrapped key prefix; only payload nonce/ciphertext differ.
        let header_len = 1 + SALT_LEN + NONCE_LEN + WRAPPED_KEY_LEN;
        assert_eq!(&first[..header_len], &second[..header_len]);
        assert_ne!(first, second);
    }

    #[test]
    fn wrong_pin_fails() {
        let data = b"secret";
        let mut session = None;
        let encrypted = encrypt(data, "1234", &mut session).unwrap();

        let result = decrypt(&encrypted, "9999");
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("wrong PIN") || msg.contains("tampered"));
    }

    #[test]
    fn tampered_data_fails() {
        let data = b"money";
        let mut session = None;
        let mut encrypted = encrypt(data, "1234", &mut session).unwrap();

        // flip a byte in the ciphertext
        if encrypted.len() > 50 {
            encrypted[50] ^= 0xff;
        }

        let result = decrypt(&encrypted, "1234");
        assert!(result.is_err());
    }

    #[test]
    fn invalid_version_fails() {
        let mut bad = vec![99u8]; // wrong version
        bad.extend_from_slice(&[0u8; SALT_LEN + NONCE_LEN]);
        bad.extend_from_slice(b"garbage");

        let result = decrypt(&bad, "1234");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unsupported"));
    }

    #[test]
    fn truncated_payloads_fail_without_panicking() {
        for length in 0..(1 + SALT_LEN + NONCE_LEN + WRAPPED_KEY_LEN + NONCE_LEN) {
            let truncated = vec![ENCRYPTED_VERSION; length];
            assert!(decrypt(&truncated, "1234").is_err());
        }
    }

    #[test]
    fn retired_and_unknown_versions_are_not_current() {
        assert!(!is_current_format(&[]));
        assert!(!is_current_format(&[1]));
        assert!(!is_current_format(&[99]));
        assert!(is_current_format(&[ENCRYPTED_VERSION]));
        assert!(decrypt(&[1], "2468").is_err());
    }

    #[test]
    fn pin_change_rewraps_without_losing_data() {
        let data = b"wallet";
        let mut session = None;
        let encrypted = encrypt(data, "1111", &mut session).unwrap();
        let (plain, mut session) = decrypt(&encrypted, "1111").unwrap();
        assert_eq!(plain.as_slice(), data);

        session.rewrap_for_pin("2222").unwrap();
        let mut session = Some(session);
        let rewrapped = encrypt(data, "2222", &mut session).unwrap();
        assert!(decrypt(&rewrapped, "1111").is_err());
        let (plain2, _) = decrypt(&rewrapped, "2222").unwrap();
        assert_eq!(plain2.as_slice(), data);
    }
}
