use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, LPARAM, LRESULT, WPARAM,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK,
    MOUSEINPUT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetCursorPos, GetSystemMetrics, PeekMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, HHOOK, LLMHF_INJECTED,
    MSLLHOOKSTRUCT, PM_REMOVE, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use wiggler::core::{Point, Runtime, RuntimeMode, Settings};

const FRAME: Duration = Duration::from_millis(16);

struct HookSignal {
    event: isize,
    activity: Arc<AtomicBool>,
}

static HOOK_SIGNAL: OnceLock<HookSignal> = OnceLock::new();

pub struct NativeRuntime {
    activity_event: HANDLE,
    activity_generation: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    hook_thread: Option<thread::JoinHandle<()>>,
    hook_reset: Arc<AtomicBool>,
}

pub struct ControlState {
    pub settings: Mutex<Settings>,
    pub paused: AtomicBool,
    pub exit: AtomicBool,
    pub suspended: AtomicBool,
    pub lifecycle_generation: AtomicU64,
}

pub struct SingleInstance {
    handle: HANDLE,
}

impl SingleInstance {
    pub fn acquire() -> Result<Self, String> {
        let name: Vec<u16> = "Local\\Wiggler-Fable-v1"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe { CreateMutexW(null_mut(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(last_error("CreateMutexW"));
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return Err("another Wiggler instance is already running".to_string());
        }
        Ok(Self { handle })
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

impl ControlState {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings: Mutex::new(settings),
            paused: AtomicBool::new(false),
            exit: AtomicBool::new(false),
            suspended: AtomicBool::new(false),
            lifecycle_generation: AtomicU64::new(0),
        }
    }
}

impl NativeRuntime {
    pub fn start() -> Result<Self, String> {
        let activity_event = unsafe { CreateEventW(null_mut(), 0, 0, null_mut()) };
        if activity_event.is_null() {
            return Err(last_error("CreateEventW"));
        }

        let activity_generation = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let hook_reset = Arc::new(AtomicBool::new(false));
        if HOOK_SIGNAL
            .set(HookSignal {
                event: activity_event as isize,
                activity: Arc::clone(&activity_generation),
            })
            .is_err()
        {
            unsafe { CloseHandle(activity_event) };
            return Err("native runtime can only be started once".to_string());
        }

        let thread_stop = Arc::clone(&stop);
        let thread_hook_reset = Arc::clone(&hook_reset);
        let hook_thread = thread::spawn(move || {
            if let Err(error) = run_mouse_hook(thread_stop, thread_hook_reset) {
                eprintln!("mouse hook stopped: {error}");
            }
        });

        Ok(Self {
            activity_event,
            activity_generation,
            stop,
            hook_thread: Some(hook_thread),
            hook_reset,
        })
    }

    pub fn run(&self, control: Arc<ControlState>) -> Result<(), String> {
        let start = Instant::now();
        let mut settings = *control
            .settings
            .lock()
            .map_err(|_| "settings lock poisoned")?;
        let mut runtime = Runtime::new(settings, start);
        let mut lifecycle_generation = control.lifecycle_generation.load(Ordering::Acquire);

        loop {
            if self.stop.load(Ordering::Acquire) || control.exit.load(Ordering::Acquire) {
                return Ok(());
            }

            let current_generation = control.lifecycle_generation.load(Ordering::Acquire);
            if current_generation != lifecycle_generation {
                lifecycle_generation = current_generation;
                runtime.on_mouse_activity(Instant::now());
                self.hook_reset.store(true, Ordering::Release);
            }

            let current_settings = *control
                .settings
                .lock()
                .map_err(|_| "settings lock poisoned")?;
            if current_settings != settings {
                settings = current_settings;
                runtime.update_settings(settings, Instant::now());
            }
            if control.paused.load(Ordering::Acquire) {
                runtime.pause();
            } else if runtime.mode() == RuntimeMode::Paused {
                runtime.resume(Instant::now());
            }

            let result =
                unsafe { WaitForSingleObject(self.activity_event, FRAME.as_millis() as u32) };
            if result == windows_sys::Win32::Foundation::WAIT_FAILED {
                return Err(last_error("WaitForSingleObject"));
            }

            let now = Instant::now();
            if self.activity_generation.swap(false, Ordering::AcqRel) {
                runtime.on_mouse_activity(now);
                continue;
            }

            let cursor = cursor_position()?;
            if !control.paused.load(Ordering::Acquire) && !control.suspended.load(Ordering::Acquire)
            {
                if let Some(next) = runtime.tick(now, cursor) {
                    inject_absolute(next)?;
                }
            }
        }
    }
}

impl Drop for NativeRuntime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        unsafe {
            SetEvent(self.activity_event);
            CloseHandle(self.activity_event);
        }
        if let Some(thread) = self.hook_thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_mouse_hook(stop: Arc<AtomicBool>, reset: Arc<AtomicBool>) -> Result<(), String> {
    while !stop.load(Ordering::Acquire) {
        if let Err(error) = pump_mouse_hook(&stop, &reset) {
            eprintln!("mouse hook recovery scheduled: {error}");
            thread::sleep(Duration::from_secs(1));
        }
    }
    Ok(())
}

fn pump_mouse_hook(stop: &AtomicBool, reset: &AtomicBool) -> Result<(), String> {
    let hook: HHOOK = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), null_mut(), 0) };
    if hook.is_null() {
        return Err(last_error("SetWindowsHookExW"));
    }

    let mut message = unsafe { std::mem::zeroed() };
    while !stop.load(Ordering::Acquire) && !reset.swap(false, Ordering::AcqRel) {
        while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        thread::sleep(Duration::from_millis(1));
    }

    unsafe { UnhookWindowsHookEx(hook) };
    Ok(())
}

