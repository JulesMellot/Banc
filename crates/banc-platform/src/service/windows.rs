use std::ffi::OsString;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use windows_service::service::{
    ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

use super::{SERVICE_ARG, WINDOWS_NAME};

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
