use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc::Sender};

use axum::{
    Router,
    extract::{DefaultBodyLimit, Multipart, Path as UrlPath, State},
    body::Body,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use tokio::{fs::File, io::AsyncWriteExt};
use tokio_util::io::ReaderStream;

const PAGE: &str = include_str!("index.html");

pub struct Received {
    pub path: PathBuf,
    pub size: u64,
}

/// Eine Datei, die der PC dem Handy zum Download anbietet.
pub struct SharedFile {
    pub id: u64,
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
}

pub type Files = Arc<Mutex<Vec<SharedFile>>>;

struct AppState {
    files: Files,
    token: String,
    save_dir: PathBuf,
    events: Sender<Received>,
    repaint: eframe::egui::Context,
}

type Shared = Arc<AppState>;

pub fn new_token() -> String {
    rand::random::<[u8; 16]>().iter().map(|b| format!("{b:02x}")).collect()
}

/// Startet den Server in einem eigenen Thread; er endet mit dem Prozess.
pub fn spawn(
    listener: std::net::TcpListener,
    token: String,
    save_dir: PathBuf,
    events: Sender<Received>,
    files: Files,
    repaint: eframe::egui::Context,
) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            let state = Arc::new(AppState { files, token, save_dir, events, repaint });
            let app = Router::new()
                .route("/{token}/", get(page))
                .route("/{token}/upload", post(upload))
                .route("/{token}/files", get(list_files))
                .route("/{token}/download/{id}", get(download))
                .layer(DefaultBodyLimit::disable())
                .with_state(state);
            listener.set_nonblocking(true).unwrap();
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    });
}

async fn page(
    State(state): State<Shared>,
    UrlPath(token): UrlPath<String>,
) -> Result<Html<&'static str>, StatusCode> {
    check(&state, &token)?;
    Ok(Html(PAGE))
}

async fn upload(
    State(state): State<Shared>,
    UrlPath(token): UrlPath<String>,
    mut multipart: Multipart,
) -> Result<StatusCode, StatusCode> {
    check(&state, &token)?;

    while let Some(mut field) = multipart.next_field().await.map_err(|_| StatusCode::BAD_REQUEST)? {
        let name = sanitize(field.file_name().unwrap_or("datei"));
        let path = unique_path(&state.save_dir, &name).await;
        let mut file = File::create(&path).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let mut size = 0u64;
        while let Some(chunk) = field.chunk().await.map_err(|_| StatusCode::BAD_REQUEST)? {
            size += chunk.len() as u64;
            file.write_all(&chunk).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        file.flush().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let _ = state.events.send(Received { path, size });
        state.repaint.request_repaint();
    }
    Ok(StatusCode::OK)
}

async fn list_files(
    State(state): State<Shared>,
    UrlPath(token): UrlPath<String>,
) -> Result<Response, StatusCode> {
    check(&state, &token)?;
    let files = state.files.lock().unwrap();
    let items: Vec<String> = files
        .iter()
        .map(|f| format!("{{\"id\":{},\"name\":\"{}\",\"size\":{}}}", f.id, json_escape(&f.name), f.size))
        .collect();
    let body = format!("[{}]", items.join(","));
    Ok(([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-store")], body).into_response())
}

async fn download(
    State(state): State<Shared>,
    UrlPath((token, id)): UrlPath<(String, u64)>,
) -> Result<Response, StatusCode> {
    check(&state, &token)?;
    // Nur Dateien aus der Freigabeliste, nie beliebige Pfade.
    let (path, name) = {
        let files = state.files.lock().unwrap();
        let f = files.iter().find(|f| f.id == id).ok_or(StatusCode::NOT_FOUND)?;
        (f.path.clone(), f.name.clone())
    };
    let file = File::open(&path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let len = file.metadata().await.map_err(|_| StatusCode::NOT_FOUND)?.len();
    let disposition = format!("attachment; filename*=UTF-8''{}", percent_encode(&name));
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (header::CONTENT_LENGTH, len.to_string()),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        Body::from_stream(ReaderStream::new(file)),
    )
        .into_response())
}

fn json_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn check(state: &AppState, token: &str) -> Result<(), StatusCode> {
    if token == state.token { Ok(()) } else { Err(StatusCode::NOT_FOUND) }
}

/// Entfernt Pfadanteile und für Windows ungültige Zeichen aus dem Dateinamen.
fn sanitize(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_control() || "<>:\"|?*".contains(c) { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() { "datei".into() } else { cleaned }
}

/// Hängt " (1)", " (2)" an, damit vorhandene Dateien nie überschrieben werden.
async fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
        return candidate;
    }
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = p.extension().and_then(|s| s.to_str());
    for i in 1.. {
        let n = match ext {
            Some(e) => format!("{stem} ({i}).{e}"),
            None => format!("{stem} ({i})"),
        };
        let c = dir.join(n);
        if !tokio::fs::try_exists(&c).await.unwrap_or(false) {
            return c;
        }
    }
    unreachable!()
}
