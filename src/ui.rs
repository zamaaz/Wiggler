use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::Arc;
use std::thread;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;

use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    CreateFontW, DeleteObject, GetSysColorBrush, SetBkMode, UpdateWindow, DEFAULT_CHARSET,
    FW_NORMAL, HDC, HFONT, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows_sys::Win32::UI::Controls::{BST_CHECKED, BST_UNCHECKED, EM_SETLIMITTEXT};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::native::ControlState;
use crate::startup;
use wiggler::core::{Profile, Settings};
use wiggler::settings::SettingsStore;

const WM_TRAY: u32 = WM_APP + 1;
const SUSPEND_SLEEP: u32 = 1;
const SUSPEND_LOCK: u32 = 2;
const TRAY_ID: u32 = 1;
const MENU_SETTINGS: u16 = 100;
const MENU_PAUSE: u16 = 101;
const MENU_EXIT: u16 = 102;
const MENU_REPAIR_STARTUP: u16 = 103;
const ID_PROFILE: u16 = 200;
const ID_DELAY: u16 = 201;
const ID_AMPLITUDE: u16 = 202;
const ID_SPEED: u16 = 203;
const ID_STARTUP: u16 = 204;
const ID_PAUSE: u16 = 205;
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

pub struct TrayUi {
    thread: thread::JoinHandle<()>,
    window: isize,
    control: Arc<ControlState>,
}

struct UiContext {
    control: Arc<ControlState>,
    store: SettingsStore,
    tray_window: HWND,
    settings_window: HWND,
    font: HFONT,
    labels: [HWND; 4],
    profile: HWND,
    delay: HWND,
    amplitude: HWND,
    speed: HWND,
    seconds: HWND,
    pixels: HWND,
    startup: HWND,
    pause: HWND,
    save_error_shown: bool,
}

