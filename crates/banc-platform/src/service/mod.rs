//! The agent as a system service: systemd on Linux, launchd on macOS, the
//! service control manager on Windows.
//!
//! Installation needs administrator rights. The installers of phase 7 call
//! the same code; until then, `banc-agent service install`.

use std::path::Path;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as os;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as os;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as os;

/// systemd unit name, launchd label, Windows service name.
pub const SYSTEMD_UNIT: &str = "banc-agent.service";
pub const LAUNCHD_LABEL: &str = "com.banc.agent";
pub const WINDOWS_NAME: &str = "BancAgent";

/// Argument that the service manager passes to the agent.
pub const SERVICE_ARG: &str = "--service";

/// Registers the agent, starts it now and at every boot. Replaces a
/// previous registration.
pub fn install(program: &Path) -> std::io::Result<()> {
    os::install(program)
}

/// Stops the agent and removes its registration. Its data stays.
pub fn uninstall() -> std::io::Result<()> {
    os::uninstall()
}

/// Runs the agent's main function the way the service manager expects. On
/// Windows, it reports to the service control manager and turns its stop
/// request into [`crate::signal::request_shutdown`]. Elsewhere it just
/// calls `main`.
pub fn run(main: impl FnOnce() -> u32 + Send + 'static) -> std::io::Result<u32> {
    os::run(main)
}

/// systemd unit of the agent.
pub fn systemd_unit(program: &Path) -> String {
    format!(
        "[Unit]
Description=Banc agent
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=\"{}\" {SERVICE_ARG}
Restart=always
RestartSec=5
RuntimeDirectory=banc
StateDirectory=banc
LogsDirectory=banc

[Install]
WantedBy=multi-user.target
",
        program.display()
    )
}

/// launchd property list of the agent.
pub fn launchd_plist(program: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LAUNCHD_LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{}</string>
		<string>{SERVICE_ARG}</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<true/>
	<key>ProcessType</key>
	<string>Background</string>
</dict>
</plist>
"#,
        xml_escape(&program.display().to_string())
    )
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_text() {
        let unit = systemd_unit(Path::new("/usr/bin/banc-agent"));
        assert!(
            unit.contains("ExecStart=\"/usr/bin/banc-agent\" --service\n"),
            "{unit}"
        );
        assert!(unit.contains("WantedBy=multi-user.target"));
        assert!(unit.contains("RuntimeDirectory=banc"));
    }

    #[test]
    fn plist_text() {
        let plist = launchd_plist(Path::new(
            "/Applications/Banc & Co.app/Contents/MacOS/banc-agent",
        ));
        assert!(plist.contains("<string>com.banc.agent</string>"));
        assert!(plist.contains("Banc &amp; Co.app"), "{plist}");
        assert!(plist.contains("<string>--service</string>"));
    }
}
