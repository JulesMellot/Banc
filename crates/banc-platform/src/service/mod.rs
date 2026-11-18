//! The agent as a system service: systemd on Linux, launchd on macOS, the
//! service control manager on Windows.
//!
//! Installation needs administrator rights. The installers of phase 7 call
//! the same code; until then, `banc-agent service install`.

use std::ffi::OsString;
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
/// launchd label of the tray icon, started in each user's session.
pub const SESSION_LABEL: &str = "com.banc.session";
/// launchd label and systemd unit of an update being applied.
pub const UPDATER_LABEL: &str = "com.banc.update";
/// Name of the tray icon in the programs Windows starts at login.
pub const WINDOWS_SESSION_NAME: &str = "BancSession";

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

/// Stops the running agent and waits until it has stopped. It does not come
/// back until [`start`].
pub fn stop() -> std::io::Result<()> {
    os::stop()
}

/// Starts the registered agent.
pub fn start() -> std::io::Result<()> {
    os::start()
}

/// What keeps Banc running on this machine, checked at every start of the
/// service, so that a machine that was installed earlier or updated gets it
/// too: the service restarts after a failure, and `session` (the tray icon)
/// starts when a user logs in.
pub fn keep_running(session: &Path) -> std::io::Result<()> {
    os::keep_running(session)
}

/// Runs `program` outside the service, so that it survives the service being
/// stopped. An update replaces the agent from there.
pub fn launch_updater(program: &Path, args: &[OsString]) -> std::io::Result<()> {
    os::launch_updater(program, args)
}

/// Called by the program of [`launch_updater`] as its last action.
pub fn finish_updater() {
    os::finish_updater();
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

/// launchd property list of the tray icon. A crash brings it back; choosing
/// "Quitter" in its menu does not.
pub fn session_plist(program: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{SESSION_LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{}</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<dict>
		<key>SuccessfulExit</key>
		<false/>
	</dict>
	<key>LimitLoadToSessionType</key>
	<string>Aqua</string>
	<key>ProcessType</key>
	<string>Interactive</string>
</dict>
</plist>
"#,
        xml_escape(&program.display().to_string())
    )
}

/// launchd property list of an update being applied: a job of its own, so
/// that stopping the service does not end it. If the machine restarts in the
/// middle, the update resumes at the next boot.
pub fn updater_plist(program: &Path, args: &[OsString]) -> String {
    let mut arguments = format!(
        "\t\t<string>{}</string>\n",
        xml_escape(&program.display().to_string())
    );
    for arg in args {
        arguments.push_str(&format!(
            "\t\t<string>{}</string>\n",
            xml_escape(&arg.to_string_lossy())
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{UPDATER_LABEL}</string>
	<key>ProgramArguments</key>
	<array>
{arguments}	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>AbandonProcessGroup</key>
	<true/>
</dict>
</plist>
"#
    )
}

/// The command line Windows runs at login: the path in quotes, it may hold
/// spaces (`C:\Program Files\Banc`).
pub fn windows_run_value(program: &Path) -> String {
    format!("\"{}\"", program.display())
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

    #[test]
    fn session_starts_at_login_and_stays_quit_when_asked() {
        let plist = session_plist(Path::new(
            "/Applications/Banc.app/Contents/MacOS/banc-session",
        ));
        assert!(plist.contains("<string>com.banc.session</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n\t<true/>"), "{plist}");
        assert!(
            plist.contains("<key>SuccessfulExit</key>\n\t\t<false/>"),
            "{plist}"
        );
        assert!(plist.contains("<string>Aqua</string>"));
    }

    #[test]
    fn updater_job_has_its_arguments() {
        let args = [
            OsString::from("update-apply"),
            OsString::from("/a & b/p.json"),
        ];
        let plist = updater_plist(Path::new("/tmp/banc-agent"), &args);
        assert!(plist.contains("<string>com.banc.update</string>"));
        assert!(plist.contains("<string>/tmp/banc-agent</string>"));
        assert!(plist.contains("<string>update-apply</string>"));
        assert!(
            plist.contains("<string>/a &amp; b/p.json</string>"),
            "{plist}"
        );
    }

    #[test]
    fn windows_login_command_is_quoted() {
        assert_eq!(
            windows_run_value(Path::new(r"C:\Program Files\Banc\banc-session.exe")),
            r#""C:\Program Files\Banc\banc-session.exe""#
        );
    }
}
