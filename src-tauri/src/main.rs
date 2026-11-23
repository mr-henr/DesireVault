#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha512};
use std::{
    ffi::{c_char, c_int, c_void, CStr, CString},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    process::{Command, Stdio},
    ptr,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Manager, State};

#[repr(C)]
struct Sqlite3 {
    _private: [u8; 0],
}
#[repr(C)]
struct SqliteStmt {
    _private: [u8; 0],
}
#[link(name = "sqlite3")]
unsafe extern "C" {
    fn sqlite3_open(filename: *const c_char, db: *mut *mut Sqlite3) -> c_int;
    fn sqlite3_close(db: *mut Sqlite3) -> c_int;
    fn sqlite3_errmsg(db: *mut Sqlite3) -> *const c_char;
    fn sqlite3_exec(
        db: *mut Sqlite3,
        sql: *const c_char,
        callback: *mut c_void,
        arg: *mut c_void,
        error: *mut *mut c_char,
    ) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut Sqlite3,
        sql: *const c_char,
        n: c_int,
        stmt: *mut *mut SqliteStmt,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_bind_text(
        stmt: *mut SqliteStmt,
        index: c_int,
        text: *const c_char,
        len: c_int,
        destructor: *const c_void,
    ) -> c_int;
    fn sqlite3_bind_int64(stmt: *mut SqliteStmt, index: c_int, value: i64) -> c_int;
    fn sqlite3_step(stmt: *mut SqliteStmt) -> c_int;
    fn sqlite3_column_text(stmt: *mut SqliteStmt, col: c_int) -> *const u8;
    fn sqlite3_column_int64(stmt: *mut SqliteStmt, col: c_int) -> i64;
    fn sqlite3_finalize(stmt: *mut SqliteStmt) -> c_int;
}
const SQLITE_OK: i32 = 0;
const SQLITE_ROW: i32 = 100;
const SQLITE_DONE: i32 = 101;
const SQLITE_TRANSIENT: *const c_void = -1isize as *const c_void;

