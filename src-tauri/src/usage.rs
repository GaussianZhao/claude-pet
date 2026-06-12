//! Plan-usage poller — fetches the Claude subscription rate-limit windows
//! (5-hour session + 7-day weekly) the same way Claude Code's `/usage` does:
//! the numbers ride on `anthropic-ratelimit-unified-*` response headers, so we
//! make a tiny `max_tokens:1` "quota" request and read them back. Utilization is
//! the fraction of the window used (0..1); `resets_at` is unix epoch seconds.
//!
//! The hard part is auth. We never ask the user to log in again — we reuse the
//! token Claude already stored, trying, in order:
//!   1. Claude *desktop app*: `oauth:tokenCache` in its config.json, encrypted
//!      with Electron safeStorage (Chromium "v10" AES-128-CBC, key from the
//!      "Claude Safe Storage" keychain item).
//!   2. Claude *CLI*: the `Claude Code-credentials` macOS keychain item (JSON).
//!   3. `~/.claude/.credentials.json` (file-based fallback).
//!
//! Anything missing/expired just yields `None` and the UI hides the bars.

use serde::Serialize;

/// One rate-limit window. `used_percent` is 0..100 (kept integer so `PetState`
/// can stay `Eq`); `resets_at` is unix epoch seconds (0 = unknown).
#[derive(Serialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub used_percent: u8,
    pub resets_at: i64,
}

/// The plan-usage snapshot pushed to the webview.
#[derive(Serialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    /// `anthropic-ratelimit-unified-status` (e.g. "allowed", "allowed_warning").
    pub status: String,
}

/// Fetch the current usage windows, or `None` if we can't auth or reach the API.
pub fn fetch() -> Option<Usage> {
    let token = find_token()?;
    quota_request(&token)
}

// ---- the quota request ------------------------------------------------------

fn quota_request(token: &str) -> Option<Usage> {
    let body = serde_json::json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 1,
        "messages": [{ "role": "user", "content": "quota" }],
    });

    let resp = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .ok()?
        .post("https://api.anthropic.com/v1/messages")
        .header("authorization", format!("Bearer {token}"))
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .ok()?;

    // The rate-limit headers ride on both 200 and 429 responses; a 401/403 means
    // the token is stale and carries nothing useful, so bail.
    if resp.status() == 401 || resp.status() == 403 {
        return None;
    }
    let h = resp.headers();

    let window = |abbrev: &str| -> Option<Window> {
        let util = header_f64(h, &format!("anthropic-ratelimit-unified-{abbrev}-utilization"))?;
        let reset = header_f64(h, &format!("anthropic-ratelimit-unified-{abbrev}-reset"))
            .unwrap_or(0.0) as i64;
        Some(Window {
            used_percent: (util * 100.0).round().clamp(0.0, 100.0) as u8,
            resets_at: reset,
        })
    };

    let five_hour = window("5h");
    let seven_day = window("7d");
    if five_hour.is_none() && seven_day.is_none() {
        return None; // not a subscription account (or no limit headers)
    }
    let status = h
        .get("anthropic-ratelimit-unified-status")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("allowed")
        .to_string();

    Some(Usage {
        five_hour,
        seven_day,
        status,
    })
}

fn header_f64(h: &reqwest::header::HeaderMap, name: &str) -> Option<f64> {
    h.get(name)?.to_str().ok()?.trim().parse().ok()
}

// ---- token discovery --------------------------------------------------------

fn find_token() -> Option<String> {
    desktop_token()
        .or_else(cli_keychain_token)
        .or_else(file_token)
}

/// Pull the OAuth token out of the Claude desktop app's encrypted token cache.
#[cfg(target_os = "macos")]
fn desktop_token() -> Option<String> {
    let cfg = dirs::home_dir()?
        .join("Library/Application Support/Claude/config.json");
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(cfg).ok()?).ok()?;
    let blob_b64 = json.get("oauth:tokenCache")?.as_str()?;
    let plaintext = safe_storage_decrypt(blob_b64)?;
    let creds: serde_json::Value = serde_json::from_slice(&plaintext).ok()?;

    // Keyed by "<account>:<org>:<baseurl>:<scopes>"; take the first live entry.
    let now_ms = now_secs() * 1000;
    for (_k, v) in creds.as_object()? {
        let token = v.get("token").and_then(|t| t.as_str());
        let exp = v.get("expiresAt").and_then(|e| e.as_i64()).unwrap_or(i64::MAX);
        if let Some(tok) = token {
            if exp > now_ms {
                return Some(tok.to_string());
            }
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn desktop_token() -> Option<String> {
    None
}

/// Decrypt a Chromium safeStorage "v10" blob (AES-128-CBC, key derived from the
/// "Claude Safe Storage" keychain password via PBKDF2-SHA1). The AES step uses
/// the system `openssl` so we don't have to pin a cipher-crate version.
#[cfg(target_os = "macos")]
fn safe_storage_decrypt(blob_b64: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    use std::io::Write;

    let blob = base64::engine::general_purpose::STANDARD
        .decode(blob_b64.trim())
        .ok()?;
    let ciphertext = blob.strip_prefix(b"v10")?;

    let password = keychain_password("Claude Safe Storage")?;
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password.as_bytes(), b"saltysalt", 1003, &mut key);
    let key_hex = hex(&key);
    let iv_hex = hex(&[0x20u8; 16]); // IV = 16 spaces, per Chromium os_crypt

    // `-nopad` so openssl doesn't choke on the cache's PKCS#7 tail; we strip it.
    let mut child = std::process::Command::new("openssl")
        .args([
            "enc", "-d", "-aes-128-cbc", "-K", &key_hex, "-iv", &iv_hex, "-nopad",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(ciphertext).ok()?;
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    let mut pt = out.stdout;
    // Strip PKCS#7 padding (last byte = pad length, 1..=16).
    if let Some(&pad) = pt.last() {
        if pad >= 1 && (pad as usize) <= pt.len().min(16) {
            pt.truncate(pt.len() - pad as usize);
        }
    }
    Some(pt)
}

#[cfg(target_os = "macos")]
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Read the standalone-CLI keychain credential (`Claude Code-credentials`).
#[cfg(target_os = "macos")]
fn cli_keychain_token() -> Option<String> {
    let raw = keychain_password("Claude Code-credentials")?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    json.get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()
        .map(String::from)
}

#[cfg(not(target_os = "macos"))]
fn cli_keychain_token() -> Option<String> {
    None
}

/// `security find-generic-password -s <service> -w` → the secret on stdout.
#[cfg(target_os = "macos")]
fn keychain_password(service: &str) -> Option<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", service, "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim_end_matches('\n').to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// `~/.claude/.credentials.json` (used when keychain storage is disabled).
fn file_token() -> Option<String> {
    let p = dirs::home_dir()?.join(".claude").join(".credentials.json");
    let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
    json.get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()
        .map(String::from)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
