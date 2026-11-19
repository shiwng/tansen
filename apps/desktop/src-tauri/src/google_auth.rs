//! Google sign-in (OpenID Connect, Authorization Code + PKCE).
//!
//! The flow runs entirely on the Rust side: the system browser opens
//! Google's consent page, a one-shot loopback listener catches the
//! redirect, the code is exchanged for tokens, and the id_token claims are
//! validated before anything is stored. Tokens live in the macOS Keychain;
//! only the profile fields (sub/name/email/picture) are persisted to the
//! settings table and are the only values that ever reach the frontend.
//!
//! Scopes are exactly `openid email profile` — no Gmail, Drive, Contacts,
//! Calendar or anything else. The OAuth client is the "Desktop app" type;
//! its public client configuration comes from the environment (never from
//! the frontend), and PKCE protects the code exchange.

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Build-time or runtime configuration for the Google Cloud OAuth client
/// (type: Desktop app). The client ID is public by design; PKCE protects
/// the code exchange. Set the env var when building the release
/// (`TANSEN_GOOGLE_CLIENT_ID=… tauri build`) or when launching a dev build.
/// Sign-in reports honestly when it is unset.
const CLIENT_ID_ENV: &str = "TANSEN_GOOGLE_CLIENT_ID";
/// Google's token endpoint sometimes still requires the Desktop client
/// secret even with PKCE (its docs mark it optional). When supplied it is
/// used only in the backend token request, never stored with user tokens,
/// serialized to the frontend or logged.
const CLIENT_SECRET_ENV: &str = "TANSEN_GOOGLE_CLIENT_SECRET";

const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const REVOKE_ENDPOINT: &str = "https://oauth2.googleapis.com/revoke";
const CERTS_ENDPOINT: &str = "https://www.googleapis.com/oauth2/v3/certs";
const SCOPES: &str = "openid email profile";
/// How long the loopback listener waits for the browser round trip.
const LISTEN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
/// Refresh this long before expiry so a short-lived access token never
/// expires between a status check and its next use.
const REFRESH_LEEWAY_SEC: u64 = 60;

fn resolve_config_value(runtime: Option<String>, built: Option<&str>) -> Option<String> {
    [runtime, built.map(str::to_owned)]
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty())
}

fn configured_client_id() -> Option<String> {
    resolve_config_value(
        std::env::var(CLIENT_ID_ENV).ok(),
        option_env!("TANSEN_GOOGLE_CLIENT_ID"),
    )
}

fn configured_client_secret() -> Option<String> {
    resolve_config_value(
        std::env::var(CLIENT_SECRET_ENV).ok(),
        option_env!("TANSEN_GOOGLE_CLIENT_SECRET"),
    )
}

#[derive(Debug, thiserror::Error)]
pub enum GoogleAuthError {
    #[error("Google sign-in is not configured — set TANSEN_GOOGLE_CLIENT_ID and relaunch")]
    NotConfigured,
    #[error("sign-in was cancelled or timed out")]
    Cancelled,
    #[error("Google sign-in was denied")]
    Denied,
    #[error("the browser redirect did not carry an authorization code")]
    NoCode,
    #[error("state mismatch — the redirect did not originate from this sign-in")]
    StateMismatch,
    #[error("token exchange failed: {0}")]
    Exchange(String),
    #[error("invalid id_token: {0}")]
    InvalidToken(String),
    #[error("network: {0}")]
    Network(String),
}

impl GoogleAuthError {
    /// Google reports a revoked/expired refresh token as `invalid_grant`;
    /// that is the one refresh failure that must end the local session.
    pub fn is_invalid_grant(&self) -> bool {
        matches!(self, GoogleAuthError::Exchange(message) if message.contains("invalid_grant"))
    }
}

impl From<reqwest::Error> for GoogleAuthError {
    fn from(e: reqwest::Error) -> Self {
        GoogleAuthError::Network(e.to_string())
    }
}

/// The signed-in identity, as persisted and as shown in the sidebar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleProfile {
    pub sub: String,
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub picture: String,
}

/// Keychain payload — never serialized to the frontend or logged. No
/// `Debug` derive: the only way to inspect it is the explicit accessors.
#[derive(Serialize, Deserialize)]
pub(crate) struct GoogleTokens {
    pub(crate) access_token: String,
    #[serde(default)]
    pub(crate) refresh_token: String,
    /// Seconds since the Unix epoch.
    pub(crate) expires_at: u64,
}

