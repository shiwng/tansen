//! S9: macOS Keychain storage for Subsonic credentials via the Security
//! framework (`objc2-security`, the one new dependency approved for S9).
//! Passwords NEVER touch SQLite, logs, events or frontend state — only
//! usernames and server URLs live in the settings DB.

use crate::SourceError;

/// Keychain service name for Subsonic credentials.
pub const KEYCHAIN_SERVICE: &str = "app.tansen.desktop.subsonic";

#[cfg(target_os = "macos")]
pub mod imp {
    use super::*;
    use objc2_core_foundation::{
        CFBoolean, CFData, CFMutableDictionary, CFRetained, CFString, CFType,
        kCFBooleanTrue,
    };
    use objc2_security::{
        errSecItemNotFound, errSecSuccess, kSecAttrAccount, kSecAttrService, kSecClass,
        kSecClassGenericPassword, kSecReturnData, kSecValueData, SecItemAdd, SecItemCopyMatching,
        SecItemDelete,
    };

    /// The Security constants are extern statics; resolve them once here.
    fn sec_keys(
    ) -> (
        &'static CFString,
        &'static CFString,
        &'static CFString,
        &'static CFString,
        &'static CFString,
        &'static CFString,
        &'static CFBoolean,
    ) {
        unsafe {
            (
                kSecClass,
                kSecAttrService,
                kSecAttrAccount,
                kSecValueData,
                kSecReturnData,
                kSecClassGenericPassword,
                kCFBooleanTrue.expect("kCFBooleanTrue"),
            )
        }
    }

    fn service_string() -> CFRetained<CFString> {
        CFString::from_str(KEYCHAIN_SERVICE)
    }

    /// Upserts a generic-password item for `account` (the Subsonic username).
    pub fn store_password(account: &str, password: &str) -> Result<(), SourceError> {
        let (sec_class, sec_service, sec_account, sec_value, _sec_return, class, _boolean) = sec_keys();
        let dict = CFMutableDictionary::<CFString, CFType>::empty();
        let service = service_string();
        let account_str = CFString::from_str(account);
        let data = CFData::from_bytes(password.as_bytes());
        dict.set(sec_class, &class);
        dict.set(sec_service, &service);
        dict.set(sec_account, &account_str);
        dict.set(sec_value, &data);
        let status = unsafe { SecItemAdd(dict.as_opaque(), std::ptr::null_mut()) };
        if status == errSecSuccess {
            Ok(())
        } else if status == -25299 {
            // errSecDuplicateItem: objc2-security binds no SecItemUpdate, so
            // replace by delete-then-add.
            let _ = delete_password(account);
            let status = unsafe { SecItemAdd(dict.as_opaque(), std::ptr::null_mut()) };
            if status == errSecSuccess {
                Ok(())
            } else {
                Err(SourceError::Permanent(format!("keychain add failed: {status}")))
            }
        } else {
            Err(SourceError::Permanent(format!("keychain add failed: {status}")))
        }
    }

    /// Reads the stored password; `Ok(None)` when no item exists.
    pub fn get_password(account: &str) -> Result<Option<String>, SourceError> {
        let (sec_class, sec_service, sec_account, _sec_value, sec_return, class, ret_data) = sec_keys();
        let query = CFMutableDictionary::<CFString, CFType>::empty();
        let service = service_string();
        let account_str = CFString::from_str(account);
        query.set(sec_class, &class);
        query.set(sec_service, &service);
        query.set(sec_account, &account_str);
        query.set(sec_return, &ret_data);
        let mut result: *const CFType = std::ptr::null();
        let status = unsafe { SecItemCopyMatching(query.as_opaque(), &mut result) };
        if status == errSecItemNotFound {
            return Ok(None);
        }
        if status != errSecSuccess || result.is_null() {
            return Err(SourceError::Permanent(format!("keychain read failed: {status}")));
        }
        let ptr = std::ptr::NonNull::new(result as *mut CFData)
            .ok_or_else(|| SourceError::Permanent("keychain result null".into()))?;
        let data = unsafe { CFRetained::<CFData>::from_raw(ptr) };
        Ok(Some(String::from_utf8_lossy(&data.to_vec()).into_owned()))
    }

