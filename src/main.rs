#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod native;
#[cfg(windows)]
mod startup;
#[cfg(windows)]
mod ui;

#[cfg(windows)]
fn main() {
    use std::sync::Arc;

    let _instance = match native::SingleInstance::acquire() {
        Ok(instance) => instance,
        Err(error) => {
            eprintln!("Wiggler did not start: {error}");
            return;
        }
    };
    let store = wiggler::settings::SettingsStore::for_current_user();
    let settings = store.load().unwrap_or_default();
    if settings.start_with_windows {
        let _ = startup::repair();
    }
    let control = Arc::new(native::ControlState::new(settings));
    let ui = match ui::TrayUi::start(Arc::clone(&control), store) {
        Ok(ui) => ui,
        Err(error) => {
            eprintln!("Wiggler could not create its tray: {error}");
            std::process::exit(1);
        }
    };
    let runtime = match native::NativeRuntime::start() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Wiggler could not start: {error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = runtime.run(control) {
        eprintln!("Wiggler stopped: {error}");
    }
    ui.join();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Wiggler requires Windows.");
}
