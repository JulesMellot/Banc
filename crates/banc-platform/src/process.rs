//! The current process.

/// Exit code of a program that started its replacement. Not a success, so
/// that launchd starts the program again if the replacement did not survive
/// the end of the job.
const RESTARTING: i32 = 75;

/// Starts the program on disk again with the same arguments, then ends this
/// process. Returns only if the new process could not be started.
pub fn restart() -> std::io::Result<()> {
    let program = std::env::current_exe()?;
    let mut command = std::process::Command::new(program);
    command.args(std::env::args_os().skip(1));
    detach(&mut command);
    command.spawn()?;
    std::process::exit(RESTARTING);
}

/// launchd ends what remains of a job's process group when the job ends.
#[cfg(unix)]
fn detach(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn detach(_command: &mut std::process::Command) {}
