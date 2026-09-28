use std::f64::consts::SQRT_2;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, GetCurrentThreadId, SetEvent, WaitForSingleObject,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK,
    MOUSEINPUT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, PeekMessageW,
    PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, HHOOK,
    LLMHF_INJECTED, MSLLHOOKSTRUCT, PM_NOREMOVE, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WH_MOUSE_LL, WM_APP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use wiggler::core::{Point, Profile, Runtime, RuntimeMode, Settings};

const FRAME: Duration = Duration::from_millis(16);
const WM_HOOK_CONTROL: u32 = WM_APP + 2;

struct HookSignal {
    event: isize,
    activity: Arc<AtomicU64>,
    callbacks: Arc<AtomicU64>,
}

static HOOK_SIGNAL: OnceLock<HookSignal> = OnceLock::new();

pub struct NativeRuntime {
    activity_event: HANDLE,
    activity_generation: Arc<AtomicU64>,
    callback_generation: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    hook_thread: Option<thread::JoinHandle<()>>,
    hook_thread_id: u32,
    hook_ready: Arc<AtomicBool>,
    hook_reset: Arc<AtomicBool>,
}

pub struct ControlState {
    pub settings: Mutex<Settings>,
    pub paused: AtomicBool,
    pub exit: AtomicBool,
    pub suspended: AtomicU32,
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
            suspended: AtomicU32::new(0),
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

        let activity_generation = Arc::new(AtomicU64::new(0));
        let callback_generation = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let hook_reset = Arc::new(AtomicBool::new(false));
        let hook_ready = Arc::new(AtomicBool::new(false));
        if HOOK_SIGNAL
            .set(HookSignal {
                event: activity_event as isize,
                activity: Arc::clone(&activity_generation),
                callbacks: Arc::clone(&callback_generation),
            })
            .is_err()
        {
            unsafe { CloseHandle(activity_event) };
            return Err("native runtime can only be started once".to_string());
        }

        let thread_stop = Arc::clone(&stop);
        let thread_hook_reset = Arc::clone(&hook_reset);
        let thread_hook_ready = Arc::clone(&hook_ready);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let hook_thread = thread::spawn(move || {
            if let Err(error) =
                run_mouse_hook(thread_stop, thread_hook_reset, thread_hook_ready, ready_tx)
            {
                eprintln!("mouse hook stopped: {error}");
            }
        });

        let hook_thread_id = match ready_rx.recv() {
            Ok(Ok(thread_id)) => thread_id,
            Ok(Err(error)) => {
                let _ = hook_thread.join();
                unsafe { CloseHandle(activity_event) };
                return Err(error);
            }
            Err(_) => {
                let _ = hook_thread.join();
                unsafe { CloseHandle(activity_event) };
                return Err("mouse hook thread exited before it was ready".to_string());
            }
        };

