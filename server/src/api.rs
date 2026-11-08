use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use fdl_core::protocol::{ClientMessage, Peer, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use subtle::ConstantTimeEq;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::Config;
use crate::git::{GitError, Repo};
use crate::live::{Hub, SaveError, SaveOutcome};
use crate::store::{self, Store, StoreError};

#[derive(Clone)]
pub struct AppState {
    pub cfg: Config,
    pub store: Arc<Store>,
    pub repo: Arc<Repo>,
    pub hub: Arc<Hub>,
}

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/docs", get(list).post(create))
        .route("/api/docs/{id}", get(read).put(write))
        .route("/api/docs/{id}/history", get(history))
        .route("/api/docs/{id}/diff", get(diff))
        .route("/api/docs/{id}/revision/{rev}", get(revision))
        .route("/api/docs/{id}/restore", post(restore))
        .route("/ws/docs/{id}", get(websocket))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth));
    Router::new()
        .route("/api/health", get(health))
        .merge(protected)
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(json!({"ok": true, "service": "markdownx-server"}))
}

async fn auth(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if state.cfg.token.is_empty() {
        if state.cfg.dev {
            return next.run(req).await;
        }
        return error(StatusCode::UNAUTHORIZED, "server token is not configured");
    }
    let mut token = bearer(req.headers()).unwrap_or_default();
    if token.is_empty()
        && let Some(q) = req.uri().query()
    {
        token = form_value(q, "token").unwrap_or_default();
    }
    if token.is_empty() {
        token = cookie(req.headers(), "mx_token").unwrap_or_default();
    }
    let expected = state.cfg.token.as_bytes();
    let given = token.as_bytes();
    let same = expected.len() == given.len() && expected.ct_eq(given).into();
    if !same {
        return error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    next.run(req).await
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim().to_string())
}

fn cookie(headers: &HeaderMap, key: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in raw.split(';') {
        if let Some((k, v)) = part.trim().split_once('=')
            && k == key
        {
            return Some(v.to_string());
        }
    }
    None
}