/// One-time PKCE material for a sign-in attempt.
pub struct AuthAttempt {
    pub auth_url: String,
    pub nonce: String,
    client_id: String,
    code_verifier: String,
    redirect_uri: String,
}

fn random_urlsafe() -> Result<String, GoogleAuthError> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|e| GoogleAuthError::Network(format!("random: {e}")))?;
    Ok(B64URL.encode(bytes))
}

/// RFC 7636 §4.2: challenge = BASE64URL(SHA256(ASCII(code_verifier))), no padding.
fn s256_challenge(verifier: &str) -> String {
    B64URL.encode(Sha256::digest(verifier.as_bytes()))
}

#[derive(Deserialize)]
struct JwkSet {
    keys: Vec<Jwk>,
}

#[derive(Deserialize)]
struct Jwk {
    kid: String,
    #[serde(default)]
    kty: String,
    #[serde(default)]
    alg: String,
    n: String,
    e: String,
}

#[derive(Deserialize)]
struct JwtHeader {
    alg: String,
    #[serde(default)]
    kid: String,
}

/// Verifies the id_token's RS256 signature against Google's published
/// JWKS. The token arrives over TLS from Google's token endpoint, but the
/// signature check is still mandatory: it proves the claims were signed by
/// Google for the key named in the header, not merely relayed by the
/// network. Keys are matched by `kid`; an unknown or non-RS256 key fails.
fn verify_id_token_signature(id_token: &str, jwks_json: &str) -> Result<(), GoogleAuthError> {
    let mut parts = id_token.split('.');
    let (Some(header_b64), Some(payload_b64), Some(signature_b64), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(GoogleAuthError::InvalidToken("not a JWT".into()));
    };
    let header: JwtHeader = serde_json::from_slice(
        &B64URL
            .decode(header_b64)
            .map_err(|e| GoogleAuthError::InvalidToken(format!("header: {e}")))?,
    )
    .map_err(|e| GoogleAuthError::InvalidToken(format!("header: {e}")))?;
    if header.alg != "RS256" {
        return Err(GoogleAuthError::InvalidToken(format!(
            "algorithm {}",
            header.alg
        )));
    }
    if header.kid.is_empty() {
        return Err(GoogleAuthError::InvalidToken("missing key id".into()));
    }
    let set: JwkSet = serde_json::from_str(jwks_json)
        .map_err(|e| GoogleAuthError::InvalidToken(format!("jwks: {e}")))?;
    let key = set
        .keys
        .iter()
        .find(|k| k.kid == header.kid && k.kty == "RSA" && (k.alg.is_empty() || k.alg == "RS256"))
        .ok_or_else(|| GoogleAuthError::InvalidToken("signing key not found".into()))?;
    let n = B64URL
        .decode(&key.n)
        .map_err(|e| GoogleAuthError::InvalidToken(format!("jwks modulus: {e}")))?;
    let e = B64URL
        .decode(&key.e)
        .map_err(|e| GoogleAuthError::InvalidToken(format!("jwks exponent: {e}")))?;
    let signature = B64URL
        .decode(signature_b64)
        .map_err(|e| GoogleAuthError::InvalidToken(format!("signature: {e}")))?;
    let public_key = ring::signature::RsaPublicKeyComponents { n: &n, e: &e };
    let signing_input = format!("{header_b64}.{payload_b64}");
    public_key
        .verify(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            signing_input.as_bytes(),
            &signature,
        )
        .map_err(|_| GoogleAuthError::InvalidToken("signature mismatch".into()))
}

