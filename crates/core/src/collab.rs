//! Reserved native realtime-collaboration layer.
//!
//! This module deliberately contains **no transport and no UI**. It defines the
//! session, presence and operation envelope that a future WebSocket/WebRTC
//! adapter, desktop multi-window session, or agent bridge will drive. Keeping
//! the envelope in `fdl_core` means the same operation log can be replayed by
//! the desktop editor, the Web reader and the CLI.
//!
//! The first implementation is intentionally conservative: operations are
//! sequenced and checked for stale/future bases, but the document merge is left
//! to the caller until the editor exposes stable block paths for every kind.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 1;

pub type PeerId = String;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cursor {
    pub block: String,
    pub offset: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Peer {
    pub id: PeerId,
    pub name: String,
    /// CSS hex colour chosen by the host; the server does not assign identity.
    pub color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    /// Host clock in milliseconds; used only for presence expiry.
    pub last_seen: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationKind {
    /// Replace a byte range inside one block's plain text.
    ReplaceText { block: String, start: usize, end: usize, text: String },
    /// Replace a whole block's plain text.
    SetBlockText { block: String, text: String },
    /// Insert serialized DocxXML after a block; `None` means at the end.
    InsertBlock { after: Option<String>, xml: String },
    RemoveBlock { block: String },
    /// Annotation changes are carried as opaque JSON so the annotation store
    /// remains the source of truth.
    Annotation { action: String, payload: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Operation {
    pub protocol: u32,
    pub id: String,
    pub author: PeerId,
    /// Monotonic per author.
    pub seq: u64,
    /// Session revision the author edited from.
    pub base: u64,
    pub kind: OperationKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Apply {
    Applied,
    Duplicate,
    /// The operation was built against an older revision; the caller must
    /// transform it before retrying.
    Stale,
    /// The operation refers to a revision this peer has not seen yet.
    Future,
    WrongProtocol,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionState {
    pub protocol: u32,
    pub doc: String,
    pub epoch: u64,
    pub revision: u64,
    pub peers: Vec<Peer>,
}

/// In-memory session bookkeeping. A server owns one of these per open book.
#[derive(Clone, Debug)]
pub struct Session {
    doc: String,
    epoch: u64,
    revision: u64,
    peers: BTreeMap<PeerId, Peer>,
    seen: HashSet<String>,
    next_seq: u64,
    log: Vec<Operation>,
}

impl Session {
    pub fn new(doc: impl Into<String>) -> Self {
        Self {
            doc: doc.into(),
            epoch: 1,
            revision: 0,
            peers: BTreeMap::new(),
            seen: HashSet::new(),
            next_seq: 1,
            log: vec![],
        }
    }

    pub fn doc(&self) -> &str {
        &self.doc
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn log(&self) -> &[Operation] {
        &self.log
    }

    pub fn join(&mut self, mut peer: Peer) -> Peer {
        peer.last_seen = peer.last_seen.max(now_millis());
        self.peers.insert(peer.id.clone(), peer.clone());
        peer
    }

    pub fn leave(&mut self, id: &str) -> bool {
        self.peers.remove(id).is_some()
    }

    pub fn touch(&mut self, id: &str, cursor: Option<Cursor>, at: u64) -> bool {
        let Some(peer) = self.peers.get_mut(id) else { return false };
        peer.cursor = cursor;
        peer.last_seen = at;
        true
    }

    pub fn peers(&self) -> Vec<Peer> {
        self.peers.values().cloned().collect()
    }

    pub fn state(&self) -> SessionState {
        SessionState {
            protocol: PROTOCOL_VERSION,
            doc: self.doc.clone(),
            epoch: self.epoch,
            revision: self.revision,
            peers: self.peers(),
        }
    }

    /// Create a local operation. It is not considered applied until the host
    /// calls [`Self::apply`], so a transport can echo it back like any remote op.
    pub fn local(&mut self, author: impl Into<PeerId>, kind: OperationKind) -> Operation {
        let seq = self.next_seq;
        self.next_seq += 1;
        Operation {
            protocol: PROTOCOL_VERSION,
            id: Uuid::new_v4().to_string(),
            author: author.into(),
            seq,
            base: self.revision,
            kind,
        }
    }

    /// Accept an operation from any peer. This validates ordering only; the
    /// caller applies `kind` to its own document after `Applied`.
    pub fn apply(&mut self, op: Operation) -> Apply {
        if op.protocol != PROTOCOL_VERSION {
            return Apply::WrongProtocol;
        }
        if self.seen.contains(&op.id) {
            return Apply::Duplicate;
        }
        if op.base > self.revision {
            return Apply::Future;
        }
        if op.base < self.revision {
            return Apply::Stale;
        }
        self.seen.insert(op.id.clone());
        self.revision += 1;
        self.next_seq = self.next_seq.max(op.seq + 1);
        self.log.push(op);
        Apply::Applied
    }

    /// Drop operations that predate the current epoch (for a later reconnect
    /// handshake). The method exists now so transports can be built without
    /// changing the public shape of the session.
    pub fn reset_from_snapshot(&mut self, revision: u64, epoch: u64) {
        self.epoch = epoch;
        self.revision = revision;
        self.log.clear();
        self.seen.clear();
    }
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: &str, name: &str) -> Peer {
        Peer { id: id.into(), name: name.into(), color: "#b05226".into(), cursor: None, last_seen: 0 }
    }

    fn op(author: &str, base: u64, seq: u64) -> Operation {
        Operation {
            protocol: PROTOCOL_VERSION,
            id: format!("op-{author}-{seq}"),
            author: author.into(),
            seq,
            base,
            kind: OperationKind::SetBlockText { block: "b1".into(), text: "hi".into() },
        }
    }

    #[test]
    fn joins_presence_and_leaves() {
        let mut s = Session::new("doc-1");
        s.join(peer("a", "Ada"));
        s.join(peer("b", "Lin"));
        assert_eq!(s.peers().len(), 2);
        assert!(s.touch("a", Some(Cursor { block: "b1".into(), offset: 3 }), 99));
        assert_eq!(s.state().peers[0].cursor.as_ref().unwrap().offset, 3);
        assert!(s.leave("b"));
        assert_eq!(s.peers().len(), 1);
    }

    #[test]
    fn local_ops_are_sequenced_and_apply_once() {
        let mut s = Session::new("doc-1");
        let a = s.local("a", OperationKind::InsertBlock { after: None, xml: "<p>x</p>".into() });
        assert_eq!(s.revision(), 0);
        assert_eq!(s.apply(a.clone()), Apply::Applied);
        assert_eq!(s.revision(), 1);
        assert_eq!(s.apply(a), Apply::Duplicate);
    }

    #[test]
    fn stale_and_future_are_distinct() {
        let mut s = Session::new("doc-1");
        assert_eq!(s.apply(op("a", 1, 1)), Apply::Future);
        assert_eq!(s.apply(op("a", 0, 1)), Apply::Applied);
        assert_eq!(s.apply(op("b", 0, 1)), Apply::Stale);
        assert_eq!(s.apply(Operation { protocol: 9, ..op("c", 1, 1) }), Apply::WrongProtocol);
    }

    #[test]
    fn snapshot_reset_drops_log() {
        let mut s = Session::new("doc-1");
        s.apply(op("a", 0, 1));
        s.reset_from_snapshot(7, 2);
        assert_eq!(s.revision(), 7);
        assert_eq!(s.epoch(), 2);
        assert!(s.log().is_empty());
    }
}
