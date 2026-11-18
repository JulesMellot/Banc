//! Local messages between the interface (window, tray icon, command line)
//! and the agent, over the agent's local socket (named pipe on Windows).
//!
//! Same framing as the control channel: `control::write_frame` and
//! `control::read_frame`. One request, one response, per connection.
//!
//! Nothing here runs a command on the workstation: the requests are the
//! enrollment, which only works on a machine without a team, and reading the
//! status.

use serde::{Deserialize, Serialize};

use crate::keys::Fingerprint;
use crate::machine::{Inventory, MachineRole, PersonRole};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Status,
    /// Join a team. Refused while the machine belongs to one: it must have
    /// never joined, or have been revoked by the server.
    Enroll {
        invite: InviteInput,
        /// Name shown in the admin. Default: the computer name.
        name: Option<String>,
    },
}

/// What the user gave: a clicked link, or the server address and the key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InviteInput {
    Link { link: String },
    Key { server: String, key: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Status(AgentStatus),
    Error { code: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentStatus {
    pub version: String,
    /// `None`: the machine has not joined a team yet.
    pub enrollment: Option<Enrollment>,
    pub inventory: Inventory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enrollment {
    pub server: String,
    pub machine_id: i64,
    pub name: String,
    pub machine_role: MachineRole,
    pub person_role: PersonRole,
    pub admin_fingerprint: Fingerprint,
    pub connection: Connection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    Connecting,
    Connected,
    /// The server no longer knows this machine: the admin revoked it.
    Revoked,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_shape() {
        let request = Request::Enroll {
            invite: InviteInput::Key {
                server: "https://banc.example.com".to_owned(),
                key: "abc-def".to_owned(),
            },
            name: None,
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["type"], "enroll");
        assert_eq!(json["invite"]["kind"], "key");
        assert_eq!(serde_json::from_value::<Request>(json).unwrap(), request);

        let error = Response::Error {
            code: "already_enrolled".to_owned(),
            message: "x".to_owned(),
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["type"], "error");
    }
}