/// Validates the id_token claims that matter for identity display and
/// extracts the profile. Signature verification happens in
/// [`verify_id_token_signature`]; both checks are required for the token to
/// be accepted. `nonce` is mandatory: this flow sends one, so Google must
/// echo it back, which binds the id_token to this specific attempt.
fn profile_from_id_token(
    id_token: &str,
    client_id: &str,
    nonce: &str,
    now: u64,
) -> Result<GoogleProfile, GoogleAuthError> {
    let payload_b64 = id_token
        .split('.')
        .nth(1)
        .ok_or_else(|| GoogleAuthError::InvalidToken("not a JWT".into()))?;
    let payload = B64URL
        .decode(payload_b64)
        .map_err(|e| GoogleAuthError::InvalidToken(format!("payload: {e}")))?;
    #[derive(Deserialize)]
    struct Claims {
        iss: String,
        aud: String,
        exp: u64,
        nonce: Option<String>,
        sub: String,
        #[serde(default)]
        name: String,
        #[serde(default)]
        email: String,
        #[serde(default)]
        email_verified: bool,
        #[serde(default)]
        picture: String,
    }
    let c: Claims = serde_json::from_slice(&payload)
        .map_err(|e| GoogleAuthError::InvalidToken(format!("claims: {e}")))?;
    if c.iss != "accounts.google.com" && c.iss != "https://accounts.google.com" {
        return Err(GoogleAuthError::InvalidToken(format!("issuer {}", c.iss)));
    }
    if c.aud != client_id {
        return Err(GoogleAuthError::InvalidToken(
            "audience is not this client".into(),
        ));
    }
    if c.exp <= now.saturating_sub(60) {
        return Err(GoogleAuthError::InvalidToken("token expired".into()));
    }
    if let Some(n) = &c.nonce {
        if n != nonce {
            return Err(GoogleAuthError::InvalidToken("nonce mismatch".into()));
        }
    } else {
        return Err(GoogleAuthError::InvalidToken("missing nonce".into()));
    }
    if !c.email_verified {
        return Err(GoogleAuthError::InvalidToken("email not verified".into()));
    }
    Ok(GoogleProfile {
        sub: c.sub,
        name: c.name,
        email: c.email,
        picture: c.picture,
    })
}

/// The consent URL. Scopes are exactly `openid email profile`; PKCE is
/// always S256, and `access_type=offline` requests a refresh token.
fn build_auth_url(
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    nonce: &str,
    code_challenge: &str,
) -> String {
    format!(
        "{AUTH_ENDPOINT}?response_type=code&client_id={client}&redirect_uri={redirect}&scope={scopes}&state={state}&nonce={nonce}&code_challenge={challenge}&code_challenge_method=S256&access_type=offline&prompt={prompt}",
        client = urlencode(client_id),
        redirect = urlencode(redirect_uri),
        scopes = urlencode(SCOPES),
        state = urlencode(state),
        nonce = urlencode(nonce),
        challenge = code_challenge,
        prompt = urlencode("select_account"),
    )
}

/// Starts a sign-in: binds a loopback listener, builds the consent URL, and
/// returns the attempt plus a receiver for the redirect parameters. The
/// caller owns the listener's lifetime and must drop it to close the port.
pub fn start() -> Result<
    (
        AuthAttempt,
        std::sync::mpsc::Receiver<Result<String, GoogleAuthError>>,
    ),
    GoogleAuthError,
