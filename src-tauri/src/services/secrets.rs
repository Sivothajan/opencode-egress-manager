//! DPAPI-backed storage for proxy credentials.
//!
//! Credentials never touch the plain configuration file. Each pool entry's
//! username/password is encrypted with the per-user DPAPI master key and stored
//! under `lane/credentials/<n>.bin`; the lookup file keeps only the host:port.

use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

use crate::{
    config::Paths,
    errors::{redact, AppError, Result},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CredentialRecord {
    host: String,
    port: u16,
    #[serde(with = "base64_blob")]
    blob: Vec<u8>,
}

mod base64_blob {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(value);
        serializer.serialize_str(&encoded)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CredentialStore {
    #[serde(default)]
    entries: Vec<CredentialRecord>,
}

fn store_path(paths: &Paths) -> PathBuf {
    paths.lane_dir.join("credentials.json")
}

fn protect(plaintext: &str) -> Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: plaintext.len() as u32,
        pbData: plaintext.as_bytes().as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| AppError::Other(format!("DPAPI protect failed: {e}")))?;
        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let out = slice.to_vec();
        if !output.pbData.is_null() {
            LocalFree(Some(HLOCAL(output.pbData as *mut core::ffi::c_void)));
        }
        Ok(out)
    }
}

fn unprotect(blob: &[u8]) -> Result<String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: blob.len() as u32,
        pbData: blob.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| AppError::Other(format!("DPAPI unprotect failed: {e}")))?;
        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let text = String::from_utf8_lossy(slice).to_string();
        if !output.pbData.is_null() {
            LocalFree(Some(HLOCAL(output.pbData as *mut core::ffi::c_void)));
        }
        Ok(text)
    }
}

fn load_store(paths: &Paths) -> CredentialStore {
    fs::read_to_string(store_path(paths))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Replace the credential store with credentials parsed from a proxy list.
///
/// Entries without credentials are stored with an empty blob so lookups remain
/// unambiguous and no plaintext password survives.
pub fn store_proxy_credentials(paths: &Paths, raw: &str) -> Result<()> {
    let specs =
        crate::services::proxy::chain::parse_proxy_specs(raw, crate::config::ProxyType::Socks5);
    let mut entries = Vec::new();
    for spec in specs {
        let blob = if spec.username.is_empty() && spec.password.is_empty() {
            Vec::new()
        } else {
            protect(&format!("{}\u{0}{}", spec.username, spec.password))?
        };
        entries.push(CredentialRecord {
            host: spec.host,
            port: spec.port,
            blob,
        });
    }
    let store = CredentialStore { entries };
    let text = serde_json::to_string_pretty(&store)?;
    crate::config::write_atomic_pub(&store_path(paths), text.as_bytes())?;
    Ok(())
}

/// Look up decrypted credentials for a pool node.
///
/// Returns `None` when no stored record exists, so inline credentials from the
/// configuration string are preserved.
pub fn credentials_for(paths: &Paths, host: &str, port: u16) -> Option<(String, String)> {
    let store = load_store(paths);
    for entry in store.entries {
        if entry.host == host && entry.port == port {
            if entry.blob.is_empty() {
                return Some((String::new(), String::new()));
            }
            return match unprotect(&entry.blob) {
                Ok(text) => {
                    let mut parts = text.splitn(2, '\u{0}');
                    Some((
                        parts.next().unwrap_or("").to_string(),
                        parts.next().unwrap_or("").to_string(),
                    ))
                }
                Err(err) => {
                    tracing::debug!(error = %redact(&err.user_message()), "credential decrypt failed");
                    Some((String::new(), String::new()))
                }
            };
        }
    }
    None
}

/// Remove all stored credentials (used by uninstall).
pub fn clear_credentials(paths: &Paths) -> Result<()> {
    let path = store_path(paths);
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}
