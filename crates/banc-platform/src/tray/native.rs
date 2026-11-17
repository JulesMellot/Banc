use std::future::Future;
use std::time::Duration;

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::run_return::EventLoopExtRunReturn;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use super::{Tray, TrayError};

enum UserEvent {
    Status(String),
    Menu(MenuEvent),
    Quit,
}

pub(super) fn run<S, F>(
    tray: Tray,
    interval: Duration,
    status: S,
    mut open: impl FnMut() + 'static,
) -> Result<(), TrayError>
where
    S: FnMut() -> F + Send + 'static,
    F: Future<Output = String>,
{
    let failed = |e: &dyn std::fmt::Display| TrayError::Failed(e.to_string());
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    {
        // An icon in the menu bar, none in the Dock.
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }

    let proxy = event_loop.create_proxy();
    let menu_proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = menu_proxy.send_event(UserEvent::Menu(event));
    }));
    watch(proxy, interval, status);

    let icon =
        Icon::from_rgba(tray.icon_rgba, tray.icon_size, tray.icon_size).map_err(|e| failed(&e))?;
    let line = MenuItem::new("…", false, None);
    let open_item = MenuItem::new(&tray.open_label, true, None);
    let quit_item = MenuItem::new(&tray.quit_label, true, None);
    let menu = Menu::new();
    menu.append_items(&[
        &line,
        &PredefinedMenuItem::separator(),
        &open_item,
        &quit_item,
    ])
    .map_err(|e| failed(&e))?;

    let mut icon_slot = None;
    let mut setup = Some((icon, menu));
    let mut error = None;
    event_loop.run_return(|event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            // macOS wants the icon created once the loop runs.
            Event::NewEvents(StartCause::Init) => {
                if let Some((icon, menu)) = setup.take() {
                    match TrayIconBuilder::new()
                        .with_menu(Box::new(menu))
                        .with_icon(icon)
                        .with_tooltip("Banc")
                        .build()
                    {
                        Ok(created) => icon_slot = Some(created),
                        Err(e) => {
                            error = Some(failed(&e));
                            *control_flow = ControlFlow::Exit;
                        }
                    }
                }
            }
            Event::UserEvent(UserEvent::Status(text)) => {
                line.set_text(&text);
                if let Some(icon) = &icon_slot {
                    let _ = icon.set_tooltip(Some(format!("Banc: {text}")));
                }
            }
            Event::UserEvent(UserEvent::Menu(event)) => {
                if event.id == open_item.id() {
                    open();
                } else if event.id == quit_item.id() {
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(UserEvent::Quit) => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    });
    drop(icon_slot);
    error.map_or(Ok(()), Err)
}

/// Background thread: the status every `interval`, and the OS's request to
/// stop.
fn watch<S, F>(proxy: EventLoopProxy<UserEvent>, interval: Duration, mut status: S)
where
    S: FnMut() -> F + Send + 'static,
    F: Future<Output = String>,
{
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let text = status().await;
                        if proxy.send_event(UserEvent::Status(text)).is_err() {
                            return;
                        }
                    }
                    _ = crate::signal::shutdown() => {
                        let _ = proxy.send_event(UserEvent::Quit);
                        return;
                    }
                }
            }
        });
    });
}
