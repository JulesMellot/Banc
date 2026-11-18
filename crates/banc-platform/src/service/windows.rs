use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
    ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegDeleteKeyValueW, RegSetKeyValueW, HKEY_LOCAL_MACHINE, REG_SZ,
};
use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};

use super::{SERVICE_ARG, WINDOWS_NAME, WINDOWS_SESSION_NAME};

/// The programs Windows starts when any user logs in.
const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
/// How long a stopping service is given.
const STOP_TIMEOUT: Duration = Duration::from_secs(30);

fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

fn error(error: windows_service::Error) -> std::io::Error {
    std::io::Error::other(error.to_string())
}

pub(super) fn install(program: &Path) -> std::io::Result<()> {
    let _ = uninstall();
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )
    .map_err(error)?;
    let info = ServiceInfo {
        name: OsString::from(WINDOWS_NAME),
        display_name: OsString::from("Banc agent"),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: program.to_path_buf(),
        launch_arguments: vec![OsString::from(SERVICE_ARG)],
        dependencies: Vec::new(),
        // LocalSystem.
        account_name: None,
        account_password: None,
    };
    let service = manager
        .create_service(&info, ServiceAccess::START | ServiceAccess::CHANGE_CONFIG)
        .map_err(error)?;
    service
        .set_description("Sets up this DaVinci Resolve workstation for the team.")
        .map_err(error)?;
    service.start::<&str>(&[]).map_err(error)
}

pub(super) fn uninstall() -> std::io::Result<()> {
    remove_session_at_login();
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(error)?;
    let service = manager
        .open_service(
            WINDOWS_NAME,
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
        )
        .map_err(error)?;
    let _ = service.stop();
    for _ in 0..50 {
        match service.query_status() {
            Ok(status) if status.current_state != ServiceState::Stopped => {
                std::thread::sleep(Duration::from_millis(100));
            }
            _ => break,
        }
    }
    service.delete().map_err(error)
}

fn open(access: ServiceAccess) -> std::io::Result<windows_service::service::Service> {
    ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
        .map_err(error)?
        .open_service(WINDOWS_NAME, access)
        .map_err(error)
}

pub(super) fn stop() -> std::io::Result<()> {
    let service = open(ServiceAccess::QUERY_STATUS | ServiceAccess::STOP)?;
    // Already stopped: the wait below ends at once.
    let _ = service.stop();
    let deadline = std::time::Instant::now() + STOP_TIMEOUT;
    while std::time::Instant::now() < deadline {
        if service.query_status().map_err(error)?.current_state == ServiceState::Stopped {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(std::io::Error::other("the service did not stop"))
}

pub(super) fn start() -> std::io::Result<()> {
    open(ServiceAccess::START)?
        .start::<&str>(&[])
        .map_err(error)
}

/// Restarts the service 5 seconds after any failure, and starts the tray
/// icon when a user logs in.
pub(super) fn keep_running(session: &Path) -> std::io::Result<()> {
    let service = open(ServiceAccess::START | ServiceAccess::CHANGE_CONFIG)?;
    let restart = ServiceAction {
        action_type: ServiceActionType::Restart,
        delay: Duration::from_secs(5),
    };
    service
        .update_failure_actions(ServiceFailureActions {
            reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86_400)),
            reboot_msg: None,
            command: None,
            actions: Some(vec![restart.clone(), restart.clone(), restart]),
        })
        .map_err(error)?;
    service
        .set_failure_actions_on_non_crash_failures(true)
        .map_err(error)?;
    session_at_login(session)
}

fn session_at_login(session: &Path) -> std::io::Result<()> {
    let key = wide(RUN_KEY.as_ref());
    let name = wide(WINDOWS_SESSION_NAME.as_ref());
    let value = wide(super::windows_run_value(session).as_ref());
    // SAFETY: the strings are NUL-terminated and `value` is as long as the
    // size given, in bytes.
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_LOCAL_MACHINE,
            key.as_ptr(),
            name.as_ptr(),
            REG_SZ,
            value.as_ptr().cast(),
            (value.len() * 2) as u32,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(status as i32))
    }
}

fn remove_session_at_login() {
    let key = wide(RUN_KEY.as_ref());
    let name = wide(WINDOWS_SESSION_NAME.as_ref());
    // SAFETY: the strings are NUL-terminated.
    let status = unsafe { RegDeleteKeyValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), name.as_ptr()) };
    if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
        tracing::warn!(status, "removing the tray icon from the login programs");
    }
}

/// A process of its own: the service control manager does not end it with
/// the service.
pub(super) fn launch_updater(program: &Path, args: &[OsString]) -> std::io::Result<()> {
    std::process::Command::new(program)
        .args(args)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(|_| ())
}

pub(super) fn finish_updater() {}

type Main = Box<dyn FnOnce() -> u32 + Send>;

static MAIN: Mutex<Option<Main>> = Mutex::new(None);
static EXIT: Mutex<u32> = Mutex::new(0);

define_windows_service!(ffi_service_main, service_main);

pub(super) fn run(main: impl FnOnce() -> u32 + Send + 'static) -> std::io::Result<u32> {
    *MAIN.lock().unwrap_or_else(|p| p.into_inner()) = Some(Box::new(main));
    // Blocks until the service stops.
    service_dispatcher::start(WINDOWS_NAME, ffi_service_main).map_err(error)?;
    Ok(*EXIT.lock().unwrap_or_else(|p| p.into_inner()))
}

fn service_main(_arguments: Vec<OsString>) {
    let handler = service_control_handler::register(WINDOWS_NAME, |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            crate::signal::request_shutdown();
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    });
    let Ok(handler) = handler else {
        return;
    };
    let status = |state, code| ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: if state == ServiceState::Running {
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
        } else {
            ServiceControlAccept::empty()
        },
        exit_code: ServiceExitCode::Win32(code),
        checkpoint: 0,
        wait_hint: Duration::from_secs(10),
        process_id: None,
    };
    let _ = handler.set_service_status(status(ServiceState::Running, 0));
    let main = MAIN.lock().unwrap_or_else(|p| p.into_inner()).take();
    let code = main.map_or(1, |main| main());
    *EXIT.lock().unwrap_or_else(|p| p.into_inner()) = code;
    let _ = handler.set_service_status(status(ServiceState::Stopped, code));
}