> {
    let client_id = configured_client_id().ok_or(GoogleAuthError::NotConfigured)?;
    let state = random_urlsafe()?;
    let nonce = random_urlsafe()?;
    let code_verifier = random_urlsafe()?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| GoogleAuthError::Network(format!("bind: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| GoogleAuthError::Network(format!("local_addr: {e}")))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let auth_url = build_auth_url(
        &client_id,
        &redirect_uri,
        &state,
        &nonce,
        &s256_challenge(&code_verifier),
    );

    let (tx, rx) = std::sync::mpsc::channel();
    let expected_state = state.clone();
    std::thread::spawn(move || {
        // Exactly one browser redirect is accepted; the listener closes after
        // it (or on timeout when the user cancels in the browser).
        let result = (|| -> Result<String, GoogleAuthError> {
            let mut stream = accept_with_timeout(&listener)?;
            let mut buf = [0u8; 4096];
            let n = std::io::Read::read(&mut &stream, &mut buf)
                .map_err(|e| GoogleAuthError::Network(format!("read: {e}")))?;
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let target = request.split_whitespace().nth(1).unwrap_or("");
            let query = target.split('?').nth(1).unwrap_or("");
            // Percent-decode through the URL parser: authorization codes are
            // opaque and may arrive percent-encoded.
            let params: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect();
            let find = |key: &str| {
                params
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
            };
            let code = find("code");
            let got_state = find("state");
            let error = find("error");
            let respond = |body: &str, ok: bool| -> String {
                format!(
                    "HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    if ok { "200 OK" } else { "400 Bad Request" },
                    body.len(),
                    body
                )
            };
            // Validate state before acting on any other parameter: an error
            // redirect from another flow must not cancel this attempt.
            let Some(returned) = got_state else {
                let _ = std::io::Write::write_all(
                    &mut stream,
                    respond("Sign-in failed — missing state.", false).as_bytes(),
                );
                return Err(GoogleAuthError::StateMismatch);
            };
            if returned != expected_state {
                let _ = std::io::Write::write_all(
                    &mut stream,
                    respond("Sign-in failed — state mismatch.", false).as_bytes(),
                );
                return Err(GoogleAuthError::StateMismatch);
            }
            if let Some(error) = error {
                let _ = std::io::Write::write_all(
                    &mut stream,
                    respond("Sign-in failed — you may close this tab.", false).as_bytes(),
                );
                return Err(if error == "access_denied" {
                    GoogleAuthError::Denied
                } else {
                    GoogleAuthError::Exchange(format!("authorization error: {error}"))
                });
            }
            let Some(code) = code else {
                let _ = std::io::Write::write_all(
                    &mut stream,
                    respond("Sign-in failed — missing authorization code.", false).as_bytes(),
                );
                return Err(GoogleAuthError::NoCode);
            };
            let _ = std::io::Write::write_all(
                &mut stream,
                respond("Sign-in complete — return to Tansen.", true).as_bytes(),
            );
            Ok(code)
        })();
        let _ = tx.send(result);
    });

    let attempt = AuthAttempt {
        auth_url,
        nonce,
        client_id,
        code_verifier,
        redirect_uri,
    };
    Ok((attempt, rx))
}

fn accept_with_timeout(
    listener: &std::net::TcpListener,
) -> Result<std::net::TcpStream, GoogleAuthError> {
    listener
        .set_nonblocking(true)
        .map_err(|e| GoogleAuthError::Network(format!("nonblocking: {e}")))?;
    let deadline = std::time::Instant::now() + LISTEN_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).ok();
                return Ok(stream);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err(GoogleAuthError::Cancelled);
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(GoogleAuthError::Network(format!("accept: {e}"))),
        }
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    id_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    expires_in: u64,
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: String,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn http_client() -> Result<reqwest::Client, GoogleAuthError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| GoogleAuthError::Network(format!("client: {e}")))
}

/// Fetches Google's current signing keys. They rotate, so every sign-in
/// fetches them fresh rather than trusting a cached copy.
async fn fetch_jwks(client: &reqwest::Client) -> Result<String, GoogleAuthError> {
    client
        .get(CERTS_ENDPOINT)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await
        .map_err(GoogleAuthError::from)
}

/// Exchanges the authorization code for tokens and validates the id_token's
/// signature and claims.
pub async fn exchange(
    attempt: &AuthAttempt,
    code: &str,
) -> Result<(GoogleTokens, GoogleProfile), GoogleAuthError> {
    let now = unix_now();
    let client = http_client()?;
    let client_secret = configured_client_secret();
    let mut form = vec![
        ("client_id", attempt.client_id.as_str()),
        ("code", code),
        ("code_verifier", attempt.code_verifier.as_str()),
        ("grant_type", "authorization_code"),
        ("redirect_uri", attempt.redirect_uri.as_str()),
    ];
    if let Some(secret) = client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    let resp = client.post(TOKEN_ENDPOINT).form(&form).send().await?;
    let body: TokenResponse = resp
        .json()
        .await
        .map_err(|e| GoogleAuthError::Exchange(format!("token json: {e}")))?;
    if !body.error.is_empty() {
        return Err(GoogleAuthError::Exchange(format!(
            "{}: {}",
            body.error, body.error_description
        )));
    }
    if body.id_token.is_empty() {
        return Err(GoogleAuthError::Exchange(
            "token response has no id_token".into(),
        ));
    }
    let jwks = fetch_jwks(&client).await?;
    verify_id_token_signature(&body.id_token, &jwks)?;
    let profile = profile_from_id_token(&body.id_token, &attempt.client_id, &attempt.nonce, now)?;
    Ok((
        GoogleTokens {
            access_token: body.access_token,
            refresh_token: body.refresh_token,
            expires_at: now + body.expires_in,
        },
        profile,
    ))
}