unsafe extern "system" fn mouse_hook(code: i32, message: WPARAM, data: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && is_user_mouse_event(message, data) {
        if let Some(signal) = HOOK_SIGNAL.get() {
            signal.activity.store(true, Ordering::Release);
            SetEvent(signal.event as HANDLE);
        }
    }
    CallNextHookEx(null_mut(), code, message, data)
}

fn is_user_mouse_event(message: WPARAM, data: LPARAM) -> bool {
    let relevant = matches!(
        message as u32,
        WM_MOUSEMOVE
            | WM_MOUSEWHEEL
            | WM_LBUTTONDOWN
            | WM_LBUTTONUP
            | WM_RBUTTONDOWN
            | WM_RBUTTONUP
            | WM_MBUTTONDOWN
            | WM_MBUTTONUP
            | WM_XBUTTONDOWN
            | WM_XBUTTONUP
    );
    if !relevant || data == 0 {
        return false;
    }

    let hook_data = unsafe { &*(data as *const MSLLHOOKSTRUCT) };
    hook_data.flags & LLMHF_INJECTED == 0
}

fn cursor_position() -> Result<Point, String> {
    let mut point = unsafe { std::mem::zeroed() };
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return Err(last_error("GetCursorPos"));
    }
    Ok(Point {
        x: point.x as f64,
        y: point.y as f64,
    })
}

fn inject_absolute(point: Point) -> Result<(), String> {
    let left = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) } as f64;
    let top = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) } as f64;
    let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(1) as f64;
    let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(1) as f64;
    let dx = ((point.x - left) * 65_535.0 / (width - 1.0).max(1.0)).round() as i32;
    let dy = ((point.y - top) * 65_535.0 / (height - 1.0).max(1.0)).round() as i32;

    let input = INPUT {
        r#type: 0,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let sent = unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    if sent != 1 {
        return Err(last_error("SendInput"));
    }
    Ok(())
}

fn last_error(operation: &str) -> String {
    format!("{operation} failed with Win32 error {}", unsafe {
        windows_sys::Win32::Foundation::GetLastError()
    })
}
