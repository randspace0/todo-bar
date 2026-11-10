use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SCOPE: &str = "https://www.googleapis.com/auth/tasks";

#[derive(Clone, Debug)]
pub struct Credentials {
    pub client_id: String,
    pub client_secret: String,
}

impl Credentials {
    /// Env vars win over settings.json.
    pub fn resolve(settings_id: &str, settings_secret: &str) -> Option<Self> {
        let pick = |env: &str, fallback: &str| {
            std::env::var(env).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| fallback.to_string())
        };
        let client_id = pick("TODOBAR_GOOGLE_CLIENT_ID", settings_id);
        let client_secret = pick("TODOBAR_GOOGLE_CLIENT_SECRET", settings_secret);
        (!client_id.is_empty()).then_some(Self { client_id, client_secret })
    }
}

#[derive(Serialize, Deserialize)]
struct TokenFile {
    refresh_token: String,
}

fn token_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("TodoBar").join("google.json")
}

pub fn is_connected() -> bool {
    load_refresh_token().is_some()
}

fn load_refresh_token() -> Option<String> {
    let data = fs::read(token_path()).ok()?;
    serde_json::from_slice::<TokenFile>(&data).ok().map(|t| t.refresh_token)
}

fn save_refresh_token(token: &str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let path = token_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(&path, serde_json::to_vec(&TokenFile { refresh_token: token.to_string() })?)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

pub fn disconnect() {
    let _ = fs::remove_file(token_path());
}

fn random_string(len: usize) -> String {
    let mut s = String::new();
    while s.len() < len {
        s.push_str(&uuid::Uuid::new_v4().simple().to_string());
    }
    s.truncate(len);
    s
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn percent(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Query parameter from the first line of an HTTP request, e.g. `GET /?code=x&state=y HTTP/1.1`.
pub fn query_param(request_line: &str, key: &str) -> Option<String> {
    let target = request_line.split_whitespace().nth(1)?;
    let query = target.split_once('?')?.1;
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
}

fn token_error(e: ureq::Error) -> anyhow::Error {
    match e {
        ureq::Error::Status(code, resp) => {
            anyhow!("Google token endpoint returned {code}: {}", resp.into_string().unwrap_or_default())
        }
        other => anyhow!("Google token request failed: {other}"),
    }
}

/// Opens the browser, waits for the loopback redirect, stores the refresh token.
pub fn login(creds: &Credentials) -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr()?.port());
    let verifier = random_string(64);
    let state = random_string(24);
    let url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&state={}&access_type=offline&prompt=consent",
        percent(&creds.client_id),
        percent(&redirect),
        percent(SCOPE),
        pkce_challenge(&verifier),
        state
    );
    open::that(&url).context("could not open the browser")?;

    let deadline = Instant::now() + Duration::from_secs(180);
    let (mut stream, request) = loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let line = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string();
                // Ignore stray requests (e.g. /favicon.ico) until one carries the result.
                if query_param(&line, "code").is_some() || query_param(&line, "error").is_some() {
                    break (stream, line);
                }
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() > deadline {
                    bail!("timed out waiting for the Google sign-in");
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e.into()),
        }
    };
    let reply = |stream: &mut std::net::TcpStream, msg: &str| {
        let body = format!("<html><body style=\"font-family:sans-serif\"><h3>{msg}</h3>You can close this tab.</body></html>");
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    };
    if let Some(err) = query_param(&request, "error") {
        reply(&mut stream, "Sign-in was cancelled");
        bail!("Google sign-in failed: {err}");
    }
    if query_param(&request, "state").as_deref() != Some(state.as_str()) {
        reply(&mut stream, "Sign-in failed");
        bail!("OAuth state mismatch");
    }
    let code = query_param(&request, "code").ok_or_else(|| anyhow!("no code in redirect"))?;
    let resp: TokenResponse = ureq::post("https://oauth2.googleapis.com/token")
        .send_form(&[
            ("code", code.as_str()),
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier.as_str()),
        ])
        .map_err(token_error)?
        .into_json()?;
    let Some(refresh) = resp.refresh_token else {
        reply(&mut stream, "Sign-in failed");
        bail!("Google returned no refresh token (revoke TodoBar at myaccount.google.com/permissions and retry)");
    };
    save_refresh_token(&refresh)?;
    reply(&mut stream, "TodoBar is connected to Google Tasks");
    Ok(())
}

/// A fresh access token from the stored refresh token.
pub fn access_token(creds: &Credentials) -> Result<String> {
    let refresh = load_refresh_token().ok_or_else(|| anyhow!("not connected to Google"))?;
    let resp: TokenResponse = ureq::post("https://oauth2.googleapis.com/token")
        .send_form(&[
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("refresh_token", refresh.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .map_err(token_error)?
        .into_json()?;
    Ok(resp.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_example() {
        // RFC 7636 appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn parses_redirect_query() {
        let line = "GET /?state=abc&code=4%2F0Ab_c%20d HTTP/1.1";
        assert_eq!(query_param(line, "code").as_deref(), Some("4/0Ab_c d"));
        assert_eq!(query_param(line, "state").as_deref(), Some("abc"));
        assert_eq!(query_param("GET /favicon.ico HTTP/1.1", "code"), None);
    }
}