/// True when the access token is expired or about to expire. A missing
/// expiry (0) is treated as expired so a refresh is attempted once.
pub(crate) fn needs_refresh(tokens: &GoogleTokens, now: u64) -> bool {
    tokens.expires_at <= now.saturating_add(REFRESH_LEEWAY_SEC)
}

/// Refreshes an expired access token. Google may omit a new refresh token;
/// the existing one stays valid and is carried forward.
pub async fn refresh(refresh_token: &str) -> Result<GoogleTokens, GoogleAuthError> {
    let client_id = configured_client_id().ok_or(GoogleAuthError::NotConfigured)?;
    let now = unix_now();
    let client = http_client()?;
    let client_secret = configured_client_secret();
    let mut form = vec![
        ("client_id", client_id.as_str()),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];
    if let Some(secret) = client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    let resp = client.post(TOKEN_ENDPOINT).form(&form).send().await?;
    let body: TokenResponse = resp
        .json()
        .await
        .map_err(|e| GoogleAuthError::Exchange(format!("token json: {e}")))?;
    if !body.error.is_empty() {
        return Err(GoogleAuthError::Exchange(format!(
            "{}: {}",
            body.error, body.error_description
        )));
    }
    if body.access_token.is_empty() {
        return Err(GoogleAuthError::Exchange(
            "refresh response has no access_token".into(),
        ));
    }
    Ok(GoogleTokens {
        access_token: body.access_token,
        refresh_token: if body.refresh_token.is_empty() {
            refresh_token.to_owned()
        } else {
            body.refresh_token
        },
        expires_at: now + body.expires_in,
    })
}

