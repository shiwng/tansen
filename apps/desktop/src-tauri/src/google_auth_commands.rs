//! Tauri command surface for Google sign-in. The pure flow (PKCE, loopback
//! listener, token exchange, id_token validation) lives in `google_auth`;
//! this file owns the in-flight guard, persistence (Keychain via
//! `sources::keychain`, profile fields via the settings table) and the
//! `google_auth_changed` event the sidebar listens to.

use crate::google_auth::{self, GoogleAuthError, GoogleProfile, GoogleTokens};
use crate::StateHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;

/// One sign-in at a time: a second click while the browser is open must not
/// bind a second loopback listener and orphan the first attempt.
static IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// The persisted profile from the settings table, or None when signed out.
/// An empty stored value counts as absent.
fn load_profile(state: &app_core::AppState) -> Option<GoogleProfile> {
    let rows = persistence::repo::list_settings(&state.db).ok()?;
    let get = |key: &str| {
        rows.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty())
    };
    Some(GoogleProfile {
        sub: get("google_sub")?,
        name: get("google_name").unwrap_or_default(),
        email: get("google_email").unwrap_or_default(),
        picture: get("google_picture").unwrap_or_default(),
    })
}

fn persist_profile(state: &app_core::AppState, profile: &GoogleProfile) -> persistence::Result<()> {
    persistence::repo::set_setting(&state.db, "google_sub", &profile.sub)?;
    persistence::repo::set_setting(&state.db, "google_name", &profile.name)?;
    persistence::repo::set_setting(&state.db, "google_email", &profile.email)?;
    persistence::repo::set_setting(&state.db, "google_picture", &profile.picture)
}

/// Removes the token and profile fields for a session. Used by explicit
/// sign-out and by restore when the persisted session can no longer be
/// refreshed. Never touches another account's Keychain item.
fn clear_session(state: &app_core::AppState, sub: Option<&str>) {
    if let Some(sub) = sub {
        let _ = sources::keychain::delete_google_token(sub);
    }
    for key in [
        "google_sub",
        "google_name",
        "google_email",
        "google_picture",
    ] {
        let _ = persistence::repo::delete_setting(&state.db, key);
    }
}

fn revoke_stored_token(profile: &GoogleProfile) {
    let Ok(Some(json)) = sources::keychain::get_google_token(&profile.sub) else {
        return;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) else {
        return;
    };
    let token = parsed
        .get("access_token")
        .or_else(|| parsed.get("refresh_token"))
        .and_then(|v| v.as_str());
    if let Some(token) = token {
        let token = token.to_owned();
        // Local sign-out stays authoritative even if the network is gone;
        // revocation failure is never surfaced. Revoking either token
        // invalidates the grant, including a stored refresh token.
        tokio::spawn(async move { google_auth::revoke(&token).await });
    }
}

/// Signs in through the system browser and resolves to the signed-in profile.
/// Rejects with `GoogleAuthError::NotConfigured` until a client ID is
/// embedded. Tokens are stored in the Keychain only; the frontend receives
/// profile fields exclusively. On success `google_auth_changed` carries the
/// profile; a failed attempt emits nothing (the previous session is intact).
#[tauri::command]
pub async fn google_auth_start(
    app: tauri::AppHandle,
    state: StateHandle<'_>,
) -> Result<GoogleProfile, String> {
    if IN_PROGRESS.swap(true, Ordering::SeqCst) {
        return Err("A Google sign-in is already in progress".into());
    }
    let result = run_sign_in(&state).await;
    IN_PROGRESS.store(false, Ordering::SeqCst);
    if let Ok(profile) = &result {
        let _ = app.emit("google_auth_changed", Some(profile));
    }
    result.map_err(|e| e.to_string())
}

