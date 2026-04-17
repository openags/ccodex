use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Item, Session, SessionId, Turn, TurnId};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum ProtocolEvent {
    SessionCreated(Session),
    SessionUpdated(Session),
    TurnStarted(Turn),
    TurnUpdated(Turn),
    TurnFinished(Turn),
    ItemAppended(Item),
    SessionArchived(SessionId),
    Warning { code: String, message: String },
    Error { code: String, message: String },
}

impl ProtocolEvent {
    pub fn session_id(&self) -> Option<&SessionId> {
        match self {
            Self::SessionCreated(session) | Self::SessionUpdated(session) => Some(&session.id),
            Self::TurnStarted(turn) | Self::TurnUpdated(turn) | Self::TurnFinished(turn) => Some(&turn.session_id),
            Self::ItemAppended(item) => {
                let _ = item;
                None
            }
            Self::SessionArchived(id) => Some(id),
            Self::Warning { .. } | Self::Error { .. } => None,
        }
    }

    pub fn turn_id(&self) -> Option<&TurnId> {
        match self {
            Self::TurnStarted(turn) | Self::TurnUpdated(turn) | Self::TurnFinished(turn) => Some(&turn.id),
            _ => None,
        }
    }
}