/// Best-effort token revocation on sign-out (RFC 7009). Failure is ignored:
/// local sign-out is authoritative for Tansen.
pub async fn revoke(access_token: &str) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok();
    if let Some(client) = client {
        let _ = client
            .post(REVOKE_ENDPOINT)
            .form(&[("token", access_token)])
            .send()
            .await;
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC7636_VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    const RFC7636_CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

    #[test]
    fn pkce_s256_matches_rfc7636_vector() {
        assert_eq!(s256_challenge(RFC7636_VERIFIER), RFC7636_CHALLENGE);
    }

    #[test]
    fn urlsafe_encoding_has_no_padding() {
        let e = random_urlsafe().unwrap();
        assert_eq!(e.len(), 43, "32 bytes -> 43 base64url chars");
        assert!(!e.contains('='));
        assert!(e
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
    }

    #[test]
    fn urlencode_leaves_unreserved_and_escapes_the_rest() {
        assert_eq!(urlencode("abc.123-def~_x"), "abc.123-def~_x");
        assert_eq!(urlencode("a b/c?d=e"), "a%20b%2Fc%3Fd%3De");
    }

    fn make_id_token(claims: &serde_json::Value) -> String {
        let header = B64URL.encode(br#"{"alg":"RS256","kid":"k"}"#);
        let payload = B64URL.encode(claims.to_string().as_bytes());
        format!("{header}.{payload}.sig")
    }

    fn claims(over: serde_json::Value) -> serde_json::Value {
        let mut base = serde_json::json!({
            "iss": "https://accounts.google.com",
            "aud": "my-client",
            "exp": 4_102_444_800u64,
            "sub": "sub-123",
            "name": "Alice Example",
            "email": "alice@example.com",
            "email_verified": true,
            "picture": "https://lh3.example/p.png",
            "nonce": "nonce-1",
        });
        if let (Some(base_obj), Some(over_obj)) = (base.as_object_mut(), over.as_object()) {
            for (k, v) in over_obj {
                base_obj.insert(k.clone(), v.clone());
            }
        }
        base
    }

    #[test]
    fn valid_id_token_yields_profile() {
        let p = profile_from_id_token(
            &make_id_token(&claims(serde_json::json!({}))),
            "my-client",
            "nonce-1",
            1_000_000,
        )
        .unwrap();
        assert_eq!(p.sub, "sub-123");
        assert_eq!(p.name, "Alice Example");
        assert_eq!(p.email, "alice@example.com");
        assert_eq!(p.picture, "https://lh3.example/p.png");
    }

    #[test]
    fn id_token_rejections() {
        let now = 1_000_000u64;
        // wrong issuer
        assert!(profile_from_id_token(
            &make_id_token(&claims(serde_json::json!({"iss": "https://evil.example"}))),
            "my-client",
            "nonce-1",
            now
        )
        .is_err());
        // wrong audience
        assert!(profile_from_id_token(
            &make_id_token(&claims(serde_json::json!({"aud": "other-client"}))),
            "my-client",
            "nonce-1",
            now
        )
        .is_err());
        // expired
        assert!(profile_from_id_token(
            &make_id_token(&claims(serde_json::json!({"exp": 100u64}))),
            "my-client",
            "nonce-1",
            now
        )
        .is_err());
        // email unverified
        assert!(profile_from_id_token(
            &make_id_token(&claims(serde_json::json!({"email_verified": false}))),
            "my-client",
            "nonce-1",
            now
        )
        .is_err());
        // nonce mismatch
        assert!(profile_from_id_token(
            &make_id_token(&claims(serde_json::json!({"nonce": "other"}))),
            "my-client",
            "nonce-1",
            now
        )
        .is_err());
        // nonce missing (the flow always sends one, so its absence is not valid)
        let mut no_nonce = claims(serde_json::json!({}));
        no_nonce.as_object_mut().unwrap().remove("nonce");
        assert!(
            profile_from_id_token(&make_id_token(&no_nonce), "my-client", "nonce-1", now).is_err()
        );
    }

    // Offline fixture: an RS256 JWT signed with a throwaway test key. Only
    // the public JWK is embedded; the private key is not in the repo.
    const SIGNED_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6InRlc3Qta2V5LTEiLCJ0eXAiOiJKV1QifQ.eyJpc3MiOiJodHRwczovL2FjY291bnRzLmdvb2dsZS5jb20iLCJhdWQiOiJteS1jbGllbnQiLCJleHAiOjQxMDI0NDQ4MDAsImlhdCI6MTcwMDAwMDAwMCwic3ViIjoic3ViLTEyMyIsIm5hbWUiOiJBbGljZSBFeGFtcGxlIiwiZW1haWwiOiJhbGljZUBleGFtcGxlLmNvbSIsImVtYWlsX3ZlcmlmaWVkIjp0cnVlLCJwaWN0dXJlIjoiaHR0cHM6Ly9saDMuZXhhbXBsZS9wLnBuZyIsIm5vbmNlIjoibm9uY2UtMSJ9.mEKQUVehUeWxfx5NRAaqXNQIyuOAdXQQDuM3MrVRCH9pewaEtQdFgZGATc7waNjZMMm8af7PV085QGgiy6mlDFjGnRuQIcDZgePs3fsjtUnzio2XNSs7VpByeaKT_1UndQGY9JFUviye-h1LV83dScC28aEMgbmg-yqzhs-EVcKR2AlfoAaUKo_wxCiWPOaDMqtyHUOyGcVNp-Yb8NWLNiiEFpbxwAptSCBVNWR3AFoSbS5jB_KAy_PAIAHYurk5O_cmUC129GlSDYEfM-ZTJYXWzPx-Bvp6B-IpIFEa_fC3gN5P8IRBhm5XU9eya5NqEjup8ek8VzTFHMLF-nyDuQ";
    const TEST_JWKS: &str = r#"{"keys":[{"kid":"test-key-1","kty":"RSA","alg":"RS256","use":"sig","n":"ztjvq9dn2sam0rax4mte0EnNkFAU9oXP4zDyC5PljBUSAudIC7_yavNMmweA-c7YkZus_OfpkrFaxHKi-YdO1NprOsthmJzGr0QhVdjfuutLu2Dg5nRG98R-LSo3gVCaIRXP_GFS5_HYsM5YVGJ7fgF0Q_idGdG4N9cEM3Gkf6Hc8P61kzcDOnmqnK2kgFPlWR_j6l8o_daYiM5Thtt-BSDXMFgvGiMP9Wb9v_mvQc6CKiQUXtH8ABjrEAPPVeqwiXmrmXP5anZu8I2yLlcOdbTlbMqMFCoDXrpo09DuIgvvmrfsjidLvfVIcTbXypvfaXsBRvy-LkHMSJ6jxcLwZw","e":"AQAB"}]}"#;

    #[test]
    fn signature_is_verified_against_the_jwks() {
        verify_id_token_signature(SIGNED_TOKEN, TEST_JWKS).unwrap();
        let profile =
            profile_from_id_token(SIGNED_TOKEN, "my-client", "nonce-1", 1_000_000).unwrap();
        assert_eq!(profile.email, "alice@example.com");

        // Tampered signature.
        let mut tampered = SIGNED_TOKEN.to_owned();
        let last = tampered.pop().unwrap();
        tampered.push(if last == 'A' { 'B' } else { 'A' });
        assert!(verify_id_token_signature(&tampered, TEST_JWKS).is_err());

        // Unknown key id.
        let header = B64URL.encode(br#"{"alg":"RS256","kid":"other-key"}"#);
        let unknown = format!("{header}.{}", SIGNED_TOKEN.split_once('.').unwrap().1);
        assert!(verify_id_token_signature(&unknown, TEST_JWKS).is_err());

        // Non-RS256 algorithm.
        let header = B64URL.encode(br#"{"alg":"none","kid":"test-key-1"}"#);
        let none = format!("{header}.{}", SIGNED_TOKEN.split_once('.').unwrap().1);
        assert!(verify_id_token_signature(&none, TEST_JWKS).is_err());
    }

    #[test]
    fn auth_url_requests_exactly_openid_email_profile_with_pkce() {
        let url = build_auth_url(
            "123.apps.googleusercontent.com",
            "http://127.0.0.1:4567/callback",
            "state-1",
            "nonce-1",
            "challenge-1",
        );
        let parsed = url::Url::parse(&url).unwrap();
        let params: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(
            params.get("scope").map(String::as_str),
            Some("openid email profile")
        );
        assert_eq!(
            params.get("response_type").map(String::as_str),
            Some("code")
        );
        assert_eq!(
            params.get("client_id").map(String::as_str),
            Some("123.apps.googleusercontent.com")
        );
        assert_eq!(
            params.get("redirect_uri").map(String::as_str),
            Some("http://127.0.0.1:4567/callback")
        );
        assert_eq!(params.get("state").map(String::as_str), Some("state-1"));
        assert_eq!(params.get("nonce").map(String::as_str), Some("nonce-1"));
        assert_eq!(
            params.get("code_challenge").map(String::as_str),
            Some("challenge-1")
        );
        assert_eq!(
            params.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert_eq!(
            params.get("access_type").map(String::as_str),
            Some("offline")
        );
        assert_eq!(
            params.get("prompt").map(String::as_str),
            Some("select_account")
        );
        // No Gmail, Drive, Contacts or Calendar scope, and no secret in the URL.
        assert!(!params.contains_key("client_secret"));
        assert!(!url.contains("gmail"));
        assert!(!url.contains("drive"));
        assert!(!url.contains("calendar"));
    }

    #[test]
    fn refresh_window_is_conservative() {
        let tokens = |expires_at| GoogleTokens {
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at,
        };
        assert!(needs_refresh(&tokens(0), 1_000), "missing expiry refreshes");
        assert!(
            needs_refresh(&tokens(1_060), 1_000),
            "inside the 60 s leeway"
        );
        assert!(!needs_refresh(&tokens(1_061), 1_000), "outside the leeway");
        assert!(!needs_refresh(&tokens(9_999), 1_000));
    }

    #[test]
    fn client_id_resolution_prefers_runtime_and_ignores_blank() {
        assert_eq!(resolve_config_value(None, None), None);
        assert_eq!(
            resolve_config_value(Some("  ".into()), Some("built")),
            Some("built".into())
        );
        assert_eq!(
            resolve_config_value(Some(" runtime ".into()), Some("built")),
            Some("runtime".into())
        );
    }

    #[test]
    fn unconfigured_client_is_reported() {
        // No runtime or build-time ID: start() must refuse before any
        // listener exists. If the test shell supplies an ID, the guard is
        // unreachable and this test has nothing to assert.
        assert_eq!(resolve_config_value(None, None), None);
        if configured_client_id().is_some() {
            return;
        }
        match start() {
            Err(GoogleAuthError::NotConfigured) => {}
            other => panic!("expected NotConfigured, got {:?}", other.map(|_| ())),
        }
    }
}