async fn run_sign_in(state: &app_core::AppState) -> Result<GoogleProfile, GoogleAuthError> {
    let (attempt, rx) = google_auth::start()?;
    open_system_browser(&attempt.auth_url)?;
    // The redirect wait blocks on a std channel; keep it off the async
    // runtime. recv() adds its own Result layer: Ok(Ok(code)) is the only
    // success shape; the listener's own error (cancelled, state mismatch…)
    // is surfaced as-is, and a missing send means the thread died.
    let code = match tauri::async_runtime::spawn_blocking(move || rx.recv()).await {
        Ok(Ok(Ok(code))) => code,
        Ok(Ok(Err(e))) => return Err(e),
        Ok(Err(_)) => {
            return Err(GoogleAuthError::Network(
                "redirect listener terminated".into(),
            ))
        }
        Err(e) => return Err(GoogleAuthError::Network(format!("join: {e}"))),
    };
    let (tokens, profile) = google_auth::exchange(&attempt, &code).await?;
    // A stale item for a different account must not survive the switch.
    if let Some(previous) = load_profile(state) {
        if previous.sub != profile.sub {
            let _ = sources::keychain::delete_google_token(&previous.sub);
        }
    }
    let token_json = serde_json::to_string(&tokens)
        .map_err(|e| GoogleAuthError::Network(format!("token json: {e}")))?;
    sources::keychain::store_google_token(&profile.sub, &token_json)
        .map_err(|e| GoogleAuthError::Network(format!("keychain: {e}")))?;
    if let Err(e) = persist_profile(state, &profile) {
        // Do not leave a token behind for a session the UI cannot restore.
        let _ = sources::keychain::delete_google_token(&profile.sub);
        return Err(GoogleAuthError::Network(format!("persist: {e}")));
    }
    Ok(profile)
}

/// The system browser, not a webview: Google's consent page must not run
/// inside the app's own process, and no shell plugin is involved.
fn open_system_browser(url: &str) -> Result<(), GoogleAuthError> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| GoogleAuthError::Network(format!("open browser: {e}")))
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map(|_| ())
            .map_err(|e| GoogleAuthError::Network(format!("open browser: {e}")))
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| GoogleAuthError::Network(format!("open browser: {e}")))
    }
}

/// The signed-in profile, or null when signed out (session restore at
/// launch). A missing/unreadable token ends the session; an expired access
/// token is refreshed with the Keychain-held refresh token. A network
/// failure during refresh keeps the local profile (Tansen makes no Google
/// API calls) and retries on the next launch; `invalid_grant` means the
/// grant is gone and signs the session out.
#[tauri::command]
pub async fn google_auth_status(state: StateHandle<'_>) -> Result<Option<GoogleProfile>, String> {
    let Some(profile) = load_profile(&state) else {
        return Ok(None);
    };
    let tokens = match sources::keychain::get_google_token(&profile.sub) {
        Ok(Some(json)) => match serde_json::from_str::<GoogleTokens>(&json) {
            Ok(tokens) => tokens,
            Err(_) => {
                clear_session(&state, Some(&profile.sub));
                return Ok(None);
            }
        },
        Ok(None) => {
            clear_session(&state, Some(&profile.sub));
            return Ok(None);
        }
        // Keychain temporarily locked: keep the profile rather than
        // destroying a valid session over a transient read failure.
        Err(_) => return Ok(Some(profile)),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if !google_auth::needs_refresh(&tokens, now) {
        return Ok(Some(profile));
    }
    if tokens.refresh_token.is_empty() {
        clear_session(&state, Some(&profile.sub));
        return Ok(None);
    }
    match google_auth::refresh(&tokens.refresh_token).await {
        Ok(updated) => {
            if let Ok(json) = serde_json::to_string(&updated) {
                if sources::keychain::store_google_token(&profile.sub, &json).is_err() {
                    tracing::warn!("Google token refresh succeeded but Keychain update failed");
                }
            }
            Ok(Some(profile))
        }
        Err(e) if e.is_invalid_grant() => {
            clear_session(&state, Some(&profile.sub));
            Ok(None)
        }
        Err(e) => {
            // Offline or Google unavailable: keep the local session. It is
            // cosmetic (no Google API is called) and retried next launch.
            tracing::warn!(error = %e, "Google session refresh deferred");
            Ok(Some(profile))
        }
    }
}

/// Signs out: revokes the stored token at Google (best-effort, detached),
/// deletes the Keychain item and every profile setting, and emits null.
#[tauri::command]
pub async fn google_sign_out(app: tauri::AppHandle, state: StateHandle<'_>) -> Result<(), String> {
    let profile = load_profile(&state);
    if let Some(profile) = &profile {
        revoke_stored_token(profile);
    }
    clear_session(&state, profile.as_ref().map(|p| p.sub.as_str()));
    let _ = app.emit::<Option<GoogleProfile>>("google_auth_changed", None);
    Ok(())
}