    /// Deletes the stored item (best-effort for source removal).
    pub fn delete_password(account: &str) -> Result<(), SourceError> {
        let (sec_class, sec_service, sec_account, _sec_value, _sec_return, class, _boolean) = sec_keys();
        let query = CFMutableDictionary::<CFString, CFType>::empty();
        let service = service_string();
        let account_str = CFString::from_str(account);
        query.set(sec_class, &class);
        query.set(sec_service, &service);
        query.set(sec_account, &account_str);
        let status = unsafe { SecItemDelete(query.as_opaque()) };
        if status == errSecSuccess || status == errSecItemNotFound {
            Ok(())
        } else {
            Err(SourceError::Permanent(format!("keychain delete failed: {status}")))
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub mod imp {
    use super::*;

    pub fn store_password(_account: &str, _password: &str) -> Result<(), SourceError> {
        Err(SourceError::Permanent("keychain unavailable on this platform".into()))
    }

    pub fn get_password(_account: &str) -> Result<Option<String>, SourceError> {
        Ok(None)
    }

    pub fn delete_password(_account: &str) -> Result<(), SourceError> {
        Err(SourceError::Permanent("keychain unavailable on this platform".into()))
    }
}

pub use imp::{delete_password, get_password, store_password};

/// The Keychain account key for one Subsonic credential: username AND
/// server (host + non-default port, so same-host-different-port servers
/// do not collide either). Keying by username alone let two same-username
/// servers overwrite each other's password (delete-then-add) and let
/// server A authenticate with server B's credential; the server that the
/// secret belongs to must be part of the item identity.
pub fn credential_account(username: &str, base_url: &str) -> String {
    let parsed = url::Url::parse(base_url).ok();
    let host = parsed.as_ref().and_then(|u| u.host_str()).unwrap_or_default();
    if host.is_empty() {
        return username.to_string();
    }
    let mut server = host.to_ascii_lowercase();
    if let Some(port) = parsed.as_ref().and_then(|u| u.port()) {
        server.push_str(&format!(":{port}"));
    }
    format!("{username}@{server}")
}

/// Reads a server-bound credential, falling back to the pre-fix
/// username-only key so credentials stored by older builds keep working
/// until re-saved (which migrates them to the bound key).
pub fn get_password_server_bound(account: &str) -> Result<Option<String>, SourceError> {
    if let Some(found) = get_password(account)? {
        return Ok(Some(found));
    }
    match account.split_once('@') {
        Some((legacy_username, _host)) if !legacy_username.is_empty() => get_password(legacy_username),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_account_binds_the_server() {
        assert_eq!(
            credential_account("alice", "https://music.example.com:443/rest"),
            "alice@music.example.com",
            "default port omitted"
        );
        assert_eq!(
            credential_account("Alice", "http://Home.local:4040/"),
            "Alice@home.local:4040",
            "non-default port kept: same host on another port is another server"
        );
        // Unparseable base falls back to the username (never panics).
        assert_eq!(credential_account("bob", "not a url"), "bob");
        // Distinct servers never share one item.
        assert_ne!(
            credential_account("alice", "https://a.example.com"),
            credential_account("alice", "https://b.example.com")
        );
        assert_ne!(
            credential_account("alice", "https://music.example.com"),
            credential_account("alice", "https://music.example.com:8443"),
            "same host, different port must not share the item"
        );
    }

    /// F0: runtime verification of the S9 SecItem round trip against the
    /// real login keychain. #[ignore]d by default (tests must not touch the
    /// user keychain implicitly) — run explicitly:
    ///   cargo test -p sources --lib keychain -- --ignored
    /// Stores a throwaway secret under the real service with a sentinel
    /// account, reads it back, then DELETES it; the item never outlives the
    /// test even on failure (drop guard).
    #[test]
    #[ignore]
    fn keychain_roundtrip_store_read_delete() {
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = imp::delete_password(&self.0);
            }
        }
        let account = "tansen-f0-verification";
        let _guard = Cleanup(account.to_string());
        // Deterministic starting state.
        let _ = imp::delete_password(account);
        assert_eq!(imp::get_password(account).unwrap(), None, "starts absent");

        imp::store_password(account, "s3cret-🎁-value").unwrap();
        assert_eq!(
            imp::get_password(account).unwrap().as_deref(),
            Some("s3cret-🎁-value"),
            "stored secret reads back byte-exact (UTF-8)"
        );
        // Upsert path (duplicate → delete-then-add).
        imp::store_password(account, "s3cret-2").unwrap();
        assert_eq!(imp::get_password(account).unwrap().as_deref(), Some("s3cret-2"));
        imp::delete_password(account).unwrap();
        assert_eq!(imp::get_password(account).unwrap(), None, "delete removes");
    }
}
