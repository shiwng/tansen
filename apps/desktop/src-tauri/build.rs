use std::{env, fs, path::Path};

/// Local OAuth configuration, intentionally gitignored. Both `tauri dev` and
/// `tauri build` run this script, so the client ID is available to every
/// build without putting credentials in the frontend or source tree.
const OAUTH_ENV_FILE: &str = "google-oauth.env";

fn file_value(path: &Path, key: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    text.lines().find_map(|line| {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (name, value) = line.split_once('=')?;
        if name.trim() != key {
            return None;
        }
        let value = value
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .to_owned();
        (!value.is_empty()).then_some(value)
    })
}

fn main() {
    let oauth_env = Path::new(OAUTH_ENV_FILE);
    println!("cargo:rerun-if-changed={OAUTH_ENV_FILE}");
    for key in ["TANSEN_GOOGLE_CLIENT_ID", "TANSEN_GOOGLE_CLIENT_SECRET"] {
        println!("cargo:rerun-if-env-changed={key}");
        let from_environment = env::var(key).ok().filter(|v| !v.trim().is_empty());
        if from_environment.is_none() {
            if let Some(value) = file_value(oauth_env, key) {
                println!("cargo:rustc-env={key}={value}");
            }
        }
    }
    tauri_build::build()
}
