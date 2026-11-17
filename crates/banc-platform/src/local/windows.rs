use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use tokio::net::windows::named_pipe::{
    ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
};
use windows_sys::Win32::Foundation::{LocalFree, ERROR_PIPE_BUSY};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

pub type ServerStream = NamedPipeServer;
pub type ClientStream = NamedPipeClient;

/// LocalSystem and administrators: full control. Authenticated users: read
/// and write, so that the interface of any session can talk to the agent.
const SDDL: &str = "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;AU)";

pub(super) struct Listener {
    name: PathBuf,
    next: NamedPipeServer,
}

impl Listener {
    pub(super) fn bind(endpoint: &Path) -> std::io::Result<Self> {
        let next = create(endpoint, true)?;
        Ok(Self {
            name: endpoint.to_path_buf(),
            next,
        })
    }

    pub(super) async fn accept(&mut self) -> std::io::Result<ServerStream> {
        self.next.connect().await?;
        // The next client gets a new instance of the pipe.
        let fresh = create(&self.name, false)?;
        Ok(std::mem::replace(&mut self.next, fresh))
    }
}

fn create(name: &Path, first: bool) -> std::io::Result<NamedPipeServer> {
    let sddl: Vec<u16> = std::ffi::OsStr::new(SDDL)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: `sddl` is a NUL-terminated wide string; on success Windows
    // allocates the descriptor, freed below with LocalFree.
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if converted == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true);
    // SAFETY: `attributes` and the descriptor it points to are valid for
    // the duration of the call, which copies them into the pipe object.
    let server = unsafe {
        options.create_with_security_attributes_raw(
            name,
            (&mut attributes as *mut SECURITY_ATTRIBUTES).cast::<c_void>(),
        )
    };
    // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
    unsafe {
        LocalFree(descriptor);
    }
    server
}

pub(super) async fn connect(endpoint: &Path) -> std::io::Result<ClientStream> {
    loop {
        match ClientOptions::new().open(endpoint) {
            Ok(client) => return Ok(client),
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

/// The pipe namespace is flat: the folder's hash keeps names apart.
pub(super) fn endpoint_in(dir: &Path, name: &str) -> PathBuf {
    let mut hasher = std::hash::DefaultHasher::new();
    std::hash::Hash::hash(dir, &mut hasher);
    let hash = std::hash::Hasher::finish(&hasher);
    PathBuf::from(format!(r"\\.\pipe\banc-{name}-{hash:016x}"))
}
