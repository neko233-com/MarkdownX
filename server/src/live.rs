use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use fdl_core::protocol::{Peer, ServerMessage};
use tokio::sync::{broadcast, Mutex};

use crate::git::{GitError, Repo};
use crate::store::{self, Store, StoreError};

const HISTORY_LIMIT: usize = 256;

#[derive(Clone, Debug)]
pub struct Revision {
    pub rev: u64,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct Conflict {
    pub rev: u64,
    pub title: String,
    pub content: String,
    pub merged: String,
    pub by: String,
}

#[derive(Clone, Debug)]
pub struct SaveOutcome {
    pub rev: u64,
    pub title: String,
    pub by: String,
    pub at: u64,
    pub changed: bool,
    pub commit: Option<String>,
    pub warning: Option<String>,
    pub conflict: Option<Conflict>,
}

#[derive(Debug)]
pub enum SaveError {
    TooBig,
    Stale,
    Store(StoreError),
    Merge(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooBig => write!(f, "document is too large"),
            Self::Stale => write!(f, "revision is no longer available"),
            Self::Store(e) => write!(f, "{e}"),
            Self::Merge(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for SaveError {}

struct PeerState {
    peer: Peer,
}

struct State {
    content: String,
    rev: u64,
    by: String,
    at: u64,
    history: VecDeque<Revision>,
    peers: HashMap<String, PeerState>,
}

pub struct Room {
    id: String,
    store: Arc<Store>,
    repo: Arc<Repo>,
    max_bytes: usize,
    state: Mutex<State>,
    tx: broadcast::Sender<ServerMessage>,
}

impl Room {
    pub async fn open(id: String, store: Arc<Store>, repo: Arc<Repo>, max_bytes: usize) -> Result<Self, StoreError> {
        let content = match store.read(&id) {
            Ok(s) => s,
            Err(StoreError::NotFound) => String::new(),
            Err(e) => return Err(e),
        };
        let now = now_millis();
        let mut history = VecDeque::new();
        history.push_back(Revision { rev: 1, content: content.clone() });
        let (tx, _) = broadcast::channel(512);
        Ok(Self {
            id,
            store,
            repo,
            max_bytes,
            state: Mutex::new(State { content, rev: 1, by: String::new(), at: now, history, peers: HashMap::new() }),
            tx,
        })
    }

    pub async fn snapshot(&self, client_id: &str) -> ServerMessage {
        let st = self.state.lock().await;
        ServerMessage::Snapshot {
            client_id: client_id.to_string(),
            id: self.id.clone(),
            title: store::title(&st.content, &self.id),
            rev: st.rev,
            content: st.content.clone(),
            peers: peers(&st),
        }
    }

    /// Register a socket, return its snapshot and a stream of room messages.
    pub async fn join(&self, peer: Peer) -> (ServerMessage, broadcast::Receiver<ServerMessage>) {
        let rx = self.tx.subscribe();
        let mut st = self.state.lock().await;
        st.peers.insert(peer.id.clone(), PeerState { peer: peer.clone() });
        let snapshot = ServerMessage::Snapshot {
            client_id: peer.id.clone(),
            id: self.id.clone(),
            title: store::title(&st.content, &self.id),
            rev: st.rev,
            content: st.content.clone(),
            peers: peers(&st),
        };
        drop(st);
        self.broadcast_presence().await;
        (snapshot, rx)
    }

    pub async fn leave(&self, client_id: &str) {
        let mut st = self.state.lock().await;
        st.peers.remove(client_id);
        drop(st);
        self.broadcast_presence().await;
    }

    pub async fn cursor(&self, client_id: &str, cursor: usize) {
        let mut st = self.state.lock().await;
        if let Some(p) = st.peers.get_mut(client_id) {
            p.peer.cursor = Some(cursor);
        }
        drop(st);
        self.broadcast_presence().await;
    }

    pub async fn save(&self, base: u64, content: String, author: &str, origin: &str) -> Result<SaveOutcome, SaveError> {
        if content.len() > self.max_bytes {
            return Err(SaveError::TooBig);
        }
        let author = clean_name(author);
        let now = now_millis();
        let mut st = self.state.lock().await;
        if base == st.rev {
            if content == st.content {
                return Ok(SaveOutcome {
                    rev: st.rev,
                    title: store::title(&st.content, &self.id),
                    by: st.by.clone(),
                    at: st.at,
                    changed: false,
                    commit: None,
                    warning: None,
                    conflict: None,
                });
            }
            return self.commit_locked(&mut st, content, &author, origin, now).await;
        }
        if base > st.rev {
            return Err(SaveError::Stale);
        }
        let Some(base_content) = st.history.iter().find(|r| r.rev == base).map(|r| r.content.clone()) else {
            return Err(SaveError::Stale);
        };
        let current = st.content.clone();
        let expected = st.rev;
        drop(st);

        let (merged, clean) = self.repo.merge(base_content.as_bytes(), current.as_bytes(), content.as_bytes()).await.map_err(|e| SaveError::Merge(e.to_string()))?;
        if !clean {
            let st = self.state.lock().await;
            return Ok(SaveOutcome {
                rev: st.rev,
                title: store::title(&st.content, &self.id),
                by: author.clone(),
                at: now,
                changed: false,
                commit: None,
                warning: None,
                conflict: Some(Conflict {
                    rev: st.rev,
                    title: store::title(&st.content, &self.id),
                    content: st.content.clone(),
                    merged,
                    by: author,
                }),
            });
        }
        if merged == current {
            let st = self.state.lock().await;
            return Ok(SaveOutcome {
                rev: st.rev,
                title: store::title(&st.content, &self.id),
                by: author,
                at: now,
                changed: false,
                commit: None,
                warning: None,
                conflict: None,
            });
        }
        let mut st = self.state.lock().await;
        if st.rev != expected {
            return Ok(SaveOutcome {
                rev: st.rev,
                title: store::title(&st.content, &self.id),
                by: author.clone(),
                at: now,
                changed: false,
                commit: None,
                warning: None,
                conflict: Some(Conflict {
                    rev: st.rev,
                    title: store::title(&st.content, &self.id),
                    content: st.content.clone(),
                    merged: String::new(),
                    by: author,
                }),
            });
        }
        self.commit_locked(&mut st, merged, &author, origin, now).await
    }

    pub async fn restore(&self, content: String, author: &str, origin: &str) -> Result<SaveOutcome, SaveError> {
        if content.len() > self.max_bytes {
            return Err(SaveError::TooBig);
        }
        let author = clean_name(author);
        let now = now_millis();
        let mut st = self.state.lock().await;
        self.commit_locked(&mut st, content, &author, origin, now).await
    }

    async fn commit_locked(&self, st: &mut State, content: String, author: &str, origin: &str, now: u64) -> Result<SaveOutcome, SaveError> {
        self.store.write(&self.id, &content, self.max_bytes).map_err(SaveError::Store)?;
        st.content = content.clone();
        st.rev += 1;
        st.by = author.to_string();
        st.at = now;
        st.history.push_back(Revision { rev: st.rev, content: content.clone() });
        while st.history.len() > HISTORY_LIMIT {
            st.history.pop_front();
        }
        let rev = st.rev;
        let title = store::title(&content, &self.id);
        let rel = self.store.rel(&self.id).map_err(SaveError::Store)?;
        let (commit, warning) = match self.repo.commit(&rel, &format!("edit: {author}"), author).await {
            Ok(hash) => (Some(hash), None),
            Err(GitError::NoChanges) => (None, None),
            Err(e) => (None, Some(e.to_string())),
        };
        let update = ServerMessage::Update {
            rev,
            title: title.clone(),
            by: author.to_string(),
            origin: Some(origin.to_string()),
            at: now,
            content: content.clone(),
        };
        let _ = self.tx.send(update);
        Ok(SaveOutcome { rev, title, by: author.to_string(), at: now, changed: true, commit, warning, conflict: None })
    }

    async fn broadcast_presence(&self) {
        let peers = {
            let st = self.state.lock().await;
            peers(&st)
        };
        let _ = self.tx.send(ServerMessage::Presence { peers });
    }
}

pub struct Hub {
    pub store: Arc<Store>,
    pub repo: Arc<Repo>,
    max_bytes: usize,
    rooms: Mutex<HashMap<String, Arc<Room>>>,
}

impl Hub {
    pub fn new(store: Arc<Store>, repo: Arc<Repo>, max_bytes: usize) -> Self {
        Self { store, repo, max_bytes, rooms: Mutex::new(HashMap::new()) }
    }

    pub async fn room(&self, id: &str) -> Result<Arc<Room>, SaveError> {
        let mut rooms = self.rooms.lock().await;
        if let Some(room) = rooms.get(id) {
            return Ok(room.clone());
        }
        let room = Arc::new(Room::open(id.to_string(), self.store.clone(), self.repo.clone(), self.max_bytes).await.map_err(SaveError::Store)?);
        rooms.insert(id.to_string(), room.clone());
        Ok(room)
    }

}

fn peers(st: &State) -> Vec<Peer> {
    let mut out: Vec<Peer> = st.peers.values().map(|p| p.peer.clone()).collect();
    out.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    out
}

fn clean_name(name: &str) -> String {
    let name = name.trim().replace(['\n', '\r', '\t', '\0'], "");
    if name.is_empty() { "Reader".into() } else { name.chars().take(32).collect() }
}

fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup() -> (Arc<Room>, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("markdownx-room-{}", uuid::Uuid::new_v4()));
        let store = Arc::new(Store::open(&root).unwrap());
        store.write("note", "# Note\n\nbody\n", 1 << 20).unwrap();
        let repo = Arc::new(Repo::open(&root, "Test", "test@example.com").await.unwrap());
        repo.commit("docs/note.md", "create", "Ada").await.unwrap();
        (Arc::new(Room::open("note".into(), store, repo, 1 << 20).await.unwrap()), root)
    }

    #[tokio::test]
    async fn saves_and_broadcasts_full_content() {
        let (room, root) = setup().await;
        let (_snap, mut rx) = room.join(Peer { id: "c1".into(), name: "Ada".into(), color: "#000".into(), cursor: None }).await;
        let out = room.save(1, "# Note\n\nbody\n\nA\n".into(), "Ada", "c1").await.unwrap();
        assert!(out.changed);
        let msg = loop {
            match rx.recv().await.unwrap() {
                ServerMessage::Presence { .. } => continue,
                other => break other,
            }
        };
        match msg {
            ServerMessage::Update { content, origin, .. } => {
                assert!(content.contains("A"));
                assert_eq!(origin.as_deref(), Some("c1"));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(room.store.read("note").unwrap().contains("A"));
        let _ = room.leave("c1").await;
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn merges_stale_base_and_reports_conflict() {
        let (room, root) = setup().await;
        let _ = room.join(Peer { id: "a".into(), name: "A".into(), color: "#000".into(), cursor: None }).await;
        let _ = room.join(Peer { id: "b".into(), name: "B".into(), color: "#111".into(), cursor: None }).await;
        room.save(1, "# Note\n\nbody\n\nA\n".into(), "A", "a").await.unwrap();
        let out = room.save(1, "# Note\n\nbody\n\nB\n".into(), "B", "b").await.unwrap();
        assert!(out.changed || out.conflict.is_some());
        assert!(room.store.read("note").unwrap().contains("A"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn rejects_oversized_writes() {
        let (room, root) = setup().await;
        let err = room.save(1, "x".repeat((1 << 20) + 1), "Ada", "c1").await.unwrap_err();
        assert!(matches!(err, SaveError::TooBig));
        let _ = std::fs::remove_dir_all(root);
    }
}