struct Db(*mut Sqlite3);
unsafe impl Send for Db {}
impl Drop for Db {
    fn drop(&mut self) {
        unsafe {
            sqlite3_close(self.0);
        }
    }
}
impl Db {
    fn open(path: &std::path::Path) -> Result<Self, String> {
        let name = CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let mut raw = ptr::null_mut();
        let code = unsafe { sqlite3_open(name.as_ptr(), &mut raw) };
        if code != SQLITE_OK {
            return Err(unsafe {
                CStr::from_ptr(sqlite3_errmsg(raw))
                    .to_string_lossy()
                    .into_owned()
            });
        }
        Ok(Self(raw))
    }
    fn error(&self) -> String {
        unsafe {
            CStr::from_ptr(sqlite3_errmsg(self.0))
                .to_string_lossy()
                .into_owned()
        }
    }
    fn exec(&self, sql: &str) -> Result<(), String> {
        let sql = CString::new(sql).map_err(|e| e.to_string())?;
        let rc = unsafe {
            sqlite3_exec(
                self.0,
                sql.as_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if rc == SQLITE_OK {
            Ok(())
        } else {
            Err(self.error())
        }
    }
    fn prepare(&self, sql: &str) -> Result<*mut SqliteStmt, String> {
        let sql = CString::new(sql).map_err(|e| e.to_string())?;
        let mut stmt = ptr::null_mut();
        let rc =
            unsafe { sqlite3_prepare_v2(self.0, sql.as_ptr(), -1, &mut stmt, ptr::null_mut()) };
        if rc == SQLITE_OK {
            Ok(stmt)
        } else {
            Err(self.error())
        }
    }
    fn bind_text(&self, stmt: *mut SqliteStmt, index: i32, value: &str) -> Result<(), String> {
        let value = CString::new(value.replace('\0', " ")).map_err(|e| e.to_string())?;
        let rc = unsafe { sqlite3_bind_text(stmt, index, value.as_ptr(), -1, SQLITE_TRANSIENT) };
        if rc == SQLITE_OK {
            Ok(())
        } else {
            Err(self.error())
        }
    }
    fn bind_int(&self, stmt: *mut SqliteStmt, index: i32, value: i64) -> Result<(), String> {
        let rc = unsafe { sqlite3_bind_int64(stmt, index, value) };
        if rc == SQLITE_OK {
            Ok(())
        } else {
            Err(self.error())
        }
    }
    fn text(stmt: *mut SqliteStmt, col: i32) -> String {
        unsafe {
            let p = sqlite3_column_text(stmt, col);
            if p.is_null() {
                String::new()
            } else {
                CStr::from_ptr(p as *const c_char)
                    .to_string_lossy()
                    .into_owned()
            }
        }
    }
    fn step(&self, stmt: *mut SqliteStmt) -> Result<i32, String> {
        let rc = unsafe { sqlite3_step(stmt) };
        if rc == SQLITE_ROW || rc == SQLITE_DONE {
            Ok(rc)
        } else {
            Err(self.error())
        }
    }
    fn finalize(stmt: *mut SqliteStmt) {
        unsafe {
            sqlite3_finalize(stmt);
        }
    }
    fn insert(&self, sql: &str, values: &[&str]) -> Result<(), String> {
        let stmt = self.prepare(sql)?;
        for (i, v) in values.iter().enumerate() {
            if let Err(e) = self.bind_text(stmt, i as i32 + 1, v) {
                Self::finalize(stmt);
                return Err(e);
            }
        }
        let result = self.step(stmt);
        Self::finalize(stmt);
        result.map(|_| ())
    }
}

struct Store(Mutex<Db>);
struct StartupNotice(Mutex<Option<String>>);

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Game {
    id: i64,
    title: String,
    status: String,
    priority: String,
    genre: String,
    year: String,
    proton: String,
    cover: String,
    developer: String,
    publisher: String,
    app_id: String,
    description: String,
    torrent_url: String,
    favorite: bool,
    added_at: String,
    updated_at: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SteamSearchResult {
    app_id: String,
    title: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProtonDbSummary {
    tier: String,
    confidence: String,
    score: f64,
    total: u64,
    trending_tier: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QbitSettings {
    host: String,
    username: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QbitTorrent {
    hash: String,
    name: String,
    state: String,
    progress: f64,
    download_speed: u64,
    upload_speed: u64,
    size: u64,
    eta: i64,
    tags: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QbitDownloadsSnapshot {
    items: Vec<QbitTorrent>,
    updated_game_ids: Vec<i64>,
}

fn http_get(url: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    if !url.starts_with("https://") {
        return Err("A Steam só pode ser consultada por HTTPS.".into());
    }
    let result = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "15",
            "--user-agent",
            "GameLibraryManager/0.1.0",
            "--proto",
            "=https",
            "--",
        ])
        .arg(url)
        .output()
        .map_err(|e| format!("Não encontrei o curl para consultar a Steam: {e}"))?;
    if !result.status.success() {
        let msg = String::from_utf8_lossy(&result.stderr).trim().to_string();
        return Err(if msg.is_empty() {
            "A Steam não respondeu à consulta.".into()
        } else {
            format!("Falha ao consultar a Steam: {msg}")
        });
    }
    if result.stdout.len() > max_bytes {
        return Err("A resposta da Steam ultrapassou o limite de segurança.".into());
    }
    Ok(result.stdout)
}
fn encode_query(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char)
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
fn parse_store_app_id(input: &str) -> Result<Option<String>, String> {
    let value = input.trim();
    if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(Some(value.to_string()));
    }
    if !(value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("store.steampowered.com/")
        || value.starts_with("www.store.steampowered.com/"))
    {
        return Ok(None);
    }
    let normalized = if value.starts_with("http") {
        value.to_string()
    } else {
        format!("https://{value}")
    };
    let rest = normalized.split_once("://").map(|(_, r)| r).unwrap_or("");
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if !matches!(
        host.to_ascii_lowercase().as_str(),
        "store.steampowered.com" | "www.store.steampowered.com"
    ) {
        return Err("Cole o link de uma página da loja store.steampowered.com.".into());
    }
    let mut parts = path.split('/');
    if parts.next() != Some("app") {
        return Err("O link precisa apontar para uma página de jogo da Steam.".into());
    }
    let id = parts.next().unwrap_or("");
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Não consegui localizar o AppID nesse link da Steam.".into());
    }
    Ok(Some(id.to_string()))
}
fn json_string(v: &Value) -> String {
    v.as_str().unwrap_or("").trim().to_string()
}
fn parse_steam_search(payload: &[u8]) -> Result<Vec<SteamSearchResult>, String> {
    let root: Value = serde_json::from_slice(payload)
        .map_err(|e| format!("A Steam retornou uma resposta inválida: {e}"))?;
    let items = root
        .get("items")
        .and_then(Value::as_array)
        .ok_or("A busca da Steam não trouxe uma lista de jogos.")?;
    Ok(items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("app"))
        .filter_map(|item| {
            let id = item.get("id")?.as_u64()?.to_string();
            let title = json_string(item.get("name")?);
            (!title.is_empty()).then_some(SteamSearchResult { app_id: id, title })
        })
        .take(12)
        .collect())
}
fn parse_steam_details(app_id: &str, payload: &[u8]) -> Result<Game, String> {
    let root: Value = serde_json::from_slice(payload)
        .map_err(|e| format!("A Steam retornou detalhes inválidos: {e}"))?;
    let entry = root
        .get(app_id)
        .ok_or("Não encontrei esse jogo na loja Steam.")?;
    if !entry
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("A Steam não encontrou esse jogo.".into());
    }
    let data = entry
        .get("data")
        .ok_or("A Steam retornou detalhes incompletos.")?;
    if data.get("type").and_then(Value::as_str) != Some("game") {
        return Err("Esse resultado da Steam não é um jogo.".into());
    }
    let title = json_string(
        data.get("name")
            .ok_or("A Steam não informou o nome do jogo.")?,
    );
    if title.is_empty() {
        return Err("A Steam não informou o nome do jogo.".into());
    }
    let join_field = |key: &str| {
        data.get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    };
    let genre = data
        .get("genres")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|g| g.get("description").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let date = data
        .get("release_date")
        .and_then(|d| d.get("date"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let year = date
        .as_bytes()
        .windows(4)
        .find(|w| w.iter().all(u8::is_ascii_digit))
        .map(|w| String::from_utf8_lossy(w).into_owned())
        .unwrap_or_default();
    Ok(Game {
        id: 0,
        title,
        status: "Na lista".into(),
        priority: "Tenho interesse".into(),
        genre,
        year,
        proton: "Sem dados".into(),
        cover: String::new(),
        developer: join_field("developers"),
        publisher: join_field("publishers"),
        app_id: app_id.into(),
        description: json_string(data.get("short_description").unwrap_or(&Value::Null)),
        torrent_url: String::new(),
        favorite: false,
        added_at: String::new(),
        updated_at: String::new(),
    })
}
fn image_data_url(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)))
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
    } else if bytes.starts_with(b"GIF8") {
        Some(format!("data:image/gif;base64,{}", STANDARD.encode(bytes)))
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(format!("data:image/webp;base64,{}", STANDARD.encode(bytes)))
    } else {
        None
    }
}
fn steam_search(query: String) -> Result<Vec<SteamSearchResult>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("Digite o nome do jogo, AppID ou link da Steam.".into());
    }
    if parse_store_app_id(query)?.is_some() {
        return Err(
            "Para importar um AppID ou link, use o campo direto e pressione Importar.".into(),
        );
    }
    let url = format!(
        "https://store.steampowered.com/api/storesearch/?term={}&l=brazilian&cc=br",
        encode_query(query)
    );
    parse_steam_search(&http_get(&url, 2_000_000)?)
}
fn steam_game_details(app_id: String) -> Result<Game, String> {
    let id = parse_store_app_id(&app_id)?.ok_or("Informe um AppID Steam ou um link da loja.")?;
    let url =
        format!("https://store.steampowered.com/api/appdetails?appids={id}&cc=br&l=brazilian");
    let payload = http_get(&url, 3_000_000)?;
    let mut game = parse_steam_details(&id, &payload)?;
    let cover_url=format!("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{id}/library_600x900.jpg");
    let cover = http_get(&cover_url, 8_000_000)
        .ok()
        .and_then(|b| image_data_url(&b));
    if let Some(image) = cover {
        game.cover = image;
    } else {
        let root: Value = serde_json::from_slice(&payload).map_err(|e| e.to_string())?;
        if let Some(header) = root
            .get(&id)
            .and_then(|x| x.get("data"))
            .and_then(|d| d.get("header_image"))
            .and_then(Value::as_str)
        {
            if let Ok(bytes) = http_get(header, 8_000_000) {
                game.cover = image_data_url(&bytes).unwrap_or_default();
            }
        }
    }
    Ok(game)
}
fn protondb_summary(app_id: String) -> Result<Option<ProtonDbSummary>, String> {
    let id = parse_store_app_id(&app_id)?
        .ok_or("Informe o AppID ou link da loja Steam para consultar o ProtonDB.")?;
    let url = format!("https://www.protondb.com/api/v1/reports/summaries/{id}.json");
    let result = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            "15",
            "--user-agent",
            "DesireVault/0.1.0",
            "--write-out",
            "\\n%{http_code}",
            "--proto",
            "=https",
            "--",
        ])
        .arg(&url)
        .output()
        .map_err(|e| format!("Não encontrei o curl para consultar o ProtonDB: {e}"))?;
    if !result.status.success() {
        let msg = String::from_utf8_lossy(&result.stderr).trim().to_string();
        return Err(if msg.is_empty() {
            "O ProtonDB não respondeu à consulta.".into()
        } else {
            format!("Falha ao consultar o ProtonDB: {msg}")
        });
    }
    if result.stdout.len() > 1_000_000 {
        return Err("A resposta do ProtonDB ultrapassou o limite de segurança.".into());
    }
    let response = String::from_utf8_lossy(&result.stdout);
    let (body, status) = response
        .trim_end()
        .rsplit_once('\n')
        .ok_or("A resposta do ProtonDB não informou o status da consulta.")?;
    if status.trim() == "404" {
        return Ok(None);
    }
    if status.trim() != "200" {
        return Err(format!(
            "O ProtonDB respondeu com status HTTP {}.",
            status.trim()
        ));
    }
    parse_protondb_summary(body.as_bytes()).map(Some)
}
fn parse_protondb_summary(payload: &[u8]) -> Result<ProtonDbSummary, String> {
    let value: Value = serde_json::from_slice(payload)
        .map_err(|e| format!("O ProtonDB retornou dados inválidos: {e}"))?;
    let tier = json_string(value.get("tier").unwrap_or(&Value::Null));
    if tier.is_empty() {
        return Err("O ProtonDB não informou uma classificação.".into());
    }
    Ok(ProtonDbSummary {
        tier,
        confidence: json_string(value.get("confidence").unwrap_or(&Value::Null)),
        score: value.get("score").and_then(Value::as_f64).unwrap_or(0.0),
        total: value.get("total").and_then(Value::as_u64).unwrap_or(0),
        trending_tier: json_string(value.get("trendingTier").unwrap_or(&Value::Null)),
    })
}
#[derive(Clone)]
struct QbitConnection {
    host: String,
    auth: QbitAuth,
}
#[derive(Clone)]
enum QbitAuth {
    Bearer(String),
    Basic { username: String, password: String },
}
fn validate_qbit_host(input: &str) -> Result<String, String> {
    let host = input.trim().trim_end_matches('/');
    let (scheme, authority) = host
        .split_once("://")
        .ok_or("Use o endereço local da Web UI, por exemplo http://127.0.0.1:8080.")?;
    if !matches!(scheme, "http" | "https")
        || authority.contains('/')
        || authority.contains('@')
        || authority.contains('?')
        || authority.contains('#')
    {
        return Err(
            "Por segurança, informe somente o endereço local da Web UI do qBittorrent.".into(),
        );
    }
    let valid = authority == "localhost"
        || authority == "127.0.0.1"
        || authority == "[::1]"
        || ["localhost:", "127.0.0.1:", "[::1]:"].iter().any(|prefix| {
            authority.strip_prefix(prefix).is_some_and(|p| {
                !p.is_empty()
                    && p.bytes().all(|b| b.is_ascii_digit())
                    && p.parse::<u16>().is_ok_and(|n| n > 0)
            })
        });
    if !valid {
        return Err(
            "A conexão do qBittorrent deve apontar para este computador (localhost ou 127.0.0.1)."
                .into(),
        );
    }
    Ok(host.to_string())
}
fn curl_quote(value: &str) -> Result<String, String> {
    if value.contains(['\n', '\r', '\0']) {
        return Err("O endereço ou credencial contém caracteres inválidos.".into());
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}
fn qbit_secret_store(key: &str) -> Result<(), String> {
    let mut child = Command::new("secret-tool")
        .args([
            "store",
            "--label=Desire Vault qBittorrent API",
            "application",
            "desire-vault",
            "service",
            "qbittorrent-api-key",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Não consegui acessar o cofre seguro do sistema: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("Não consegui abrir o cofre seguro do sistema.")?
        .write_all(format!("{key}\n").as_bytes())
        .map_err(|e| format!("Não consegui guardar a chave no cofre: {e}"))?;
    let output = child
        .wait_with_output()
        .map_err(|e| format!("Não consegui confirmar o cofre seguro: {e}"))?;
    if !output.status.success() {
        return Err("O cofre seguro do sistema não aceitou a chave do qBittorrent.".into());
    }
    Ok(())
}
fn qbit_password_store(password: &str) -> Result<(), String> {
    let mut child = Command::new("secret-tool")
        .args([
            "store",
            "--label=Desire Vault qBittorrent password",
            "application",
            "desire-vault",
            "service",
            "qbittorrent-password",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Não consegui acessar o cofre seguro do sistema: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("Não consegui abrir o cofre seguro do sistema.")?
        .write_all(format!("{password}\n").as_bytes())
        .map_err(|e| format!("Não consegui guardar a senha no cofre: {e}"))?;
    let output = child
        .wait_with_output()
        .map_err(|e| format!("Não consegui confirmar o cofre seguro: {e}"))?;
    if !output.status.success() {
        return Err("O cofre seguro do sistema não aceitou a senha do qBittorrent.".into());
    }
    Ok(())
}
fn qbit_password_read() -> Result<String, String> {
    let output = Command::new("secret-tool")
        .args([
            "lookup",
            "application",
            "desire-vault",
            "service",
            "qbittorrent-password",
        ])
        .output()
        .map_err(|e| format!("Não consegui acessar o cofre seguro do sistema: {e}"))?;
    if !output.status.success() {
        return Err("Informe seu usuário e senha do qBittorrent na tela Downloads.".into());
    }
    let password = String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string();
    if password.is_empty() {
        return Err("Informe seu usuário e senha do qBittorrent na tela Downloads.".into());
    }
    Ok(password)
}
fn qbit_api_key_read() -> Result<String, String> {
    let output = Command::new("secret-tool")
        .args([
            "lookup",
            "application",
            "desire-vault",
            "service",
            "qbittorrent-api-key",
        ])
        .output()
        .map_err(|e| format!("Não consegui acessar o cofre seguro do sistema: {e}"))?;
    if !output.status.success() {
        return Err("A chave automática do qBittorrent ainda não foi criada.".into());
    }
    let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !valid_qbit_api_key(&key) {
        return Err("A chave automática do qBittorrent está inválida.".into());
    }
    Ok(key)
}
fn valid_qbit_api_key(key: &str) -> bool {
    key.len() == 32
        && key.starts_with("qbt_")
        && key[4..].bytes().all(|b| b.is_ascii_alphanumeric())
}
fn generate_qbit_api_key() -> Result<String, String> {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut file = File::open("/dev/urandom")
        .map_err(|e| format!("Não consegui criar uma chave segura: {e}"))?;
    let mut chars = String::with_capacity(28);
    let mut bytes = [0u8; 64];
    while chars.len() < 28 {
        file.read_exact(&mut bytes)
            .map_err(|e| format!("Não consegui criar uma chave segura: {e}"))?;
        for byte in bytes {
            if byte < 248 {
                chars.push(alphabet[(byte % 62) as usize] as char);
                if chars.len() == 28 {
                    break;
                }
            }
        }
    }
    Ok(format!("qbt_{chars}"))
}
fn hmac_sha512(key: &[u8], message: &[u8]) -> [u8; 64] {
    let mut key_block = [0u8; 128];
    if key.len() > key_block.len() {
        let digest = Sha512::digest(key);
        key_block[..digest.len()].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Vec::with_capacity(128 + message.len());
    inner.extend(key_block.iter().map(|byte| byte ^ 0x36));
    inner.extend_from_slice(message);
    let inner_hash = Sha512::digest(&inner);
    let mut outer = Vec::with_capacity(128 + inner_hash.len());
    outer.extend(key_block.iter().map(|byte| byte ^ 0x5c));
    outer.extend_from_slice(&inner_hash);
    let digest = Sha512::digest(&outer);
    let mut result = [0u8; 64];
    result.copy_from_slice(&digest);
    result
}
fn pbkdf2_sha512(password: &[u8], salt: &[u8], rounds: u32) -> [u8; 64] {
    let mut first_block = Vec::with_capacity(salt.len() + 4);
    first_block.extend_from_slice(salt);
    first_block.extend_from_slice(&1u32.to_be_bytes());
    let mut current = hmac_sha512(password, &first_block);
    let mut result = current;
    for _ in 1..rounds {
        current = hmac_sha512(password, &current);
        for (out, byte) in result.iter_mut().zip(current) {
            *out ^= byte;
        }
    }
    result
}
fn generate_qbit_webui_password_hash() -> Result<(String, String), String> {
    let password = generate_qbit_api_key()?;
    let mut salt = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut salt))
        .map_err(|e| format!("Não consegui proteger a Web UI do qBittorrent: {e}"))?;
    let hash = pbkdf2_sha512(password.as_bytes(), &salt, 100_000);
    Ok((
        password,
        format!(
            "@ByteArray({}:{})",
            STANDARD.encode(salt),
            STANDARD.encode(hash)
        ),
    ))
}
fn qbit_config_path() -> Result<PathBuf, String> {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or("Não encontrei a pasta de configuração do usuário.")?;
    Ok(root.join("qBittorrent").join("qBittorrent.conf"))
}
fn is_qbit_process_name(name: &str) -> bool {
    name.to_ascii_lowercase().contains("qbittorrent")
}
fn qbit_running_process() -> Option<(String, String)> {
    fs::read_dir("/proc")
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .find_map(|entry| {
            let pid = entry.file_name().to_string_lossy().into_owned();
            if !pid.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let state = fs::read_to_string(entry.path().join("status")).ok()?;
            if state
                .lines()
                .any(|line| line.starts_with("State:") && line.contains('Z'))
            {
                return None;
            }
            let comm = fs::read_to_string(entry.path().join("comm")).ok()?;
            let executable = fs::read_link(entry.path().join("exe"))
                .ok()
                .and_then(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                });
            let name = [Some(comm.trim().to_string()), executable]
                .into_iter()
                .flatten()
                .find(|name| is_qbit_process_name(name))?;
            Some((pid, name))
        })
}
fn stop_qbittorrent_if_running() -> Result<(), String> {
    let Some((pid, process)) = qbit_running_process() else {
        return Ok(());
    };
    let status = Command::new("kill")
        .args(["-TERM", &pid])
        .status()
        .map_err(|e| format!("Não consegui reiniciar o qBittorrent automaticamente: {e}"))?;
    if !status.success() {
        return Err(format!(
            "Não consegui encerrar qBittorrent ({process}) para preparar a integração."
        ));
    }
    for _ in 0..200 {
        if qbit_running_process().is_none() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    Err("O qBittorrent não encerrou a tempo para aplicar a configuração automática. Tente novamente.".into())
}
fn start_qbittorrent(config_path: &std::path::Path) -> Result<(), String> {
    if config_path.exists() {
        let original = fs::read_to_string(config_path)
            .map_err(|e| format!("Não consegui ler as preferências do qBittorrent: {e}"))?;
        let updated = update_qbittorrent_window_preferences(&original);
        if updated != original {
            write_qbit_config(config_path, &updated)?;
        }
    }
    let config_root = config_path
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("Não consegui determinar a pasta de configuração do qBittorrent.")?;
    Command::new("qbittorrent")
        .env("XDG_CONFIG_HOME", config_root)
        .arg("--no-splash")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Não consegui iniciar o qBittorrent em segundo plano: {e}"))
}
fn qbit_ini_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        line.split_once('=')
            .filter(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().trim_matches('"').to_string())
    })
}
fn qbit_ini_section_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_section = line[1..line.len() - 1].eq_ignore_ascii_case(section);
            continue;
        }
        if in_section {
            if let Some((name, value)) = line.split_once('=') {
                if name.trim() == key {
                    return Some(value.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    None
}
fn set_qbit_ini_section_values(text: &str, section: &str, changes: &[(&str, &str)]) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let header = format!("[{section}]");
    let start = lines
        .iter()
        .position(|line| line.trim().eq_ignore_ascii_case(&header));
    let start = match start {
        Some(index) => index,
        None => {
            if !lines.is_empty() && !lines.last().is_some_and(String::is_empty) {
                lines.push(String::new());
            }
            lines.push(header);
            lines.len() - 1
        }
    };
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, line)| line.trim().starts_with('['))
        .map(|(index, _)| index)
        .unwrap_or(lines.len());
    let mut found = vec![false; changes.len()];
    for line in lines.iter_mut().take(end).skip(start + 1) {
        if let Some((name, _)) = line.trim().split_once('=') {
            if let Some((index, (key, value))) = changes
                .iter()
                .enumerate()
                .find(|(_, (key, _))| *key == name.trim())
            {
                *line = format!("{key}={value}");
                found[index] = true;
            }
        }
    }
    let mut additions = Vec::new();
    for (index, (key, value)) in changes.iter().enumerate() {
        if !found[index] {
            additions.push(format!("{key}={value}"));
        }
    }
    lines.splice(end..end, additions);
    let mut output = lines.join("\n");
    output.push('\n');
    output
}
fn update_qbittorrent_window_preferences(text: &str) -> String {
    let preferences = set_qbit_ini_section_values(
        text,
        "Preferences",
        &[
            ("General\\StartMinimized", "true"),
            ("General\\SystrayEnabled", "true"),
            ("General\\MinimizeToTray", "true"),
        ],
    );
    set_qbit_ini_section_values(&preferences, "GUI", &[("StartUpWindowState", "Minimized")])
}
fn choose_qbit_webui_port(config: &str) -> Result<u16, String> {
    let requested = qbit_ini_value(config, "WebUI\\Port")
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(8080);
    if std::net::TcpListener::bind(("127.0.0.1", requested)).is_ok() {
        return Ok(requested);
    }
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| format!("Não encontrei uma porta local livre para a Web UI: {e}"))?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|e| format!("Não consegui selecionar uma porta local: {e}"))
}
fn update_qbit_ini(text: &str, key: &str, port: u16, password_hash: &str) -> String {
    let changes = [
        ("WebUI\\Enabled", "true".to_string()),
        ("WebUI\\Address", "127.0.0.1".to_string()),
        ("WebUI\\Port", port.to_string()),
        ("WebUI\\APIKey", key.to_string()),
        ("WebUI\\Username", "admin".to_string()),
        ("WebUI\\Password_PBKDF2", format!("\"{password_hash}\"")),
        ("WebUI\\LocalHostAuth", "true".to_string()),
        ("WebUI\\HTTPS\\Enabled", "false".to_string()),
        ("WebUI\\UseUPnP", "false".to_string()),
        ("WebUI\\CSRFProtection", "true".to_string()),
        ("WebUI\\HostHeaderValidation", "true".to_string()),
        ("General\\StartMinimized", "true".to_string()),
        ("General\\SystrayEnabled", "true".to_string()),
        ("General\\MinimizeToTray", "true".to_string()),
    ];
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let start = lines
        .iter()
        .position(|line| line.trim().eq_ignore_ascii_case("[Preferences]"));
    if let Some(start) = start {
        let end = lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, line)| line.trim().starts_with('['))
            .map(|(i, _)| i)
            .unwrap_or(lines.len());
        let mut found = vec![false; changes.len()];
        for line in lines.iter_mut().take(end).skip(start + 1) {
            let trimmed = line.trim();
            if let Some((name, _)) = trimmed.split_once('=') {
                if let Some((i, (key, value))) = changes
                    .iter()
                    .enumerate()
                    .find(|(_, (k, _))| *k == name.trim())
                {
                    *line = format!("{key}={value}");
                    found[i] = true;
                }
            }
        }
        let mut missing = Vec::new();
        for (i, (name, value)) in changes.iter().enumerate() {
            if !found[i] {
                missing.push(format!("{name}={value}"));
            }
        }
        lines.splice(end..end, missing);
    } else {
        if !lines.is_empty() && !lines.last().is_some_and(String::is_empty) {
            lines.push(String::new());
        }
        lines.push("[Preferences]".into());
        for (name, value) in changes {
            lines.push(format!("{name}={value}"));
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}
fn write_qbit_config(path: &std::path::Path, contents: &str) -> Result<(), String> {
    let parent = path.parent().ok_or("Pasta de configuração inválida.")?;
    fs::create_dir_all(parent)
        .map_err(|e| format!("Não consegui criar a pasta de configuração: {e}"))?;
    let temp = parent.join(format!(".qBittorrent.conf.{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|e| format!("Não consegui preparar a configuração segura do qBittorrent: {e}"))?;
    if let Err(e) = file
        .write_all(contents.as_bytes())
        .and_then(|_| file.sync_all())
    {
        let _ = fs::remove_file(&temp);
        return Err(format!(
            "Não consegui gravar a configuração do qBittorrent: {e}"
        ));
    }
    fs::rename(&temp, path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("Não consegui atualizar a configuração do qBittorrent: {e}")
    })?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("Não consegui proteger a configuração do qBittorrent: {e}"))?;
    Ok(())
}
fn qbit_connection(db: &Db) -> Result<QbitConnection, String> {
    let get = |key: &str| -> Result<String, String> {
        let s = db.prepare("SELECT value FROM app_settings WHERE key=?1")?;
        db.bind_text(s, 1, key)?;
        let value = if db.step(s)? == SQLITE_ROW {
            Db::text(s, 0)
        } else {
            String::new()
        };
        Db::finalize(s);
        Ok(value)
    };
    let host = get("qbit_host")?;
    let username = get("qbit_username")?;
    if host.is_empty() || username.is_empty() {
        return Err(
            "Conecte ao qBittorrent informando seu usuário e senha na tela Downloads.".into(),
        );
    }
    // Prefer a previously saved login before an older API key. The user may
    // have changed qBittorrent credentials outside the app since that key was saved.
    let auth = if let Ok(password) = qbit_password_read() {
        QbitAuth::Basic { username, password }
    } else {
        QbitAuth::Bearer(qbit_api_key_read()?)
    };
    if matches!(&auth, QbitAuth::Basic { password, .. } if password.is_empty()) {
        return Err("A conexão segura com o qBittorrent precisa ser preparada novamente.".into());
    }
    Ok(QbitConnection {
        host: validate_qbit_host(&host)?,
        auth,
    })
}
fn qbit_request(
    connection: &QbitConnection,
    path: &str,
    form: Option<&[(&str, String)]>,
) -> Result<Vec<u8>, String> {
    let url = format!("{}{path}", connection.host);
    let referer = format!("{}/", connection.host);
    let authorization = match &connection.auth {
        QbitAuth::Bearer(key) => format!("Bearer {key}"),
        QbitAuth::Basic { username, password } => {
            format!(
                "Basic {}",
                STANDARD.encode(format!("{username}:{password}"))
            )
        }
    };
    let mut config = format!(
        "url = {}\nheader = {}\nreferer = {}\n",
        curl_quote(&url)?,
        curl_quote(&format!("Authorization: {authorization}"))?,
        curl_quote(&referer)?
    );
    if let Some(fields) = form {
        config.push_str("request = \"POST\"\n");
        for (key, value) in fields {
            config.push_str(&format!(
                "data-urlencode = {}\n",
                curl_quote(&format!("{key}={value}"))?
            ));
        }
    }
    let mut child = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "8",
            "--write-out",
            "\\n%{http_code}",
            "--proto",
            "=http,https",
            "--config",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Não encontrei o curl para conectar ao qBittorrent: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("Não consegui abrir a conexão com o qBittorrent.")?
        .write_all(config.as_bytes())
        .map_err(|e| format!("Falha ao enviar a solicitação ao qBittorrent: {e}"))?;
    let output = child
        .wait_with_output()
        .map_err(|e| format!("A conexão com o qBittorrent falhou: {e}"))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(
            if message.contains("Failed to connect") || message.contains("Connection refused") {
                format!("Não há Web UI respondendo em {}. Confira se o qBittorrent abriu a interface local.", connection.host)
            } else {
                format!("Falha de comunicação com o qBittorrent: {message}")
            },
        );
    }
    let response = String::from_utf8_lossy(&output.stdout);
    let (body, status) = response
        .trim_end()
        .rsplit_once('\n')
        .ok_or("O qBittorrent retornou uma resposta incompleta.")?;
    let code = status.trim();
    if code == "401" || code == "403" {
        return Err(
            "O qBittorrent recusou a autenticação. Confira o usuário e a senha da Interface Web."
                .into(),
        );
    }
    if !code.starts_with('2') {
        return Err(format!(
            "A Web UI do qBittorrent respondeu com HTTP {code}."
        ));
    }
    Ok(body.as_bytes().to_vec())
}
fn parse_qbit_torrents(payload: &[u8]) -> Result<Vec<QbitTorrent>, String> {
    let values: Value = serde_json::from_slice(payload)
        .map_err(|e| format!("A lista do qBittorrent veio em formato inválido: {e}"))?;
    let rows = values
        .as_array()
        .ok_or("O qBittorrent não retornou a lista de downloads.")?;
    Ok(rows
        .iter()
        .map(|row| QbitTorrent {
            hash: json_string(row.get("hash").unwrap_or(&Value::Null)),
            name: json_string(row.get("name").unwrap_or(&Value::Null)),
            state: json_string(row.get("state").unwrap_or(&Value::Null)),
            progress: row.get("progress").and_then(Value::as_f64).unwrap_or(0.0),
            download_speed: row.get("dlspeed").and_then(Value::as_u64).unwrap_or(0),
            upload_speed: row.get("upspeed").and_then(Value::as_u64).unwrap_or(0),
            size: row.get("size").and_then(Value::as_u64).unwrap_or(0),
            eta: row.get("eta").and_then(Value::as_i64).unwrap_or(-1),
            tags: json_string(row.get("tags").unwrap_or(&Value::Null)),
        })
        .collect())
}
fn set_game_status(db: &Db, id: i64, status: &str) -> Result<(), String> {
    let stmt = db.prepare("UPDATE games SET status=?1,updated_at=CURRENT_TIMESTAMP WHERE id=?2")?;
    if let Err(error) = db
        .bind_text(stmt, 1, status)
        .and_then(|_| db.bind_int(stmt, 2, id))
    {
        Db::finalize(stmt);
        return Err(error);
    }
    let result = db.step(stmt);
    Db::finalize(stmt);
    result.map(|_| ())
}
fn sync_qbittorrent_game_statuses(db: &Db, torrents: &[QbitTorrent]) -> Result<Vec<i64>, String> {
    let stmt = db.prepare("SELECT id FROM games WHERE status='Baixando'")?;
    let mut game_ids = Vec::new();
    loop {
        match db.step(stmt)? {
            SQLITE_ROW => game_ids.push(unsafe { sqlite3_column_int64(stmt, 0) }),
            SQLITE_DONE => break,
            _ => unreachable!(),
        }
    }
    Db::finalize(stmt);
    let mut changed = Vec::new();
    for id in game_ids {
        let tag = format!("desire-vault-game-{id}");
        let torrent = torrents
            .iter()
            .find(|torrent| torrent.tags.split(',').any(|item| item.trim() == tag));
        let next_status = match torrent {
            Some(torrent) if torrent.progress >= 1.0 => "Baixado",
            Some(_) => "Baixando",
            None => "Na lista",
        };
        if next_status != "Baixando" {
            set_game_status(db, id, next_status)?;
            changed.push(id);
        }
    }
    Ok(changed)
}
fn validate_torrent_url(input: &str) -> Result<String, String> {
    let value = input.trim();
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("magnet:?xt=urn:btih:") && value.len() > 30 {
        return Ok(value.to_string());
    }
    if (lower.starts_with("https://") || lower.starts_with("http://"))
        && lower
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .ends_with(".torrent")
    {
        return Ok(value.to_string());
    }
    Err("Informe um link Magnet válido ou um link HTTP/HTTPS terminado em .torrent.".into())
}
fn db_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("library.sqlite3"))
}
fn initialize(app: &tauri::AppHandle) -> Result<Db, String> {
    let path = db_path(app)?;
    let db = Db::open(&path)?;
    db.exec("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS games(id INTEGER PRIMARY KEY AUTOINCREMENT,title TEXT NOT NULL,status TEXT NOT NULL DEFAULT 'Na lista',priority TEXT NOT NULL DEFAULT 'Tenho interesse',genre TEXT NOT NULL DEFAULT '',year TEXT NOT NULL DEFAULT '',proton TEXT NOT NULL DEFAULT 'Sem dados',cover TEXT NOT NULL DEFAULT '',developer TEXT NOT NULL DEFAULT '',publisher TEXT NOT NULL DEFAULT '',app_id TEXT NOT NULL DEFAULT '',description TEXT NOT NULL DEFAULT '',torrent_url TEXT NOT NULL DEFAULT '',favorite INTEGER NOT NULL DEFAULT 0,added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP); CREATE TABLE IF NOT EXISTS app_settings(key TEXT PRIMARY KEY,value TEXT NOT NULL DEFAULT '');")?;
    ensure_column(&db, "torrent_url", "TEXT NOT NULL DEFAULT ''")?;
    cleanup_demo_entries(&db)?;
    Ok(db)
}
fn ensure_column(db: &Db, name: &str, definition: &str) -> Result<(), String> {
    let stmt = db.prepare("PRAGMA table_info(games)")?;
    let mut found = false;
    loop {
        match db.step(stmt)? {
            SQLITE_ROW => {
                if Db::text(stmt, 1) == name {
                    found = true
                }
            }
            SQLITE_DONE => break,
            _ => unreachable!(),
        }
    }
    Db::finalize(stmt);
    if !found {
        db.exec(&format!("ALTER TABLE games ADD COLUMN {name} {definition}"))?;
    }
    Ok(())
}
fn cleanup_demo_entries(db: &Db) -> Result<(), String> {
    db.exec("BEGIN IMMEDIATE; DELETE FROM games WHERE cover LIKE 'demo:%' AND app_id=''; DROP TABLE IF EXISTS app_state; COMMIT;")
}
const SELECT:&str="SELECT id,title,status,priority,genre,year,proton,cover,developer,publisher,app_id,description,torrent_url,favorite,added_at,updated_at FROM games";
fn read_game(s: *mut SqliteStmt) -> Game {
    Game {
        id: unsafe { sqlite3_column_int64(s, 0) },
        title: Db::text(s, 1),
        status: Db::text(s, 2),
        priority: Db::text(s, 3),
        genre: Db::text(s, 4),
        year: Db::text(s, 5),
        proton: Db::text(s, 6),
        cover: Db::text(s, 7),
        developer: Db::text(s, 8),
        publisher: Db::text(s, 9),
        app_id: Db::text(s, 10),
        description: Db::text(s, 11),
        torrent_url: Db::text(s, 12),
        favorite: unsafe { sqlite3_column_int64(s, 13) } != 0,
        added_at: Db::text(s, 14),
        updated_at: Db::text(s, 15),
    }
}
#[tauri::command]
fn list_games(
    store: State<'_, Store>,
    status: Option<String>,
    query: Option<String>,
    sort: Option<String>,
    favorites: Option<bool>,
) -> Result<Vec<Game>, String> {
    let db = store.0.lock().map_err(|e| e.to_string())?;
    let order=match sort.as_deref().unwrap_or("title"){"recent"=>"added_at DESC","year"=>"year DESC,title COLLATE NOCASE","priority"=>"CASE priority WHEN 'Quero muito jogar' THEN 0 WHEN 'Tenho interesse' THEN 1 ELSE 2 END,title COLLATE NOCASE",_=>"title COLLATE NOCASE"};
    let sql=format!("{SELECT} WHERE (?1='Todos' OR ?1='' OR status=?1) AND (?2='%%' OR title LIKE ?2 OR genre LIKE ?2 OR developer LIKE ?2) AND (?3=0 OR favorite=1) ORDER BY {order}");
    let stmt = db.prepare(&sql)?;
    db.bind_text(stmt, 1, &status.unwrap_or_else(|| "Todos".into()))?;
    db.bind_text(stmt, 2, &format!("%{}%", query.unwrap_or_default()))?;
    db.bind_int(stmt, 3, favorites.unwrap_or(false) as i64)?;
    let mut out = Vec::new();
    loop {
        match db.step(stmt)? {
            SQLITE_ROW => out.push(read_game(stmt)),
            SQLITE_DONE => break,
            _ => unreachable!(),
        }
    }
    Db::finalize(stmt);
    Ok(out)
}
fn library_backup_value(db: &Db) -> Result<Value, String> {
    let stmt = db.prepare(&format!("{SELECT} ORDER BY id"))?;
    let mut games = Vec::new();
    loop {
        match db.step(stmt)? {
            SQLITE_ROW => games.push(read_game(stmt)),
            SQLITE_DONE => break,
            _ => unreachable!(),
        }
    }
    Db::finalize(stmt);
    Ok(
        serde_json::json!({"format":"desire-vault-library","version":1,"exportedAt":SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),"games":games}),
    )
}
fn restore_games_transaction(db: &Db, games: &[Game]) -> Result<(), String> {
    db.exec("BEGIN IMMEDIATE; DELETE FROM games;")?;
    let insert_result = (|| -> Result<(), String> {
        for game in games {
            let stmt = db.prepare("INSERT INTO games(id,title,status,priority,genre,year,proton,cover,developer,publisher,app_id,description,torrent_url,favorite,added_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)")?;
            let fields = [
                &game.title,
                &game.status,
                &game.priority,
                &game.genre,
                &game.year,
                &game.proton,
                &game.cover,
                &game.developer,
                &game.publisher,
                &game.app_id,
                &game.description,
                &game.torrent_url,
            ];
            let mut result = db.bind_int(stmt, 1, game.id);
            for (index, field) in fields.iter().enumerate() {
                if result.is_ok() {
                    result = db.bind_text(stmt, index as i32 + 2, field);
                }
            }
            if result.is_ok() {
                result = db.bind_int(stmt, 14, game.favorite as i64);
            }
            if result.is_ok() {
                result = db.bind_text(stmt, 15, &game.added_at);
            }
            if result.is_ok() {
                result = db.bind_text(stmt, 16, &game.updated_at);
            }
            if result.is_ok() {
                result = db.step(stmt).map(|_| ());
            }
            Db::finalize(stmt);
            result?;
        }
        Ok(())
    })();
    if let Err(error) = insert_result {
        let _ = db.exec("ROLLBACK;");
        return Err(format!(
            "Restauração cancelada sem alterar a biblioteca: {error}"
        ));
    }
    db.exec("COMMIT;")
}
#[tauri::command]
fn export_library_backup(store: State<'_, Store>) -> Result<Value, String> {
    let db = store.0.lock().map_err(|e| e.to_string())?;
    library_backup_value(&db)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedBackup {
    path: String,
    games: usize,
}
#[tauri::command]
fn save_library_backup(
    store: State<'_, Store>,
    suggested_name: String,
) -> Result<Option<SavedBackup>, String> {
    let backup = {
        let db = store.0.lock().map_err(|e| e.to_string())?;
        library_backup_value(&db)?
    };
    let suggested_name = PathBuf::from(suggested_name)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "DesireVault-backup.json".into());
    let Some(mut path) = rfd::FileDialog::new()
        .set_title("Salvar backup do Desire Vault")
        .set_file_name(suggested_name)
        .add_filter("Arquivo JSON", &["json"])
        .save_file()
    else {
        return Ok(None);
    };
    if path.extension().is_none() {
        path.set_extension("json");
    }
    let games = backup
        .get("games")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let bytes = serde_json::to_vec_pretty(&backup).map_err(|e| e.to_string())?;
    let file_name = path
        .file_name()
        .ok_or("O destino escolhido não tem um nome de arquivo válido.")?;
    let temp = path.with_file_name(format!(
        ".{}.tmp-{}",
        file_name.to_string_lossy(),
        std::process::id()
    ));
    fs::write(&temp, bytes).map_err(|e| format!("Não consegui gravar o backup: {e}"))?;
    #[cfg(unix)]
    fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("Não consegui proteger o arquivo de backup: {e}"))?;
    if let Err(error) = fs::rename(&temp, &path) {
        let _ = fs::remove_file(&temp);
        return Err(format!(
            "Não consegui finalizar o arquivo de backup: {error}"
        ));
    }
    Ok(Some(SavedBackup {
        path: path.to_string_lossy().into_owned(),
        games,
    }))
}
#[tauri::command]
fn restore_library_backup(
    app: tauri::AppHandle,
    store: State<'_, Store>,
    backup: Value,
) -> Result<usize, String> {
    let db = store.0.lock().map_err(|e| e.to_string())?;
    restore_backup_with_db(&app, &db, backup)
}
fn restore_backup_with_db(app: &tauri::AppHandle, db: &Db, backup: Value) -> Result<usize, String> {
    if backup.get("format").and_then(Value::as_str) != Some("desire-vault-library")
        || backup.get("version").and_then(Value::as_u64) != Some(1)
    {
        return Err("Este arquivo não é um backup compatível do Desire Vault.".into());
    }
    let games: Vec<Game> = serde_json::from_value(
        backup
            .get("games")
            .cloned()
            .ok_or("O backup não contém a biblioteca.")?,
    )
    .map_err(|e| format!("Não consegui ler a biblioteca do backup: {e}"))?;
    if games.len() > 100_000
        || games.iter().any(|game| {
            game.id < 1
                || game.title.trim().is_empty()
                || game.title.len() > 1024 * 1024
                || game.cover.len() > 64 * 1024 * 1024
        })
    {
        return Err("O backup contém dados inválidos ou grandes demais.".into());
    }
    let current = library_backup_value(db)?;
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    let safety = data_dir.join(format!(
        "pre-restore-{}.json",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    ));
    fs::write(
        &safety,
        serde_json::to_vec(&current).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Não consegui criar a cópia de segurança antes da restauração: {e}"))?;
    fs::set_permissions(&safety, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    restore_games_transaction(db, &games)?;
    Ok(games.len())
}
fn restore_pending_install_backup(app: &tauri::AppHandle, db: &Db) -> Option<String> {
    let data_dir = app.path().app_data_dir().ok()?;
    let pending = data_dir.join("pending-install-restore.json");
    if !pending.is_file() {
        return None;
    }
    let result = (|| {
        let bytes = fs::read(&pending).map_err(|e| e.to_string())?;
        if bytes.len() > 128 * 1024 * 1024 {
            return Err("o arquivo excede o limite de 128 MB".to_string());
        }
        let backup: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let count = restore_backup_with_db(app, db, backup)?;
        fs::remove_file(&pending).map_err(|e| e.to_string())?;
        Ok::<usize, String>(count)
    })();
    match result {
        Ok(count) => Some(format!(
            "Backup restaurado automaticamente: {count} {}.",
            if count == 1 { "jogo" } else { "jogos" }
        )),
        Err(error) => {
            let failed = data_dir.join("pending-install-restore.failed.json");
            let _ = fs::rename(&pending, &failed);
            Some(format!(
                "A restauração automática não foi concluída: {error}. O backup foi mantido em {}.",
                failed.display()
            ))
        }
    }
}
#[tauri::command]
fn take_startup_notice(notice: State<'_, StartupNotice>) -> Result<Option<String>, String> {
    Ok(notice.0.lock().map_err(|e| e.to_string())?.take())
}
#[tauri::command]
fn save_game_record(db: &Db, game: Game) -> Result<Game, String> {
    let sql = if game.id == 0 {
        "INSERT INTO games(title,status,priority,genre,year,proton,cover,developer,publisher,app_id,description,torrent_url,favorite) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)"
    } else {
        "UPDATE games SET title=?1,status=?2,priority=?3,genre=?4,year=?5,proton=?6,cover=?7,developer=?8,publisher=?9,app_id=?10,description=?11,torrent_url=?12,favorite=?13,updated_at=CURRENT_TIMESTAMP WHERE id=?14"
    };
    let stmt = db.prepare(sql)?;
    let vals = [
        &game.title,
        &game.status,
        &game.priority,
        &game.genre,
        &game.year,
        &game.proton,
        &game.cover,
        &game.developer,
        &game.publisher,
        &game.app_id,
        &game.description,
        &game.torrent_url,
    ];
    for (i, v) in vals.iter().enumerate() {
        if let Err(e) = db.bind_text(stmt, i as i32 + 1, v) {
            Db::finalize(stmt);
            return Err(e);
        }
    }
    if let Err(e) = db.bind_int(stmt, 13, game.favorite as i64) {
        Db::finalize(stmt);
        return Err(e);
    }
    if game.id != 0 {
        if let Err(e) = db.bind_int(stmt, 14, game.id) {
            Db::finalize(stmt);
            return Err(e);
        }
    }
    if let Err(e) = db.step(stmt) {
        Db::finalize(stmt);
        return Err(e);
    }
    Db::finalize(stmt);
    let id = if game.id == 0 {
        unsafe {
            extern "C" {
                fn sqlite3_last_insert_rowid(db: *mut Sqlite3) -> i64;
            }
            sqlite3_last_insert_rowid(db.0)
        }
    } else {
        game.id
    };
    let query = db.prepare(&format!("{SELECT} WHERE id=?1"))?;
    db.bind_int(query, 1, id)?;
    let result = if db.step(query)? == SQLITE_ROW {
        read_game(query)
    } else {
        return Err(db.error());
    };
    Db::finalize(query);
    Ok(result)
}
#[tauri::command]
fn save_game(store: State<'_, Store>, game: Game) -> Result<Game, String> {
    let db = store.0.lock().map_err(|e| e.to_string())?;
    save_game_record(&db, game)
}
#[tauri::command]
fn delete_game(store: State<'_, Store>, id: i64) -> Result<(), String> {
    let db = store.0.lock().map_err(|e| e.to_string())?;
    let s = db.prepare("DELETE FROM games WHERE id=?1")?;
    db.bind_int(s, 1, id)?;
    let r = db.step(s);
    Db::finalize(s);
    r.map(|_| ())
}
#[tauri::command]
fn set_favorite(store: State<'_, Store>, id: i64, value: bool) -> Result<(), String> {
    let db = store.0.lock().map_err(|e| e.to_string())?;
    let s = db.prepare("UPDATE games SET favorite=?1 WHERE id=?2")?;
    db.bind_int(s, 1, value as i64)?;
    db.bind_int(s, 2, id)?;
    let r = db.step(s);
    Db::finalize(s);
    r.map(|_| ())
}
#[tauri::command]
fn search_steam_games(query: String) -> Result<Vec<SteamSearchResult>, String> {
    steam_search(query)
}
#[tauri::command]
fn get_steam_game(app_id: String) -> Result<Game, String> {
    steam_game_details(app_id)
}
#[tauri::command]
fn open_steam_page(app_id: String) -> Result<(), String> {
    let id = parse_store_app_id(&app_id)?.ok_or("AppID Steam inválido.")?;
    Command::new("xdg-open")
        .arg(format!("https://store.steampowered.com/app/{id}/"))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Não consegui abrir o navegador: {e}"))
}
#[tauri::command]
fn find_steam_game(store: State<'_, Store>, app_id: String) -> Result<Option<Game>, String> {
    let db = store.0.lock().map_err(|e| e.to_string())?;
    let s = db.prepare(&format!("{SELECT} WHERE app_id=?1 LIMIT 1"))?;
    db.bind_text(s, 1, &app_id)?;
    let result = if db.step(s)? == SQLITE_ROW {
        Some(read_game(s))
    } else {
        None
    };
    Db::finalize(s);
    Ok(result)
}
#[tauri::command]
fn get_protondb_summary(app_id: String) -> Result<Option<ProtonDbSummary>, String> {
    protondb_summary(app_id)
}
#[tauri::command]
fn open_protondb_page(app_id: String) -> Result<(), String> {
    let id = parse_store_app_id(&app_id)?.ok_or("AppID Steam inválido.")?;
    Command::new("xdg-open")
        .arg(format!("https://www.protondb.com/app/{id}"))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Não consegui abrir o ProtonDB: {e}"))
}
#[tauri::command]
fn get_qbittorrent_settings(store: State<'_, Store>) -> Result<Option<QbitSettings>, String> {
    let db = store.0.lock().map_err(|e| e.to_string())?;
    let get = |key: &str| -> Result<String, String> {
        let s = db.prepare("SELECT value FROM app_settings WHERE key=?1")?;
        db.bind_text(s, 1, key)?;
        let value = if db.step(s)? == SQLITE_ROW {
            Db::text(s, 0)
        } else {
            String::new()
        };
        Db::finalize(s);
        Ok(value)
    };
    let host = get("qbit_host")?;
    let username = get("qbit_username")?;
    if host.is_empty() || username.is_empty() {
        Ok(None)
    } else {
        Ok(Some(QbitSettings { host, username }))
    }
}
#[tauri::command]
fn configure_qbittorrent(
    store: State<'_, Store>,
    host: String,
    username: String,
    password: String,
) -> Result<String, String> {
    let host = validate_qbit_host(&host)?;
    let username = username.trim();
    if username.is_empty()
        || username.chars().any(|c| matches!(c, '\n' | '\r' | '\0'))
        || password.len() < 6
        || password.chars().any(|c| matches!(c, '\n' | '\r' | '\0'))
    {
        return Err("Informe o usuário e a senha da Interface Web do qBittorrent.".into());
    }
    let connection = QbitConnection {
        host: host.clone(),
        auth: QbitAuth::Basic {
            username: username.to_string(),
            password: password.clone(),
        },
    };
    let version = qbit_request(&connection, "/api/v2/app/version", None)?;
    let version = String::from_utf8_lossy(&version).trim().to_string();
    if version.is_empty() {
        return Err("O qBittorrent respondeu sem informar a versão da Web API.".into());
    }
    qbit_password_store(&password)?;
    let db = store.0.lock().map_err(|e| e.to_string())?;
    db.insert("INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", &["qbit_host", &host])?;
    db.insert("INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", &["qbit_username", username])?;
    Ok(version)
}
#[tauri::command]
fn enable_qbittorrent_webui(store: State<'_, Store>) -> Result<String, String> {
    stop_qbittorrent_if_running()?;
    let path = qbit_config_path()?;
    let original = fs::read_to_string(&path).unwrap_or_default();
    let port = choose_qbit_webui_port(&original)?;
    let host = format!("http://127.0.0.1:{port}");
    let key = qbit_ini_value(&original, "WebUI\\APIKey")
        .filter(|v| valid_qbit_api_key(v))
        .unwrap_or(generate_qbit_api_key()?);
    let (webui_password, password_hash) = generate_qbit_webui_password_hash()?;
    let contents = update_qbit_ini(&original, &key, port, &password_hash);
    if path.exists() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let backup = path.with_file_name(format!("qBittorrent.conf.backup.{stamp}"));
        fs::copy(&path, &backup)
            .map_err(|e| format!("Não consegui criar cópia de segurança da configuração: {e}"))?;
        fs::set_permissions(&backup, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Não consegui proteger a cópia de segurança: {e}"))?;
    }
    write_qbit_config(&path, &contents)?;
    if let Err(error) = qbit_secret_store(&key) {
        if path.exists() {
            if original.is_empty() {
                let _ = fs::remove_file(&path);
            } else {
                let _ = write_qbit_config(&path, &original);
            }
        }
        return Err(error);
    }
    if let Err(error) = qbit_password_store(&webui_password) {
        if original.is_empty() {
            let _ = fs::remove_file(&path);
        } else {
            let _ = write_qbit_config(&path, &original);
        }
        return Err(error);
    }
    let db = store.0.lock().map_err(|e| e.to_string())?;
    db.insert("INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",&["qbit_host",&host])?;
    db.insert("INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",&["qbit_username","admin"])?;
    drop(db);
    start_qbittorrent(&path)?;
    let connection = QbitConnection {
        host,
        auth: QbitAuth::Bearer(key),
    };
    let mut last_error = String::new();
    for _ in 0..12 {
        std::thread::sleep(std::time::Duration::from_millis(750));
        match qbit_request(&connection, "/api/v2/app/version", None) {
            Ok(version) => {
                let version = String::from_utf8_lossy(&version).trim().to_string();
                if !version.is_empty() {
                    return Ok(version);
                }
            }
            Err(error) => last_error = error,
        }
    }
    Err(format!("O qBittorrent abriu, mas a Web API não respondeu em {}. {last_error} Confira o registro de execução do qBittorrent para mensagens de inicialização da Web UI.", connection.host))
}
#[tauri::command]
fn ensure_qbittorrent_ready(store: State<'_, Store>) -> Result<String, String> {
    let saved_connection = {
        let db = store.0.lock().map_err(|e| e.to_string())?;
        qbit_connection(&db).ok()
    };
    if let Some(connection) = saved_connection {
        if let Ok(version) = qbit_request(&connection, "/api/v2/app/version", None) {
            let version = String::from_utf8_lossy(&version).trim().to_string();
            if !version.is_empty() {
                if qbit_running_process().is_some() {
                    let path = qbit_config_path()?;
                    let original = fs::read_to_string(&path).unwrap_or_default();
                    if qbit_ini_section_value(&original, "GUI", "StartUpWindowState").as_deref()
                        != Some("Minimized")
                    {
                        stop_qbittorrent_if_running()?;
                        let latest = fs::read_to_string(&path).unwrap_or_default();
                        let updated = update_qbittorrent_window_preferences(&latest);
                        if updated != latest {
                            write_qbit_config(&path, &updated)?;
                        }
                        start_qbittorrent(&path)?;
                        for _ in 0..24 {
                            std::thread::sleep(std::time::Duration::from_millis(500));
                            if let Ok(response) =
                                qbit_request(&connection, "/api/v2/app/version", None)
                            {
                                let response =
                                    String::from_utf8_lossy(&response).trim().to_string();
                                if !response.is_empty() {
                                    return Ok(response);
                                }
                            }
                        }
                        return Err("O qBittorrent foi reiniciado minimizado, mas ainda está iniciando. Tente atualizar os downloads em alguns segundos.".into());
                    }
                }
                return Ok(version);
            }
        }
        if qbit_running_process().is_none() {
            let path = qbit_config_path()?;
            if path.exists() && start_qbittorrent(&path).is_ok() {
                for _ in 0..20 {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    if let Ok(version) = qbit_request(&connection, "/api/v2/app/version", None) {
                        let version = String::from_utf8_lossy(&version).trim().to_string();
                        if !version.is_empty() {
                            return Ok(version);
                        }
                    }
                }
            }
        }
    }
    enable_qbittorrent_webui(store)
}
#[tauri::command]
fn test_qbittorrent_connection(store: State<'_, Store>) -> Result<String, String> {
    let db = store.0.lock().map_err(|e| e.to_string())?;
    let connection = qbit_connection(&db)?;
    let version = qbit_request(&connection, "/api/v2/app/version", None)?;
    Ok(String::from_utf8_lossy(&version).trim().to_string())
}
#[tauri::command]
fn list_qbittorrent_downloads(store: State<'_, Store>) -> Result<QbitDownloadsSnapshot, String> {
    let db = store.0.lock().map_err(|e| e.to_string())?;
    let connection = qbit_connection(&db)?;
    let payload = qbit_request(&connection, "/api/v2/torrents/info", None)?;
    let items = parse_qbit_torrents(&payload)?;
    let updated_game_ids = sync_qbittorrent_game_statuses(&db, &items)?;
    Ok(QbitDownloadsSnapshot {
        items,
        updated_game_ids,
    })
}
fn validate_qbit_hash(hash: &str) -> Result<&str, String> {
    let hash = hash.trim();
    if matches!(hash.len(), 40 | 64) && hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(hash)
    } else {
        Err("O identificador do torrent é inválido.".into())
    }
}
fn qbit_api_v5(connection: &QbitConnection) -> Result<bool, String> {
    let version = qbit_request(connection, "/api/v2/app/version", None)?;
    let version = String::from_utf8_lossy(&version);
    let major = version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .unwrap_or(5);
    Ok(major >= 5)
}
#[tauri::command]
fn control_qbittorrent_torrent(
    store: State<'_, Store>,
    hash: String,
    action: String,
) -> Result<(), String> {
    let hash = validate_qbit_hash(&hash)?;
    let db = store.0.lock().map_err(|e| e.to_string())?;
    let connection = qbit_connection(&db)?;
    let api_v5 = qbit_api_v5(&connection)?;
    let path = match (action.as_str(), api_v5) {
        ("pause", true) => "/api/v2/torrents/stop",
        ("pause", false) => "/api/v2/torrents/pause",
        ("resume", true) => "/api/v2/torrents/start",
        ("resume", false) => "/api/v2/torrents/resume",
        _ => return Err("Ação de download desconhecida.".into()),
    };
    qbit_request(&connection, path, Some(&[("hashes", hash.to_string())]))?;
    Ok(())
}
#[tauri::command]
fn remove_qbittorrent_torrent(
    store: State<'_, Store>,
    hash: String,
    delete_files: bool,
) -> Result<(), String> {
    let hash = validate_qbit_hash(&hash)?;
    let db = store.0.lock().map_err(|e| e.to_string())?;
    let connection = qbit_connection(&db)?;
    let torrent_path = format!("/api/v2/torrents/info?hashes={hash}");
    let torrent_data = qbit_request(&connection, &torrent_path, None)?;
    let torrents = parse_qbit_torrents(&torrent_data)?;
    let linked_game_id = torrents.first().and_then(|torrent| {
        torrent.tags.split(',').find_map(|tag| {
            tag.trim()
                .strip_prefix("desire-vault-game-")
                .and_then(|id| id.parse::<i64>().ok())
        })
    });
    let fields = [
        ("hashes", hash.to_string()),
        ("deleteFiles", delete_files.to_string()),
    ];
    qbit_request(&connection, "/api/v2/torrents/delete", Some(&fields))?;
    if let Some(id) = linked_game_id {
        let should_reset = delete_files
            || torrents
                .first()
                .is_some_and(|torrent| torrent.progress < 1.0);
        if should_reset {
            set_game_status(&db, id, "Na lista")?;
        }
    }
    Ok(())
}
#[tauri::command]
fn add_game_torrent_to_qbittorrent(store: State<'_, Store>, id: i64) -> Result<(), String> {
    let db = store.0.lock().map_err(|e| e.to_string())?;
    let stmt = db.prepare("SELECT torrent_url,status FROM games WHERE id=?1")?;
    db.bind_int(stmt, 1, id)?;
    let (url, game_status) = if db.step(stmt)? == SQLITE_ROW {
        (Db::text(stmt, 0), Db::text(stmt, 1))
    } else {
        Db::finalize(stmt);
        return Err("Não encontrei esse jogo na biblioteca.".into());
    };
    Db::finalize(stmt);
    let url = validate_torrent_url(&url)?;
    let connection = qbit_connection(&db)?;
    let tag = format!("desire-vault-game-{id}");
    qbit_request(
        &connection,
        "/api/v2/torrents/createTags",
        Some(&[("tags", tag.clone())]),
    )?;
    let fields = [("urls", url), ("paused", "false".into()), ("tags", tag)];
    let response = qbit_request(&connection, "/api/v2/torrents/add", Some(&fields))?;
    let body = String::from_utf8_lossy(&response).trim().to_string();
    if body.eq_ignore_ascii_case("fails.") {
        return Err("O qBittorrent recusou o link e não iniciou o download.".into());
    }
    if game_status == "Na lista" {
        set_game_status(&db, id, "Baixando")?;
    }
    Ok(())
}
#[tauri::command]
fn open_qbittorrent_client() -> Result<(), String> {
    Command::new("qbittorrent")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Não consegui abrir o qBittorrent: {e}"))
}
#[tauri::command]
fn minimize_window(window: tauri::Window) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}
#[tauri::command]
fn toggle_maximize_window(window: tauri::Window) -> Result<(), String> {
    if window.is_maximized().map_err(|e| e.to_string())? {
        window.unmaximize()
    } else {
        window.maximize()
    }
    .map_err(|e| e.to_string())
}
#[tauri::command]
fn close_window(window: tauri::Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

#[tauri::command]
fn uninstall_desire_vault(app: tauri::AppHandle, remove_user_data: bool) -> Result<(), String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("Não encontrei a pasta pessoal do usuário.")?;
    let data_root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    let mut paths = vec![
        data_root.join("applications/desire-vault.desktop"),
        data_root.join("icons/hicolor/256x256/apps/desire-vault.png"),
    ];
    if let Ok(output) = Command::new("xdg-user-dir").arg("DESKTOP").output() {
        if output.status.success() {
            let desktop = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !desktop.is_empty() {
                paths.push(PathBuf::from(desktop).join("Desire Vault.desktop"));
            }
        }
    }
    for path in paths {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Não consegui remover {}: {error}", path.display())),
        }
    }
    if remove_user_data {
        let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
        match fs::remove_dir_all(&app_data) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Não consegui remover os dados locais: {error}")),
        }
    }
    let _ = Command::new("update-desktop-database")
        .arg(data_root.join("applications"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = Command::new("kbuildsycoca6")
        .arg("--noincremental")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = Command::new("gtk-update-icon-cache")
        .args(["--force", "--ignore-theme-index"])
        .arg(data_root.join("icons/hicolor"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    match fs::remove_file(&executable) {
        Ok(()) => Ok(()),
        Err(error) => Err(format!(
            "Não consegui remover o executável {}: {error}",
            executable.display()
        )),
    }
}

fn main() {
    tauri::Builder::default()
        .on_page_load(|webview, _payload| {
            if webview.label() == "main" {
                if let Err(error) = webview.window().show() {
                    eprintln!("Não foi possível mostrar a janela principal: {error}");
                }
            }
        })
        .setup(|app| {
            let db = initialize(&app.handle()).map_err(std::io::Error::other)?;
            let startup_notice = restore_pending_install_backup(&app.handle(), &db);
            app.manage(Store(Mutex::new(db)));
            app.manage(StartupNotice(Mutex::new(startup_notice)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_games,
            export_library_backup,
            save_library_backup,
            restore_library_backup,
            take_startup_notice,
            save_game,
            delete_game,
            set_favorite,
            search_steam_games,
            get_steam_game,
            open_steam_page,
            find_steam_game,
            get_protondb_summary,
            open_protondb_page,
            get_qbittorrent_settings,
            configure_qbittorrent,
            enable_qbittorrent_webui,
            ensure_qbittorrent_ready,
            test_qbittorrent_connection,
            list_qbittorrent_downloads,
            control_qbittorrent_torrent,
            remove_qbittorrent_torrent,
            add_game_torrent_to_qbittorrent,
            open_qbittorrent_client,
            minimize_window,
            toggle_maximize_window,
            close_window,
            uninstall_desire_vault
        ])
        .run(tauri::generate_context!())
        .expect("falha ao iniciar Game Library Manager");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn library_backup_export_and_restore_preserve_games_and_covers() {
        let path = std::env::temp_dir().join(format!(
            "desire-vault-backup-{}.sqlite3",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let db = Db::open(&path).unwrap();
        db.exec("CREATE TABLE games(id INTEGER PRIMARY KEY AUTOINCREMENT,title TEXT NOT NULL,status TEXT NOT NULL,priority TEXT NOT NULL,genre TEXT NOT NULL,year TEXT NOT NULL,proton TEXT NOT NULL,cover TEXT NOT NULL,developer TEXT NOT NULL,publisher TEXT NOT NULL,app_id TEXT NOT NULL,description TEXT NOT NULL,torrent_url TEXT NOT NULL,favorite INTEGER NOT NULL,added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);").unwrap();
        let original = Game {
            id: 0,
            title: "Meu jogo".into(),
            status: "Baixado".into(),
            priority: "Quero muito jogar".into(),
            genre: "Aventura".into(),
            year: "2024".into(),
            proton: "Gold".into(),
            cover: "data:image/png;base64,abc123".into(),
            developer: "Estúdio".into(),
            publisher: "Distribuidora".into(),
            app_id: "12345".into(),
            description: "Nota".into(),
            torrent_url: "".into(),
            favorite: true,
            added_at: String::new(),
            updated_at: String::new(),
        };
        let saved = save_game_record(&db, original).unwrap();
        let backup = library_backup_value(&db).unwrap();
        let json = serde_json::to_string(&backup).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["games"][0]["cover"], "data:image/png;base64,abc123");
        let mut changed = saved.clone();
        changed.title = "Alterado".into();
        changed.cover.clear();
        save_game_record(&db, changed).unwrap();
        let games: Vec<Game> = serde_json::from_value(parsed["games"].clone()).unwrap();
        restore_games_transaction(&db, &games).unwrap();
        let stmt = db.prepare(&format!("{SELECT} WHERE id=?1")).unwrap();
        db.bind_int(stmt, 1, saved.id).unwrap();
        assert_eq!(db.step(stmt).unwrap(), SQLITE_ROW);
        let result = read_game(stmt);
        Db::finalize(stmt);
        assert_eq!(result.title, "Meu jogo");
        assert_eq!(result.cover, "data:image/png;base64,abc123");
        assert!(result.favorite);
        assert_eq!(result.status, "Baixado");
        drop(db);
        let _ = fs::remove_file(path);
    }
    #[test]
    fn startup_cleanup_removes_only_generated_demo_rows() {
        let path =
            std::env::temp_dir().join(format!("glm-cleanup-test-{}.sqlite3", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Db::open(&path).unwrap();
        db.exec("CREATE TABLE games(id INTEGER PRIMARY KEY,title TEXT,cover TEXT,app_id TEXT); CREATE TABLE app_state(key TEXT PRIMARY KEY,value TEXT);").unwrap();
        db.insert(
            "INSERT INTO games(title,cover,app_id) VALUES(?1,?2,?3)",
            &["Demo", "demo:1|#111|#222|TEST", ""],
        )
        .unwrap();
        db.insert(
            "INSERT INTO games(title,cover,app_id) VALUES(?1,?2,?3)",
            &["Meu jogo manual", "", ""],
        )
        .unwrap();
        db.insert(
            "INSERT INTO games(title,cover,app_id) VALUES(?1,?2,?3)",
            &["Jogo Steam", "data:image/jpeg;base64,abc", "220"],
        )
        .unwrap();
        db.insert(
            "INSERT INTO app_state(key,value) VALUES(?1,?2)",
            &["demo_seeded", "1"],
        )
        .unwrap();
        cleanup_demo_entries(&db).unwrap();
        let q = db.prepare("SELECT COUNT(*) FROM games").unwrap();
        assert_eq!(db.step(q).unwrap(), SQLITE_ROW);
        assert_eq!(unsafe { sqlite3_column_int64(q, 0) }, 2);
        Db::finalize(q);
        let q = db
            .prepare("SELECT COUNT(*) FROM games WHERE title='Demo'")
            .unwrap();
        assert_eq!(db.step(q).unwrap(), SQLITE_ROW);
        assert_eq!(unsafe { sqlite3_column_int64(q, 0) }, 0);
        Db::finalize(q);
        let q = db
            .prepare(
                "SELECT COUNT(*) FROM games WHERE title='Jogo Steam' AND cover LIKE 'data:image/%'",
            )
            .unwrap();
        assert_eq!(db.step(q).unwrap(), SQLITE_ROW);
        assert_eq!(unsafe { sqlite3_column_int64(q, 0) }, 1);
        Db::finalize(q);
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn steam_store_links_must_be_from_the_store_and_have_an_app_id() {
        assert_eq!(
            parse_store_app_id("https://store.steampowered.com/app/220/Half-Life_2/")
                .unwrap()
                .as_deref(),
            Some("220")
        );
        assert_eq!(
            parse_store_app_id("store.steampowered.com/app/620/")
                .unwrap()
                .as_deref(),
            Some("620")
        );
        assert!(parse_store_app_id("https://example.com/app/220/").is_err());
        assert!(parse_store_app_id("https://store.steampowered.com/app/not-a-number/").is_err());
    }
    #[test]
    fn steam_search_and_details_are_parsed_into_local_fields() {
        let search = r#"{"items":[{"type":"app","id":220,"name":"Half-Life 2"},{"type":"sub","id":1,"name":"Package"}]}"#;
        let found = parse_steam_search(search.as_bytes()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].app_id, "220");
        let details = r#"{"220":{"success":true,"data":{"type":"game","name":"Half-Life 2","developers":["Valve"],"publishers":["Valve"],"short_description":"Uma aventura em City 17.","release_date":{"date":"16/nov./2004"},"genres":[{"description":"Ação"}]}}}"#;
        let game = parse_steam_details("220", details.as_bytes()).unwrap();
        assert_eq!(game.title, "Half-Life 2");
        assert_eq!(game.year, "2004");
        assert_eq!(game.developer, "Valve");
        assert_eq!(game.genre, "Ação");
        assert_eq!(game.app_id, "220");
    }
    #[test]
    fn protondb_summary_is_parsed_without_treating_missing_fields_as_incompatibility() {
        let payload=br#"{"tier":"gold","confidence":"strong","score":0.78,"total":42,"trendingTier":"platinum"}"#;
        let summary = parse_protondb_summary(payload).unwrap();
        assert_eq!(summary.tier, "gold");
        assert_eq!(summary.confidence, "strong");
        assert_eq!(summary.total, 42);
        assert_eq!(summary.trending_tier, "platinum");
        assert!(parse_protondb_summary(br#"{"confidence":"low"}"#).is_err());
    }
    #[test]
    fn qbittorrent_setup_accepts_only_local_webui_and_supported_torrent_links() {
        assert_eq!(
            validate_qbit_host("http://127.0.0.1:8080/").unwrap(),
            "http://127.0.0.1:8080"
        );
        assert_eq!(
            validate_qbit_host("http://localhost:9090").unwrap(),
            "http://localhost:9090"
        );
        assert!(validate_qbit_host("http://example.com:8080").is_err());
        assert!(validate_torrent_url(
            "magnet:?xt=urn:btih:1234567890123456789012345678901234567890&dn=Meu+Jogo"
        )
        .is_ok());
        assert!(
            validate_torrent_url("https://meus-arquivos.example/jogo.torrent?token=abc").is_ok()
        );
        assert!(validate_torrent_url("https://example.com/").is_err());
    }
    #[test]
    fn qbittorrent_api_key_is_validated_and_webui_is_bound_only_to_loopback() {
        let key = "qbt_ABCDEFGHIJKLMNOPQRSTUVWXYZ12";
        assert_eq!(key.len(), 32);
        assert!(valid_qbit_api_key(key));
        assert!(!valid_qbit_api_key("password"));
        assert!(is_qbit_process_name("qbittorrent"));
        assert!(is_qbit_process_name("qbittorrent-qt"));
        assert!(is_qbit_process_name("qBittorrent-nox"));
        assert!(!is_qbit_process_name("bash"));
        assert_eq!(
            STANDARD.encode(pbkdf2_sha512(b"password", b"salt", 1)),
            "hn9wzxreAs/zdSWZo6U9xK80x6ZpgVrl1RNVThyM8lLALUcKKFoFAbrZmb/pQ8CPBQI119aLHaVeY/c7YKV/zg=="
        );
        let original="[Preferences]\nGeneral\\Language=pt_BR\n[BitTorrent]\nSession\\DefaultSavePath=/games\n";
        let updated = update_qbit_ini(original, key, 8181, "@ByteArray(salt:hash)");
        assert!(updated.contains("WebUI\\Enabled=true"));
        assert!(updated.contains("WebUI\\Address=127.0.0.1"));
        assert!(updated.contains("WebUI\\Port=8181"));
        assert!(updated.contains(&format!("WebUI\\APIKey={key}")));
        assert!(updated.contains("WebUI\\Password_PBKDF2=\"@ByteArray(salt:hash)\""));
        assert!(updated.contains("WebUI\\LocalHostAuth=true"));
        assert!(updated.contains("WebUI\\HTTPS\\Enabled=false"));
        assert!(updated.contains("General\\Language=pt_BR"));
        assert!(updated.contains("Session\\DefaultSavePath=/games"));
        assert!(
            updated.find("WebUI\\Enabled=true").unwrap() < updated.find("[BitTorrent]").unwrap()
        );
    }
    #[test]
    fn qbittorrent_download_rows_parse_progress_and_rates() {
        let list=parse_qbit_torrents(br#"[{"hash":"abc","name":"Jogo","state":"downloading","progress":0.5,"dlspeed":1024,"upspeed":256,"size":4096,"eta":60}]"#).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Jogo");
        assert_eq!(list[0].progress, 0.5);
        assert_eq!(list[0].download_speed, 1024);
        assert_eq!(list[0].eta, 60);
    }
    #[test]
    fn old_library_database_receives_torrent_url_column() {
        let path =
            std::env::temp_dir().join(format!("glm-migrate-test-{}.sqlite3", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Db::open(&path).unwrap();
        db.exec("CREATE TABLE games(id INTEGER PRIMARY KEY,title TEXT)")
            .unwrap();
        ensure_column(&db, "torrent_url", "TEXT NOT NULL DEFAULT ''").unwrap();
        let s = db.prepare("PRAGMA table_info(games)").unwrap();
        let mut found = false;
        loop {
            match db.step(s).unwrap() {
                SQLITE_ROW => {
                    if Db::text(s, 1) == "torrent_url" {
                        found = true
                    }
                }
                SQLITE_DONE => break,
                _ => unreachable!(),
            }
        }
        Db::finalize(s);
        assert!(found);
        drop(db);
        let _ = std::fs::remove_file(path);
    }
    #[test]
    #[ignore = "faz consultas públicas à Steam e ao ProtonDB; executar somente em smoke test de rede"]
    fn steam_public_search_imports_real_metadata_and_local_cover() {
        let found = steam_search("Half-Life 2".into()).unwrap();
        assert!(found.iter().any(|g| g.app_id == "220"));
        let game = steam_game_details("220".into()).unwrap();
        assert_eq!(game.title, "Half-Life 2");
        assert_eq!(game.developer, "Valve");
        assert!(game.cover.starts_with("data:image/"));
        let proton = protondb_summary("220".into())
            .unwrap()
            .expect("Half-Life 2 tem relatórios ProtonDB");
        assert!(matches!(
            proton.tier.as_str(),
            "platinum" | "gold" | "silver" | "bronze" | "borked"
        ));
        assert!(proton.total > 0);
        assert!(
            protondb_summary("999999999".into()).unwrap().is_none(),
            "AppID sem relatórios precisa continuar como Sem dados"
        );
        let path =
            std::env::temp_dir().join(format!("glm-steam-smoke-{}.sqlite3", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Db::open(&path).unwrap();
        db.exec("CREATE TABLE games(id INTEGER PRIMARY KEY AUTOINCREMENT,title TEXT NOT NULL,status TEXT NOT NULL,priority TEXT NOT NULL,genre TEXT NOT NULL,year TEXT NOT NULL,proton TEXT NOT NULL,cover TEXT NOT NULL,developer TEXT NOT NULL,publisher TEXT NOT NULL,app_id TEXT NOT NULL,description TEXT NOT NULL,torrent_url TEXT NOT NULL DEFAULT '',favorite INTEGER NOT NULL,added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)").unwrap();
        let saved = save_game_record(&db, game).unwrap();
        assert!(saved.id > 0);
        assert_eq!(saved.app_id, "220");
        assert!(saved.cover.starts_with("data:image/"));
        drop(db);
        let db = Db::open(&path).unwrap();
        let s = db.prepare(&format!("{SELECT} WHERE app_id='220'")).unwrap();
        assert_eq!(db.step(s).unwrap(), SQLITE_ROW);
        let restored = read_game(s);
        assert_eq!(restored.title, "Half-Life 2");
        assert!(restored.cover.starts_with("data:image/"));
        Db::finalize(s);
        drop(db);
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn sqlite_data_survives_reopen_and_can_be_updated_and_deleted() {
        let path = std::env::temp_dir().join(format!("glm-test-{}.sqlite3", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let db = Db::open(&path).unwrap();
            db.exec("CREATE TABLE games(id INTEGER PRIMARY KEY,title TEXT NOT NULL)")
                .unwrap();
            db.insert(
                "INSERT INTO games(title) VALUES(?1)",
                &["Teste de persistência"],
            )
            .unwrap();
            let q = db.prepare("SELECT id,title FROM games").unwrap();
            assert_eq!(db.step(q).unwrap(), SQLITE_ROW);
            let id = unsafe { sqlite3_column_int64(q, 0) };
            assert_eq!(Db::text(q, 1), "Teste de persistência");
            Db::finalize(q);
            let q = db.prepare("UPDATE games SET title=?1 WHERE id=?2").unwrap();
            db.bind_text(q, 1, "Jogo editado").unwrap();
            db.bind_int(q, 2, id).unwrap();
            db.step(q).unwrap();
            Db::finalize(q);
        }
        {
            let db = Db::open(&path).unwrap();
            let q = db.prepare("SELECT title FROM games WHERE id=1").unwrap();
            assert_eq!(db.step(q).unwrap(), SQLITE_ROW);
            assert_eq!(Db::text(q, 0), "Jogo editado");
            Db::finalize(q);
            let q = db.prepare("DELETE FROM games WHERE id=1").unwrap();
            db.step(q).unwrap();
            Db::finalize(q);
            let q = db.prepare("SELECT COUNT(*) FROM games").unwrap();
            assert_eq!(db.step(q).unwrap(), SQLITE_ROW);
            assert_eq!(unsafe { sqlite3_column_int64(q, 0) }, 0);
            Db::finalize(q);
        }
        let _ = std::fs::remove_file(path);
    }
}
