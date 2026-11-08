//! Transport-neutral messages for the MarkdownX collaboration server.
//!
//! The browser editor, the future desktop client and the Rust server all use
//! this envelope. Keeping it in `fdl_core` avoids a second schema.

use serde::{Deserialize, Serialize};

/// A peer as seen in the browser editor. The native collab layer has a richer
/// `collab::Peer`; this is the wire shape used by the shared HTTP/WS protocol.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Full-text save. `base` is the revision the editor started from.
    Save {
        base: u64,
        content: String,
        #[serde(default)]
        name: Option<String>,
    },
    Cursor { cursor: usize },
    Ping,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Snapshot {
        client_id: String,
        id: String,
        title: String,
        rev: u64,
        content: String,
        peers: Vec<Peer>,
    },
    Update {
        rev: u64,
        title: String,
        by: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<String>,
        at: u64,
        content: String,
    },
    Conflict {
        rev: u64,
        content: String,
        merged: String,
        by: String,
    },
    Presence {
        peers: Vec<Peer>,
    },
    Saved {
        rev: u64,
        by: String,
        at: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        commit: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        warning: Option<String>,
    },
    Error {
        message: String,
    },
    Pong,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_snapshot_keep_their_wire_names() {
        let save = ClientMessage::Save { base: 3, content: "hello".into(), name: Some("Ada".into()) };
        let json = serde_json::to_value(&save).unwrap();
        assert_eq!(json["type"], "save");
        assert_eq!(json["base"], 3);
        assert_eq!(json["content"], "hello");

        let snap = ServerMessage::Snapshot {
            client_id: "c1".into(),
            id: "note".into(),
            title: "Note".into(),
            rev: 2,
            content: "body".into(),
            peers: vec![],
        };
        let json = serde_json::to_value(&snap).unwrap();
        assert_eq!(json["type"], "snapshot");
        assert_eq!(json["client_id"], "c1");
        assert_eq!(json["rev"], 2);
    }

    #[test]
    fn update_always_carries_content() {
        let update = ServerMessage::Update {
            rev: 4,
            title: "T".into(),
            by: "Ada".into(),
            origin: Some("c1".into()),
            at: 1,
            content: "merged text".into(),
        };
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["type"], "update");
        assert_eq!(json["origin"], "c1");
        assert_eq!(json["content"], "merged text");
    }

    #[test]
    fn browser_cursor_is_a_number() {
        let msg: ClientMessage = serde_json::from_str(r#"{"type":"cursor","cursor":12}"#).unwrap();
        assert_eq!(msg, ClientMessage::Cursor { cursor: 12 });
    }
}
