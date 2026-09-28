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
    let settings = match store.load() {
        Ok(settings) => settings,
        Err(error) => {
            ui::show_fatal_error(&format!("Wiggler could not read its settings: {error}"));
            return;
        }
    };
    let _ = if settings.start_with_windows {
        startup::repair()
    } else {
        startup::set_enabled(false)
    };
    let control = Arc::new(native::ControlState::new(settings));
    let ui = match ui::TrayUi::start(Arc::clone(&control), store) {
        Ok(ui) => ui,
        Err(error) => {
            eprintln!("Wiggler could not create its tray: {error}");
            ui::show_fatal_error(&format!("Wiggler could not create its tray: {error}"));
            return;
        }
    };
    let runtime = match native::NativeRuntime::start() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Wiggler could not start: {error}");
            ui.join();
            ui::show_fatal_error(&format!("Wiggler could not start: {error}"));
            return;
        }
    };

    let result = runtime.run(control);
    ui.join();
    if let Err(error) = result {
        eprintln!("Wiggler stopped: {error}");
        ui::show_fatal_error(&format!("Wiggler stopped: {error}"));
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Wiggler requires Windows.");
}
