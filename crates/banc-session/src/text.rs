//! Interface text of the tray icon, read from the same `fr.json` as the
//! window: one place for all French text.

use std::collections::HashMap;

use banc_proto::local::{AgentStatus, Connection};

static FR: &str = include_str!("../../../ui/src/lib/i18n/fr.json");

pub struct Text(HashMap<String, String>);

impl Text {
    pub fn load() -> Self {
        Self(serde_json::from_str(FR).unwrap_or_default())
    }

    /// The text of `key`, or the key itself if missing (tests catch that).
    pub fn get(&self, key: &str) -> String {
        self.0.get(key).cloned().unwrap_or_else(|| key.to_owned())
    }

    /// The first line of the menu.
    pub fn status_line(&self, status: Option<&AgentStatus>) -> String {
        let Some(status) = status else {
            return self.get("error.agent_unreachable");
        };
        match &status.enrollment {
            None => self.get("tray.not_enrolled"),
            Some(enrollment) => self.get(match enrollment.connection {
                Connection::Connecting => "connection.connecting",
                Connection::Connected => "connection.connected",
                Connection::Revoked => "connection.revoked",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use banc_proto::keys::{Fingerprint, SecretKey};
    use banc_proto::local::Enrollment;
    use banc_proto::machine::{Inventory, MachineRole, PersonRole};

    use super::*;

    #[test]
    fn every_key_exists() {
        let text = Text::load();
        for key in [
            "tray.open",
            "tray.quit",
            "tray.not_enrolled",
            "error.agent_unreachable",
            "connection.connecting",
            "connection.connected",
            "connection.revoked",
        ] {
            assert_ne!(text.get(key), key, "{key} missing from fr.json");
        }
    }

    #[test]
    fn status_lines() {
        let text = Text::load();
        assert_eq!(text.status_line(None), text.get("error.agent_unreachable"));
        let mut status = AgentStatus {
            version: "0".to_owned(),
            enrollment: None,
            inventory: Inventory::default(),
        };
        assert_eq!(
            text.status_line(Some(&status)),
            text.get("tray.not_enrolled")
        );
        status.enrollment = Some(Enrollment {
            server: "https://banc.example.com".to_owned(),
            machine_id: 1,
            name: "Station".to_owned(),
            machine_role: MachineRole::Workstation,
            person_role: PersonRole::Editor,
            admin_fingerprint: Fingerprint::of(&SecretKey::from_bytes(&[1; 32]).public()),
            connection: Connection::Connected,
        });
        assert_eq!(
            text.status_line(Some(&status)),
            text.get("connection.connected")
        );
    }
}