impl TrayUi {
    pub fn start(control: Arc<ControlState>, store: SettingsStore) -> Result<Self, String> {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread_control = Arc::clone(&control);
        let owner_control = Arc::clone(&control);
        let thread = thread::spawn(move || {
            if let Err(error) = run_ui(control, store, &ready_tx) {
                thread_control.exit.store(true, Ordering::Release);
                let _ = ready_tx.send(Err(error.clone()));
                eprintln!("tray stopped: {error}");
            }
        });
        match ready_rx.recv() {
            Ok(Ok(window)) => Ok(Self {
                thread,
                window,
                control: owner_control,
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                let _ = thread.join();
                Err("tray thread exited before it was ready".to_string())
            }
        }
    }

    pub fn join(self) {
        if !self.control.exit.load(Ordering::Acquire) && !self.thread.is_finished() {
            unsafe { PostMessageW(self.window as HWND, WM_CLOSE, 0, 0) };
        }
        let _ = self.thread.join();
    }
}

fn run_ui(
    control: Arc<ControlState>,
    store: SettingsStore,
    ready: &SyncSender<Result<isize, String>>,
) -> Result<(), String> {
    let instance = unsafe { GetModuleHandleW(null()) };
    if instance.is_null() {
        return Err(last_error("GetModuleHandleW"));
    }
    let class_name = wide("WigglerTrayWindow");
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        ..unsafe { std::mem::zeroed() }
    };
    if unsafe { RegisterClassW(&class) } == 0 {
        let error = unsafe { GetLastError() };
        if error != ERROR_CLASS_ALREADY_EXISTS {
            return Err(format!("RegisterClassW failed with Win32 error {error}"));
        }
    }
    let settings_class_name = wide("WigglerSettingsWindow");
    let settings_class = WNDCLASSW {
        lpfnWndProc: Some(settings_proc),
        hInstance: instance,
        lpszClassName: settings_class_name.as_ptr(),
        hCursor: unsafe { LoadCursorW(null_mut(), IDC_ARROW) },
        hIcon: unsafe { LoadIconW(instance, std::ptr::without_provenance::<u16>(1)) },
        hbrBackground: (5 + 1) as _,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe { RegisterClassW(&settings_class) };

    let mut context = Box::new(UiContext {
        control,
        store,
        tray_window: null_mut(),
        settings_window: null_mut(),
        font: null_mut(),
        labels: [null_mut(); 4],
        profile: null_mut(),
        delay: null_mut(),
        amplitude: null_mut(),
        speed: null_mut(),
        seconds: null_mut(),
        pixels: null_mut(),
        startup: null_mut(),
        pause: null_mut(),
        save_error_shown: false,
    });
    let context_ptr = context.as_mut() as *mut UiContext;
    let title = wide("Wiggler");
    let window = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            context_ptr as *const c_void,
        )
    };
    if window.is_null() {
        return Err(last_error("CreateWindowExW"));
    }
    TASKBAR_CREATED.store(
        unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
        Ordering::Release,
    );
    context.tray_window = window;
    if unsafe { WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION) } == 0 {
        let error = last_error("WTSRegisterSessionNotification");
        unsafe { DestroyWindow(window) };
        return Err(error);
    }
    if let Err(error) = add_tray_icon(window) {
        unsafe {
            WTSUnRegisterSessionNotification(window);
            DestroyWindow(window);
        }
        return Err(error);
    }
    let _ = ready.send(Ok(window as isize));
    let mut message = unsafe { std::mem::zeroed() };
    loop {
        let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        if result <= 0 {
            break;
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &notify_data(window));
        WTSUnRegisterSessionNotification(window);
        DestroyWindow(window);
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let mut context = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut UiContext;
    if message == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        context = create.lpCreateParams as *mut UiContext;
        SetWindowLongPtrW(window, GWLP_USERDATA, context as isize);
    }
    if context.is_null() {
        return DefWindowProcW(window, message, wparam, lparam);
    }
    let context = &mut *context;
    match message {
        WM_COMMAND => handle_command(context, wparam as u16),
        message if message == TASKBAR_CREATED.load(Ordering::Acquire) => {
            let _ = add_tray_icon(window);
            0
        }
        WM_POWERBROADCAST if wparam == PBT_APMSUSPEND as usize => {
            context
                .control
                .suspended
                .fetch_or(SUSPEND_SLEEP, Ordering::AcqRel);
            context
                .control
                .lifecycle_generation
                .fetch_add(1, Ordering::AcqRel);
            1
        }
        WM_POWERBROADCAST if wparam == PBT_APMRESUMEAUTOMATIC as usize => {
            context
                .control
                .suspended
                .fetch_and(!SUSPEND_SLEEP, Ordering::AcqRel);
            context
                .control
                .lifecycle_generation
                .fetch_add(1, Ordering::AcqRel);
            1
        }
        WM_WTSSESSION_CHANGE if wparam == WTS_SESSION_LOCK as usize => {
            context
                .control
                .suspended
                .fetch_or(SUSPEND_LOCK, Ordering::AcqRel);
            context
                .control
                .lifecycle_generation
                .fetch_add(1, Ordering::AcqRel);
            0
        }
        WM_WTSSESSION_CHANGE if wparam == WTS_SESSION_UNLOCK as usize => {
            context
                .control
                .suspended
                .fetch_and(!SUSPEND_LOCK, Ordering::AcqRel);
            context
                .control
                .lifecycle_generation
                .fetch_add(1, Ordering::AcqRel);
            0
        }
        WM_DISPLAYCHANGE | WM_DEVICECHANGE | WM_SETTINGCHANGE => {
            context
                .control
                .lifecycle_generation
                .fetch_add(1, Ordering::AcqRel);
            0
        }
        WM_CLOSE if window == context.settings_window => {
            DestroyWindow(window);
            0
        }
        WM_DESTROY if window == context.settings_window => {
            context.settings_window = null_mut();
            0
        }
        WM_DESTROY => {
            context
                .control
                .exit
                .store(true, std::sync::atomic::Ordering::Release);
            PostQuitMessage(0);
            0
        }
        WM_TRAY if lparam as u32 == WM_RBUTTONUP => {
            show_tray_menu(context);
            0
        }
        WM_TRAY if lparam as u32 == WM_LBUTTONDBLCLK => {
            show_settings(context);
            0
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn handle_command(context: &mut UiContext, command: u16) -> LRESULT {
    match command {
        MENU_SETTINGS => show_settings(context),
        ID_PAUSE => {
            toggle_pause(context);
        }
        MENU_PAUSE => {
            toggle_pause(context);
        }
        MENU_EXIT => {
            context
                .control
                .exit
                .store(true, std::sync::atomic::Ordering::Release);
            DestroyWindow(context.tray_window);
        }
        MENU_REPAIR_STARTUP => {
            let enabled = context
                .control
                .settings
                .lock()
                .map(|settings| settings.start_with_windows)
                .unwrap_or(false);
            let result = if enabled {
                startup::repair()
            } else {
                startup::set_enabled(false)
            };
            if let Err(error) = result {
                show_error(
                    context.tray_window,
                    &format!("Could not repair startup: {error}"),
                );
            }
        }
        ID_PROFILE | ID_DELAY | ID_AMPLITUDE | ID_SPEED | ID_STARTUP => update_settings(context),
        _ => {}
    }
    0
}

unsafe fn toggle_pause(context: &UiContext) {
    let paused = context
        .control
        .paused
        .load(std::sync::atomic::Ordering::Acquire);
    context
        .control
        .paused
        .store(!paused, std::sync::atomic::Ordering::Release);
    refresh_pause_button(context);
}

unsafe fn refresh_pause_button(context: &UiContext) {
    let button = if context.settings_window.is_null() {
        context.pause
    } else {
        GetDlgItem(context.settings_window, ID_PAUSE as i32)
    };
    if button.is_null() {
        return;
    }
    let paused = context
        .control
        .paused
        .load(std::sync::atomic::Ordering::Acquire);
    let label = if paused {
        "Resume Wiggler"
    } else {
        "Pause Wiggler"
    };
    SetWindowTextW(button, wide(label).as_ptr());
}

unsafe fn show_tray_menu(context: &UiContext) {
    let menu = CreatePopupMenu();
    AppendMenuW(
        menu,
        MF_STRING,
        MENU_SETTINGS as usize,
        wide("Settings").as_ptr(),
    );
    let pause_label = if context
        .control
        .paused
        .load(std::sync::atomic::Ordering::Acquire)
    {
        "Resume"
    } else {
        "Pause"
    };
    AppendMenuW(
        menu,
        MF_STRING,
        MENU_PAUSE as usize,
        wide(pause_label).as_ptr(),
    );
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(
        menu,
        MF_STRING,
        MENU_REPAIR_STARTUP as usize,
        wide("Repair Startup").as_ptr(),
    );
    AppendMenuW(menu, MF_STRING, MENU_EXIT as usize, wide("Exit").as_ptr());
    let mut point = POINT { x: 0, y: 0 };
    GetCursorPos(&mut point);
    SetForegroundWindow(context.tray_window);
    TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON,
        point.x,
        point.y,
        0,
        context.tray_window,
        null(),
    );
    DestroyMenu(menu);
}

unsafe fn show_settings(context: &mut UiContext) {
    if !context.settings_window.is_null() {
        ShowWindow(context.settings_window, SW_SHOW);
        SetForegroundWindow(context.settings_window);
        return;
    }
    let instance = GetModuleHandleW(null());
    let class = wide("WigglerSettingsWindow");

    let mut dpi = GetDpiForWindow(context.tray_window) as i32;
    if dpi == 0 {
        dpi = 96;
    }
    let s = |val: i32| (val * dpi) / 96;

    let window = CreateWindowExW(
        0,
        class.as_ptr(),
        wide("Wiggler").as_ptr(),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        s(640),
        s(370),
        context.tray_window,
        null_mut(),
        instance,
        context as *mut UiContext as *const c_void,
    );
    if window.is_null() {
        show_error(context.tray_window, &last_error("CreateWindowExW"));
        return;
    }
    context.settings_window = window;
    ShowWindow(window, SW_SHOW);
    UpdateWindow(window);
}

unsafe fn update_settings(context: &mut UiContext) {
    if context.settings_window.is_null() {
        return;
    }
    let current = *context
        .control
        .settings
        .lock()
        .expect("settings lock is valid");
    let profile = match SendMessageW(context.profile, CB_GETCURSEL, 0, 0) as i32 {
        0 => Profile::Linear,
        1 => Profile::Diagonal,
        2 => Profile::Lissajous,
        3 => Profile::Brownian,
        _ => current.profile,
    };
    let mut settings = Settings {
        profile,
        delay: std::time::Duration::from_secs(
            read_u64(context.delay).unwrap_or(current.delay.as_secs()),
        ),
        amplitude: read_f64(context.amplitude).unwrap_or(current.amplitude),
        speed: read_f64(context.speed).unwrap_or(current.speed),
        start_with_windows: SendMessageW(context.startup, BM_GETCHECK, 0, 0)
            == BST_CHECKED as isize,
    }
    .validated();

    let startup_changed = settings.start_with_windows != current.start_with_windows;
    if startup_changed {
        if let Err(error) = startup::set_enabled(settings.start_with_windows) {
            settings.start_with_windows = current.start_with_windows;
            SendMessageW(
                context.startup,
                BM_SETCHECK,
                if current.start_with_windows {
                    BST_CHECKED as usize
                } else {
                    BST_UNCHECKED as usize
                },
                0,
            );
            show_error(
                context.settings_window,
                &format!("Could not change Start with Windows: {error}"),
            );
        }
    }

    if settings == current {
        return;
    }
    if let Err(error) = context.store.save(settings) {
        if startup_changed {
            let _ = startup::set_enabled(current.start_with_windows);
            SendMessageW(
                context.startup,
                BM_SETCHECK,
                if current.start_with_windows {
                    BST_CHECKED as usize
                } else {
                    BST_UNCHECKED as usize
                },
                0,
            );
        }
        if !context.save_error_shown {
            context.save_error_shown = true;
            show_error(
                context.settings_window,
                &format!("Settings could not be saved: {error}"),
            );
        }
        return;
    }
    context.save_error_shown = false;
    if let Ok(mut value) = context.control.settings.lock() {
        *value = settings;
    }
}

unsafe fn show_error(window: HWND, message: &str) {
    MessageBoxW(
        window,
        wide(message).as_ptr(),
        wide("Wiggler").as_ptr(),
        MB_ICONERROR | MB_OK,
    );
}

pub fn show_fatal_error(message: &str) {
    unsafe { show_error(null_mut(), message) }
}

unsafe extern "system" fn settings_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let mut context = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut UiContext;
    if message == WM_NCCREATE {
        let create = &*(lparam as *const CREATESTRUCTW);
        context = create.lpCreateParams as *mut UiContext;
        SetWindowLongPtrW(window, GWLP_USERDATA, context as isize);
        let context = &mut *context;
        create_settings_controls(window, context);
        return 1;
    }
    if context.is_null() {
        return DefWindowProcW(window, message, wparam, lparam);
    }
    let context = &mut *context;
    match message {
        WM_DPICHANGED => {
            let suggested = &*(lparam as *const RECT);
            SetWindowPos(
                window,
                null_mut(),
                suggested.left,
                suggested.top,
                suggested.right - suggested.left,
                suggested.bottom - suggested.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            set_settings_font(window, context);
            layout_settings_controls(window, context);
            0
        }
        WM_SIZE => {
            layout_settings_controls(window, context);
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            SetBkMode(wparam as HDC, TRANSPARENT as i32);
            GetSysColorBrush(5) as LRESULT
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as u16;
            let notification = ((wparam >> 16) & 0xFFFF) as u32;

            if id == ID_PAUSE {
                toggle_pause(context);
                return 0;
            }

            if notification == EN_CHANGE
                || notification == CBN_SELCHANGE
                || notification == BN_CLICKED
            {
                update_settings(context);
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(window);
            0
        }
        WM_DESTROY => {
            context.settings_window = null_mut();
            0
        }
        WM_NCDESTROY => {
            if !context.font.is_null() {
                DeleteObject(context.font);
                context.font = null_mut();
            }
            DefWindowProcW(window, message, wparam, lparam)
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn create_settings_controls(window: HWND, context: &mut UiContext) {
    let instance = GetModuleHandleW(null());
    for (index, label) in [
        "Movement profile:",
        "Start moving after:",
        "Amplitude:",
        "Speed:",
    ]
    .into_iter()
    .enumerate()
    {
        context.labels[index] = CreateWindowExW(
            0,
            wide("STATIC").as_ptr(),
            wide(label).as_ptr(),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            0,
            0,
            window,
            null_mut(),
            instance,
            null(),
        );
    }

    context.profile = CreateWindowExW(
        0,
        wide("COMBOBOX").as_ptr(),
        null(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST as u32,
        0,
        0,
        0,
        0,
        window,
        ID_PROFILE as _,
        instance,
        null(),
    );

    for profile in ["Linear", "Diagonal", "Lissajous", "Brownian"] {
        SendMessageW(
            context.profile,
            CB_ADDSTRING,
            0,
            wide(profile).as_ptr() as isize,
        );
    }

    let settings = *context.control.settings.lock().unwrap();
    SendMessageW(
        context.profile,
        CB_SETCURSEL,
        profile_index(settings.profile),
        0,
    );

    context.delay = numeric_edit(
        window,
        ID_DELAY as i32,
        settings.delay.as_secs().to_string(),
        instance,
    );
    context.seconds = CreateWindowExW(
        0,
        wide("STATIC").as_ptr(),
        wide("seconds").as_ptr(),
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        0,
        0,
        window,
        null_mut(),
        instance,
        null(),
    );

    context.amplitude = numeric_edit(
        window,
        ID_AMPLITUDE as i32,
        settings.amplitude.to_string(),
        instance,
    );
    context.pixels = CreateWindowExW(
        0,
        wide("STATIC").as_ptr(),
        wide("pixels").as_ptr(),
        WS_CHILD | WS_VISIBLE,
        0,
        0,
        0,
        0,
        window,
        null_mut(),
        instance,
        null(),
    );

    context.speed = numeric_edit(
        window,
        ID_SPEED as i32,
        settings.speed.to_string(),
        instance,
    );

    context.startup = CreateWindowExW(
        0,
        wide("BUTTON").as_ptr(),
        wide("Start with Windows").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        0,
        0,
        0,
        0,
        window,
        ID_STARTUP as _,
        instance,
        null(),
    );

    SendMessageW(
        context.startup,
        BM_SETCHECK,
        if settings.start_with_windows {
            BST_CHECKED as usize
        } else {
            BST_UNCHECKED as usize
        },
        0,
    );

    context.pause = CreateWindowExW(
        0,
        wide("BUTTON").as_ptr(),
        wide("Pause Wiggler").as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP,
        0,
        0,
        0,
        0,
        window,
        ID_PAUSE as _,
        instance,
        null(),
    );

    set_settings_font(window, context);
    layout_settings_controls(window, context);
    refresh_pause_button(context);
}

unsafe fn set_settings_font(window: HWND, context: &mut UiContext) {
    let dpi = GetDpiForWindow(window).max(96) as i32;
    let font = CreateFontW(
        -(20 * dpi / 96),
        0,
        0,
        0,
        FW_NORMAL as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        0,
        0,
        0,
        0,
        wide("Segoe UI").as_ptr(),
    );
    if font.is_null() {
        return;
    }
    EnumChildWindows(window, Some(apply_settings_font), font as LPARAM);
    if !context.font.is_null() {
        DeleteObject(context.font);
    }
    context.font = font;
}

unsafe extern "system" fn apply_settings_font(hwnd: HWND, font: LPARAM) -> i32 {
    SendMessageW(hwnd, WM_SETFONT, font as WPARAM, 1);
    1
}

unsafe fn layout_settings_controls(window: HWND, context: &UiContext) {
    let mut client: RECT = std::mem::zeroed();
    GetClientRect(window, &mut client);
    let width = client.right.max(1);
    let height = client.bottom.max(1);
    let dpi = GetDpiForWindow(window).max(96) as i32;
    let scale = |value: i32| value * dpi / 96;
    let margin = scale(24);
    let content_left = margin;
    let content_right = (width - margin).max(content_left + scale(240));
    let content_width = content_right - content_left;
    let action_top = margin;
    let fields_top = action_top + scale(54);
    let footer_height = scale(42);
    let row_gap = ((height - fields_top - footer_height) / 4).max(scale(40));
    let label_width = scale(190);
    let unit_width = scale(70);
    let input_left = content_left + label_width + scale(14);
    let input_width =
        (content_width - label_width - scale(14) - unit_width - scale(8)).max(scale(120));
    let input_height = scale(34);

    MoveWindow(
        context.pause,
        content_left,
        action_top,
        scale(170),
        scale(42),
        1,
    );
    MoveWindow(
        context.startup,
        content_left + scale(190),
        action_top,
        scale(260),
        scale(42),
        1,
    );

    for (index, label) in context.labels.iter().enumerate() {
        let y = fields_top + row_gap * index as i32;
        MoveWindow(*label, content_left, y, label_width, input_height, 1);
    }
    let controls = [
        context.profile,
        context.delay,
        context.amplitude,
        context.speed,
    ];
    for (index, control) in controls.iter().enumerate() {
        let y = fields_top + row_gap * index as i32;
        MoveWindow(*control, input_left, y, input_width, input_height, 1);
    }
    let unit_y = |index: i32| fields_top + row_gap * index;
    MoveWindow(
        context.seconds,
        input_left + input_width + scale(8),
        unit_y(1),
        unit_width,
        input_height,
        1,
    );
    MoveWindow(
        context.pixels,
        input_left + input_width + scale(8),
        unit_y(2),
        unit_width,
        input_height,
        1,
    );
}

unsafe fn numeric_edit(window: HWND, id: i32, value: String, instance: HINSTANCE) -> HWND {
    let edit = CreateWindowExW(
        WS_EX_CLIENTEDGE,
        wide("EDIT").as_ptr(),
        wide(&value).as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
        0,
        0,
        0,
        0,
        window,
        id as _,
        instance,
        null(),
    );
    SendMessageW(edit, EM_SETLIMITTEXT, 12, 0);
    edit
}

fn read_text(window: HWND) -> Option<String> {
    let mut buffer = [0u16; 64];
    let length =
        unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) } as usize;
    String::from_utf16(&buffer[..length]).ok()
}

unsafe fn read_u64(window: HWND) -> Option<u64> {
    read_text(window)?.parse().ok()
}
unsafe fn read_f64(window: HWND) -> Option<f64> {
    read_text(window)?
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

fn profile_index(profile: Profile) -> usize {
    match profile {
        Profile::Linear => 0,
        Profile::Diagonal => 1,
        Profile::Lissajous => 2,
        Profile::Brownian => 3,
    }
}

fn notify_data(window: HWND) -> NOTIFYICONDATAW {
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = window;
    data.uID = TRAY_ID;
    data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    data.uCallbackMessage = WM_TRAY;
    data.hIcon = unsafe {
        LoadIconW(
            GetModuleHandleW(null()),
            std::ptr::without_provenance::<u16>(1),
        )
    };
    let tip = wide("Wiggler");
    let count = tip.len().min(data.szTip.len() - 1);
    data.szTip[..count].copy_from_slice(&tip[..count]);
    data
}

fn add_tray_icon(window: HWND) -> Result<(), String> {
    let data = notify_data(window);
    if unsafe { Shell_NotifyIconW(NIM_ADD, &data) } == 0 {
        return Err(last_error("Shell_NotifyIconW"));
    }
    Ok(())
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error(operation: &str) -> String {
    format!("{operation} failed with Win32 error {}", unsafe {
        GetLastError()
    })
}