fn form_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|p| {
        let (k, v) = p.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn list(State(state): State<AppState>) -> Response {
    match state.store.list() {
        Ok(docs) => Json(json!({"docs": docs})).into_response(),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    content: String,
}

async fn create(State(state): State<AppState>, Json(req): Json<Create>) -> Response {
    let mut content = req.content;
    if content.is_empty() {
        let title = if req.title.trim().is_empty() { "Untitled" } else { req.title.trim() };
        content = format!("# {title}\n");
    }
    let mut id = req.id.trim().to_string();
    if id.is_empty() {
        id = store::slug(&req.title);
    }
    if !store::valid_id(&id) {
        return error(StatusCode::BAD_REQUEST, "invalid document id");
    }
    match state.store.read(&id) {
        Ok(_) => return error(StatusCode::CONFLICT, "document already exists"),
        Err(StoreError::NotFound) => {}
        Err(StoreError::BadId) => return error(StatusCode::BAD_REQUEST, "invalid document id"),
        Err(e) => return error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
    if let Err(e) = state.store.write(&id, &content, state.cfg.max_bytes) {
        return store_error(e);
    }
    let rel = match state.store.rel(&id) {
        Ok(r) => r,
        Err(e) => return store_error(e),
    };
    let commit = match state.repo.commit(&rel, &format!("create: {id}"), &state.cfg.commit_name).await {
        Ok(hash) => Some(hash),
        Err(GitError::NoChanges) => None,
        Err(e) => return error(StatusCode::INTERNAL_SERVER_ERROR, &format!("git commit: {e}")),
    };
    (StatusCode::CREATED, Json(json!({"id": id, "commit": commit}))).into_response()
}

async fn read(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let content = match state.store.read(&id) {
        Ok(c) => c,
        Err(e) => return store_error(e),
    };
    let doc = state.store.stat(&id).ok();
    Json(json!({"id": id, "title": store::title(&content, &id), "content": content, "doc": doc})).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Write {
    base: u64,
    content: String,
}

async fn write(State(state): State<AppState>, Path(id): Path<String>, headers: HeaderMap, Json(req): Json<Write>) -> Response {
    if let Err(e) = state.store.stat(&id) {
        return store_error(e);
    }
    let room = match state.hub.room(&id).await {
        Ok(r) => r,
        Err(e) => return save_error(e),
    };
    let author = author(&state.cfg, &headers, None);
    match room.save(req.base, req.content, &author, "http").await {
        Ok(out) if out.conflict.is_some() => conflict(out),
        Ok(out) => saved(out),
        Err(e) => save_error(e),
    }
}

async fn history(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<HashMap<String, String>>) -> Response {
    let rel = match state.store.rel(&id) {
        Ok(r) => r,
        Err(e) => return store_error(e),
    };
    let limit = q.get("limit").and_then(|v| v.parse::<usize>().ok()).unwrap_or(50).clamp(1, 200);
    match state.repo.log(&rel, limit).await {
        Ok(commits) => Json(json!({"commits": commits})).into_response(),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

async fn diff(State(state): State<AppState>, Path(id): Path<String>, Query(q): Query<HashMap<String, String>>) -> Response {
    let rel = match state.store.rel(&id) {
        Ok(r) => r,
        Err(e) => return store_error(e),
    };
    let from = q.get("from").cloned().unwrap_or_default();
    let to = q.get("to").cloned().unwrap_or_default();
    match state.repo.diff(&from, &to, &rel).await {
        Ok(text) => Json(json!({"from": from, "to": to, "diff": text})).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

async fn revision(State(state): State<AppState>, Path((id, rev)): Path<(String, String)>) -> Response {
    let rel = match state.store.rel(&id) {
        Ok(r) => r,
        Err(e) => return store_error(e),
    };
    match state.repo.show(&rev, &rel).await {
        Ok(content) => Json(json!({"rev": rev, "content": content})).into_response(),
        Err(e) => error(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Restore {
    rev: String,
}

async fn restore(State(state): State<AppState>, Path(id): Path<String>, headers: HeaderMap, Json(req): Json<Restore>) -> Response {
    if req.rev.trim().is_empty() {
        return error(StatusCode::BAD_REQUEST, "revision is required");
    }
    let rel = match state.store.rel(&id) {
        Ok(r) => r,
        Err(e) => return store_error(e),
    };
    let content = match state.repo.show(&req.rev, &rel).await {
        Ok(c) => c,
        Err(e) => return error(StatusCode::BAD_REQUEST, &e.to_string()),
    };
    let room = match state.hub.room(&id).await {
        Ok(r) => r,
        Err(e) => return save_error(e),
    };
    match room.restore(content, &author(&state.cfg, &headers, None), "restore").await {
        Ok(out) => Json(json!({"rev": out.rev, "commit": out.commit, "warning": out.warning})).into_response(),
        Err(e) => save_error(e),
    }
}

async fn websocket(State(state): State<AppState>, Path(id): Path<String>, headers: HeaderMap, ws: WebSocketUpgrade, Query(q): Query<HashMap<String, String>>) -> Response {
    if let Err(e) = state.store.stat(&id) {
        return store_error(e);
    }
    if !origin_ok(&state.cfg, &headers) {
        return error(StatusCode::FORBIDDEN, "origin not allowed");
    }
    let name = clean_name(q.get("name").map(String::as_str).unwrap_or(""));
    let client_id = format!("c-{}", &Uuid::new_v4().simple().to_string()[..8]);
    let peer = Peer { id: client_id.clone(), name, color: color(&client_id), cursor: None };
    ws.on_upgrade(move |socket| socket_loop(socket, state, id, peer))
}

async fn socket_loop(socket: WebSocket, state: AppState, id: String, peer: Peer) {
    let room = match state.hub.room(&id).await {
        Ok(r) => r,
        Err(_) => return,
    };
    let (mut sender, mut receiver) = socket.split();
    let (snapshot, mut updates) = room.join(peer.clone()).await;
    if sender.send(Message::Text(json_text(&snapshot).into())).await.is_err() {
        room.leave(&peer.id).await;
        return;
    }
    let (direct_tx, mut direct_rx) = mpsc::channel::<ServerMessage>(32);
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = updates.recv() => match msg {
                    Ok(msg) => if sender.send(Message::Text(json_text(&msg).into())).await.is_err() { break },
                    Err(_) => break,
                },
                msg = direct_rx.recv() => match msg {
                    Some(msg) => if sender.send(Message::Text(json_text(&msg).into())).await.is_err() { break },
                    None => break,
                },
            }
        }
    });

    while let Some(Ok(msg)) = receiver.next().await {
        let Message::Text(text) = msg else { continue };
        let Ok(client) = serde_json::from_str::<ClientMessage>(text.as_str()) else {
            let _ = direct_tx.send(ServerMessage::Error { message: "invalid message".into() }).await;
            continue;
        };
        match client {
            ClientMessage::Ping => { let _ = direct_tx.send(ServerMessage::Pong).await; }
            ClientMessage::Cursor { cursor } => room.cursor(&peer.id, cursor).await,
            ClientMessage::Save { base, content, name } => {
                let author = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| peer.name.clone());
                match room.save(base, content, &author, &peer.id).await {
                    Ok(out) => {
                        if let Some(c) = out.conflict {
                            let _ = direct_tx.send(ServerMessage::Conflict { rev: c.rev, content: c.content, merged: c.merged, by: c.by }).await;
                        } else {
                            let _ = direct_tx.send(ServerMessage::Saved { rev: out.rev, by: out.by, at: out.at, commit: out.commit, warning: out.warning }).await;
                        }
                    }
                    Err(SaveError::Stale) => {
                        let _ = direct_tx.send(room.snapshot(&peer.id).await).await;
                    }
                    Err(e) => { let _ = direct_tx.send(ServerMessage::Error { message: e.to_string() }).await; }
                }
            }
        }
    }
    send_task.abort();
    room.leave(&peer.id).await;
}

fn json_text(msg: &ServerMessage) -> String {
    serde_json::to_string(msg).unwrap_or_else(|_| r#"{"type":"error","message":"encode failed"}"#.into())
}

fn saved(out: SaveOutcome) -> Response {
    Json(json!({"rev": out.rev, "title": out.title, "changed": out.changed, "commit": out.commit, "warning": out.warning})).into_response()
}

fn conflict(out: SaveOutcome) -> Response {
    match out.conflict {
        Some(c) => (StatusCode::CONFLICT, Json(json!({"rev": c.rev, "title": c.title, "content": c.content, "merged": c.merged, "by": c.by}))).into_response(),
        None => error(StatusCode::CONFLICT, "conflict"),
    }
}

fn author(cfg: &Config, headers: &HeaderMap, fallback: Option<&str>) -> String {
    let name = headers
        .get("x-markdownx-name")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| fallback.map(str::to_string))
        .unwrap_or_else(|| cfg.commit_name.clone());
    if name.is_empty() { "anonymous".into() } else { name }
}

fn origin_ok(cfg: &Config, headers: &HeaderMap) -> bool {
    if cfg.dev || cfg.origin == "*" {
        return true;
    }
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else { return true };
    if cfg.origin.is_empty() {
        let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
        return origin_host(origin).is_some_and(|h| h.eq_ignore_ascii_case(host));
    }
    origin.trim_end_matches('/').eq_ignore_ascii_case(cfg.origin.trim_end_matches('/'))
}

fn origin_host(origin: &str) -> Option<&str> {
    let rest = origin.strip_prefix("http://").or_else(|| origin.strip_prefix("https://"))?;
    Some(rest.split('/').next().unwrap_or(rest))
}

fn clean_name(name: &str) -> String {
    let name = name.trim().replace(['\n', '\r', '\t', '\0'], "");
    if name.is_empty() { "Guest".into() } else { name.chars().take(32).collect() }
}

fn color(seed: &str) -> String {
    const PALETTE: [&str; 6] = ["#b05226", "#2f6f61", "#3f5f9e", "#8a5a2b", "#7a4b8c", "#4f6b2f"];
    let n = seed.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
    PALETTE[n % PALETTE.len()].to_string()
}

fn error(status: StatusCode, text: &str) -> Response {
    (status, Json(json!({"error": text}))).into_response()
}

fn store_error(e: StoreError) -> Response {
    match e {
        StoreError::BadId => error(StatusCode::BAD_REQUEST, &e.to_string()),
        StoreError::NotFound => error(StatusCode::NOT_FOUND, &e.to_string()),
        StoreError::TooBig => error(StatusCode::PAYLOAD_TOO_LARGE, &e.to_string()),
        StoreError::Io(_) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

fn save_error(e: SaveError) -> Response {
    match e {
        SaveError::TooBig => error(StatusCode::PAYLOAD_TOO_LARGE, &e.to_string()),
        SaveError::Stale => error(StatusCode::CONFLICT, &e.to_string()),
        SaveError::Store(e) => store_error(e),
        SaveError::Merge(_) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use tower::ServiceExt;

    async fn test_app(token: &str, dev: bool) -> (Router, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("markdownx-api-{}", Uuid::new_v4()));
        let store = Arc::new(Store::open(&root).unwrap());
        let repo = Arc::new(Repo::open(&root, "Test", "test@example.com").await.unwrap());
        let hub = Arc::new(Hub::new(store.clone(), repo.clone(), 1 << 20));
        let cfg = Config {
            addr: "127.0.0.1:0".into(),
            root: root.clone(),
            token: token.into(),
            origin: String::new(),
            max_bytes: 1 << 20,
            commit_name: "Test".into(),
            commit_mail: "test@example.com".into(),
            web_dir: "server/web".into(),
            dev,
        };
        (router(AppState { cfg, store, repo, hub }), root)
    }

    fn json_request(method: &str, uri: &str, body: serde_json::Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn creates_reads_writes_and_diffs() {
        let (app, root) = test_app("", true).await;
        let res = app.clone().oneshot(Request::builder().uri("/api/health").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let res = app.clone().oneshot(json_request("POST", "/api/docs", json!({"title": "Note", "content": ""}))).await.unwrap();
        assert_eq!(res.status(), StatusCode::CREATED);
        let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        let id = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["id"].as_str().unwrap().to_string();

        let res = app.clone().oneshot(Request::builder().uri(format!("/api/docs/{id}")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("# Note"));

        let res = app.clone().oneshot(json_request("PUT", &format!("/api/docs/{id}"), json!({"base": 1, "content": "# Note\n\nbody\n"}))).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&body).unwrap()["rev"], 2);

        let res = app.clone().oneshot(Request::builder().uri(format!("/api/docs/{id}/history")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        assert!(serde_json::from_slice::<serde_json::Value>(&body).unwrap()["commits"].as_array().unwrap().len() >= 2);

        let res = app.clone().oneshot(Request::builder().uri(format!("/api/docs/{id}/diff?to=HEAD")).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn requires_a_token_when_configured() {
        let (app, root) = test_app("secret", false).await;
        let res = app.clone().oneshot(Request::builder().uri("/api/docs").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let res = app
            .clone()
            .oneshot(Request::builder().uri("/api/docs").header("authorization", "Bearer secret").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let _ = std::fs::remove_dir_all(root);
    }
}
