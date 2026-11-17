//! What a machine is and what it reports.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Writes an enum whose variants have a fixed lowercase name, used in JSON
/// and in the server database.
macro_rules! named_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $text)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = UnknownName;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(UnknownName(s.to_owned())),
                }
            }
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown value: {0}")]
pub struct UnknownName(pub String);

named_enum! {
    /// Operating system of a machine.
    Os {
        Macos => "macos",
        Windows => "windows",
        Linux => "linux",
    }
}

named_enum! {
    /// What a machine is for (section 1 of the plan).
    MachineRole {
        Workstation => "workstation",
        Depot => "depot",
        Render => "render",
    }
}

named_enum! {
    /// Role of the person using a workstation. It sets what they can do and
    /// what their workstation downloads.
    PersonRole {
        Admin => "admin",
        Editor => "editor",
        Colorist => "colorist",
    }
}

named_enum! {
    ResolveEdition {
        Studio => "studio",
        Free => "free",
    }
}

/// An installed DaVinci Resolve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveInstall {
    pub edition: ResolveEdition,
    /// As Resolve writes it, `21.1` or `21.1.2`.
    pub version: String,
}

/// State of a machine as its agent sees it. Sent at every connection and
/// whenever it changes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    pub hostname: String,
    pub os_version: String,
    /// `None`: Resolve is not installed, or not found.
    pub resolve: Option<ResolveInstall>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for os in Os::ALL {
            assert_eq!(os.as_str().parse::<Os>(), Ok(*os));
            let json = serde_json::to_string(os).unwrap();
            assert_eq!(json, format!("\"{os}\""));
        }
        for role in MachineRole::ALL {
            assert_eq!(role.as_str().parse::<MachineRole>(), Ok(*role));
        }
        for role in PersonRole::ALL {
            assert_eq!(role.as_str().parse::<PersonRole>(), Ok(*role));
        }
        assert_eq!("Macos".parse::<Os>(), Err(UnknownName("Macos".to_owned())));
    }

    #[test]
    fn inventory_json() {
        let inventory = Inventory {
            hostname: "station-3".to_owned(),
            os_version: "macOS 26.1".to_owned(),
            resolve: Some(ResolveInstall {
                edition: ResolveEdition::Studio,
                version: "21.1".to_owned(),
            }),
        };
        let json = serde_json::to_value(&inventory).unwrap();
        assert_eq!(json["resolve"]["edition"], "studio");
        assert_eq!(
            serde_json::from_value::<Inventory>(json).unwrap(),
            inventory
        );
    }
}