        Ok(Self {
            activity_event,
            activity_generation,
            callback_generation,
            stop,
            hook_thread: Some(hook_thread),
            hook_thread_id,
            hook_ready,
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
        let mut activity_generation = self.activity_generation.load(Ordering::Acquire);
        let mut monitoring = self.hook_ready.load(Ordering::Acquire);
        let mut injection_ack: Option<(u64, Instant)> = None;

        loop {
            if self.stop.load(Ordering::Acquire) || control.exit.load(Ordering::Acquire) {
                return Ok(());
            }

            let current_generation = control.lifecycle_generation.load(Ordering::Acquire);
            if current_generation != lifecycle_generation {
                lifecycle_generation = current_generation;
                runtime.on_mouse_activity(Instant::now());
                self.hook_reset.store(true, Ordering::Release);
                unsafe { PostThreadMessageW(self.hook_thread_id, WM_HOOK_CONTROL, 0, 0) };
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
            let ready = self.hook_ready.load(Ordering::Acquire);
            if !ready {
                if monitoring {
                    runtime.on_mouse_activity(now);
                }
                monitoring = false;
                injection_ack = None;
                continue;
            }
            if !monitoring {
                runtime.on_mouse_activity(now);
                monitoring = true;
            }
            let current_activity = self.activity_generation.load(Ordering::Acquire);
            if current_activity != activity_generation {
                activity_generation = current_activity;
                injection_ack = None;
                runtime.on_mouse_activity(now);
                continue;
            }
            if let Some((before, sent_at)) = injection_ack {
                if self.callback_generation.load(Ordering::Acquire) != before {
                    injection_ack = None;
                } else if now.duration_since(sent_at) >= Duration::from_millis(100) {
                    self.hook_ready.store(false, Ordering::Release);
                    self.hook_reset.store(true, Ordering::Release);
                    unsafe { PostThreadMessageW(self.hook_thread_id, WM_HOOK_CONTROL, 0, 0) };
                    runtime.on_mouse_activity(now);
                    monitoring = false;
                    injection_ack = None;
                    continue;
                } else {
                    continue;
                }
            }

            if !control.paused.load(Ordering::Acquire)
                && control.suspended.load(Ordering::Acquire) == 0
            {
                let cursor = match cursor_position() {
                    Ok(cursor) => cursor,
                    Err(_) => {
                        runtime.on_mouse_activity(now);
                        continue;
                    }
                };
                if let Some(next) = runtime.tick(now, cursor) {
                    let current_activity = self.activity_generation.load(Ordering::Acquire);
                    if current_activity != activity_generation
                        || !self.hook_ready.load(Ordering::Acquire)
                    {
                        activity_generation = current_activity;
                        injection_ack = None;
                        runtime.on_mouse_activity(Instant::now());
                        continue;
                    }
                    let callbacks_before = self.callback_generation.load(Ordering::Acquire);
                    let next = match constrain_to_monitor(
                        next,
                        runtime
                            .resting_point()
                            .expect("wiggling runtime has a resting point"),
                        runtime.settings(),
                    ) {
                        Ok(next) => next,
                        Err(_) => {
                            runtime.on_mouse_activity(now);
                            continue;
                        }
                    };
                    if inject_absolute(next).is_err() {
                        runtime.on_mouse_activity(now);
                        continue;
                    }
                    injection_ack = Some((callbacks_before, Instant::now()));
                }
            }
        }
    }
}

impl Drop for NativeRuntime {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        unsafe { PostThreadMessageW(self.hook_thread_id, WM_HOOK_CONTROL, 0, 0) };
        if let Some(thread) = self.hook_thread.take() {
            let _ = thread.join();
        }
        unsafe {
            SetEvent(self.activity_event);
            CloseHandle(self.activity_event);
        }
    }
}

fn run_mouse_hook(
    stop: Arc<AtomicBool>,
    reset: Arc<AtomicBool>,
    ready: Arc<AtomicBool>,
    initial: SyncSender<Result<u32, String>>,
) -> Result<(), String> {
    let mut initial = Some(initial);
    while !stop.load(Ordering::Acquire) {
        if let Err(error) = pump_mouse_hook(&stop, &reset, &ready, &mut initial) {
            if let Some(sender) = initial.take() {
                let _ = sender.send(Err(error.clone()));
                return Err(error);
            }
            eprintln!("mouse hook recovery scheduled: {error}");
            thread::sleep(Duration::from_secs(1));
        }
    }
    Ok(())
}

fn pump_mouse_hook(
    stop: &AtomicBool,
    reset: &AtomicBool,
    ready: &AtomicBool,
    initial: &mut Option<SyncSender<Result<u32, String>>>,
) -> Result<(), String> {
    let hook: HHOOK = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), null_mut(), 0) };
    if hook.is_null() {
        return Err(last_error("SetWindowsHookExW"));
    }

    let mut message = unsafe { std::mem::zeroed() };
    unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_NOREMOVE) };
    ready.store(true, Ordering::Release);
    if let Some(sender) = initial.take() {
        let _ = sender.send(Ok(unsafe { GetCurrentThreadId() }));
    }
    while !stop.load(Ordering::Acquire) && !reset.swap(false, Ordering::AcqRel) {
        let result = unsafe { GetMessageW(&mut message, null_mut(), 0, 0) };
        if result == -1 {
            ready.store(false, Ordering::Release);
            unsafe { UnhookWindowsHookEx(hook) };
            return Err(last_error("GetMessageW"));
        }
        if result == 0 {
            break;
        }
        if message.message != WM_HOOK_CONTROL {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    ready.store(false, Ordering::Release);
    unsafe { UnhookWindowsHookEx(hook) };
    Ok(())
}

unsafe extern "system" fn mouse_hook(code: i32, message: WPARAM, data: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        if let Some(signal) = HOOK_SIGNAL.get() {
            signal.callbacks.fetch_add(1, Ordering::AcqRel);
            if is_user_mouse_event(message, data) {
                signal.activity.fetch_add(1, Ordering::AcqRel);
                SetEvent(signal.event as HANDLE);
            }
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

fn constrain_to_monitor(
    point: Point,
    reference: Point,
    settings: Settings,
) -> Result<Point, String> {
    let monitor = unsafe {
        MonitorFromPoint(
            POINT {
                x: reference.x.round() as i32,
                y: reference.y.round() as i32,
            },
            MONITOR_DEFAULTTONEAREST,
        )
    };
    if monitor.is_null() {
        return Err(last_error("MonitorFromPoint"));
    }
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return Err(last_error("GetMonitorInfoW"));
    }
    Ok(constrain_to_rect(
        point,
        reference,
        settings.profile,
        settings.amplitude,
        info.rcMonitor,
    ))
}

fn constrain_to_rect(
    point: Point,
    reference: Point,
    profile: Profile,
    amplitude: f64,
    bounds: RECT,
) -> Point {
    let axis = |value: f64, rest: f64, min: f64, max: f64| {
        if profile == Profile::Diagonal {
            let peak = amplitude / SQRT_2;
            if rest - min < peak && max - rest > rest - min {
                (rest - (value - rest)).clamp(min, max)
            } else {
                value.clamp(min, max)
            }
        } else {
            let span = max - min;
            if span <= 0.0 {
                min
            } else {
                let position = (value - min).rem_euclid(2.0 * span);
                min + if position <= span {
                    position
                } else {
                    2.0 * span - position
                }
            }
        }
    };
    Point {
        x: axis(
            point.x,
            reference.x,
            bounds.left as f64,
            (bounds.right - 1) as f64,
        ),
        y: axis(
            point.y,
            reference.y,
            bounds.top as f64,
            (bounds.bottom - 1) as f64,
        ),
    }
}

fn inject_absolute(point: Point) -> Result<(), String> {
    let left = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) } as f64;
    let top = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) } as f64;
    let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(1) as f64;
    let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(1) as f64;
    let dx = ((point.x - left) * 65_535.0 / (width - 1.0).max(1.0))
        .round()
        .clamp(0.0, 65_535.0) as i32;
    let dy = ((point.y - top) * 65_535.0 / (height - 1.0).max(1.0))
        .round()
        .clamp(0.0, 65_535.0) as i32;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn upper_monitor() -> RECT {
        RECT {
            left: 0,
            top: -1080,
            right: 1920,
            bottom: 0,
        }
    }

    #[test]
    fn diagonal_at_top_left_edge_shuttles_inward_without_crossing_rest() {
        let reference = Point { x: 1.0, y: -1079.0 };
        let mut previous = reference;
        for step in 0..=100 {
            let displacement = 5.0 / SQRT_2 * step as f64 / 100.0;
            let raw = Point {
                x: reference.x - displacement,
                y: reference.y - displacement,
            };
            let constrained =
                constrain_to_rect(raw, reference, Profile::Diagonal, 5.0, upper_monitor());
            assert!(constrained.x >= reference.x && constrained.y >= reference.y);
            assert!(constrained.x >= previous.x && constrained.y >= previous.y);
            assert!(constrained.x < 1920.0 && constrained.y < 0.0);
            let radius = ((constrained.x - reference.x).powi(2)
                + (constrained.y - reference.y).powi(2))
            .sqrt();
            assert!(radius <= 5.0 + 1e-10);
            previous = constrained;
        }
        assert!(previous.x > reference.x && previous.y > reference.y);
    }

    #[test]
    fn other_profiles_reflect_into_the_monitor_at_an_edge() {
        let reference = Point { x: 0.0, y: -1080.0 };
        let raw = Point {
            x: -3.0,
            y: -1083.0,
        };
        for profile in [Profile::Linear, Profile::Lissajous, Profile::Brownian] {
            let constrained = constrain_to_rect(raw, reference, profile, 5.0, upper_monitor());
            assert_eq!(constrained, Point { x: 3.0, y: -1077.0 });
        }
    }
}
