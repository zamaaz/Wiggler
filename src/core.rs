use std::f64::consts::TAU;
use std::time::{Duration, Instant};

const MIN_DELAY_SECONDS: u64 = 1;
const MAX_DELAY_SECONDS: u64 = 86_400;
const MIN_AMPLITUDE_PIXELS: f64 = 0.0;
const MAX_AMPLITUDE_PIXELS: f64 = 500.0;
const MIN_SPEED: f64 = 0.1;
const MAX_SPEED: f64 = 20.0;
const DEFAULT_DELAY_SECONDS: u64 = 5;
const DEFAULT_AMPLITUDE_PIXELS: f64 = 5.0;
const DEFAULT_SPEED: f64 = 3.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile {
    Linear,
    Diagonal,
    Lissajous,
    Brownian,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub profile: Profile,
    pub delay: Duration,
    pub amplitude: f64,
    pub speed: f64,
    pub start_with_windows: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            profile: Profile::Diagonal,
            delay: Duration::from_secs(DEFAULT_DELAY_SECONDS),
            amplitude: DEFAULT_AMPLITUDE_PIXELS,
            speed: DEFAULT_SPEED,
            start_with_windows: false,
        }
    }
}

impl Settings {
    pub fn validated(self) -> Self {
        Self {
            profile: self.profile,
            delay: Duration::from_secs(
                self.delay
                    .as_secs()
                    .clamp(MIN_DELAY_SECONDS, MAX_DELAY_SECONDS),
            ),
            amplitude: self
                .amplitude
                .clamp(MIN_AMPLITUDE_PIXELS, MAX_AMPLITUDE_PIXELS),
            speed: self.speed.clamp(MIN_SPEED, MAX_SPEED),
            start_with_windows: self.start_with_windows,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    fn offset(self, x: f64, y: f64) -> Self {
        Self {
            x: self.x + x,
            y: self.y + y,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeMode {
    UserControl,
    Wiggling,
    Paused,
}

#[derive(Clone, Copy, Debug)]
pub struct MotionPath {
    profile: Profile,
    reference: Point,
    amplitude: f64,
    speed: f64,
    seed: u64,
}

impl MotionPath {
    pub fn new(settings: Settings, reference: Point, seed: u64) -> Self {
        let settings = settings.validated();
        Self {
            profile: settings.profile,
            reference,
            amplitude: settings.amplitude,
            speed: settings.speed,
            seed,
        }
    }

    pub fn sample(&self, elapsed: Duration) -> Point {
        let seconds = elapsed.as_secs_f64();
        let phase = TAU * self.speed * seconds / 60.0;
        let (x, y) = match self.profile {
            Profile::Linear => (self.amplitude * phase.sin(), 0.0),
            Profile::Diagonal => {
                let shuttle = (1.0 - phase.cos()) / 2.0;
                (-self.amplitude * shuttle, -self.amplitude * shuttle)
            }
            Profile::Lissajous => (
                self.amplitude * phase.sin(),
                self.amplitude * (1.5 * phase).sin(),
            ),
            Profile::Brownian => {
                let x = self.noise(seconds * self.speed / 8.0, self.seed);
                let y = self.noise(seconds * self.speed / 8.0, self.seed.rotate_left(32));
                let radius = (x * x + y * y).sqrt();
                let scale = if radius > 1.0 { 1.0 / radius } else { 1.0 };
                (self.amplitude * x * scale, self.amplitude * y * scale)
            }
        };
        self.reference.offset(x, y)
    }

    fn noise(&self, position: f64, seed: u64) -> f64 {
        let left = position.floor() as i64;
        let fraction = position - left as f64;
        let a = seeded_value(left, seed);
        let b = seeded_value(left + 1, seed);
        let smooth = fraction * fraction * (3.0 - 2.0 * fraction);
        a + (b - a) * smooth
    }
}

fn seeded_value(index: i64, seed: u64) -> f64 {
    let mut value = seed
        .wrapping_add(index as u64)
        .wrapping_mul(0x9E3779B97F4A7C15);
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58476D1CE4E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D049BB133111EB);
    value ^= value >> 31;
    (value as f64 / u64::MAX as f64) * 2.0 - 1.0
}

pub struct Runtime {
    settings: Settings,
    mode: RuntimeMode,
    idle_deadline: Option<Instant>,
    path: Option<MotionPath>,
    next_seed: u64,
}

impl Runtime {
    pub fn new(settings: Settings, now: Instant) -> Self {
        let settings = settings.validated();
        Self {
            settings,
            mode: RuntimeMode::UserControl,
            idle_deadline: Some(now + settings.delay),
            path: None,
            next_seed: 1,
        }
    }

    pub fn mode(&self) -> RuntimeMode {
        self.mode
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    pub fn update_settings(&mut self, settings: Settings, now: Instant) {
        self.settings = settings.validated();
        self.mode = RuntimeMode::UserControl;
        self.path = None;
        self.idle_deadline = Some(now + self.settings.delay);
    }

    pub fn on_mouse_activity(&mut self, now: Instant) {
        if self.mode != RuntimeMode::Paused {
            self.mode = RuntimeMode::UserControl;
            self.path = None;
            self.idle_deadline = Some(now + self.settings.delay);
        }
    }

    pub fn pause(&mut self) {
        self.mode = RuntimeMode::Paused;
        self.path = None;
        self.idle_deadline = None;
    }

    pub fn resume(&mut self, now: Instant) {
        self.mode = RuntimeMode::UserControl;
        self.path = None;
        self.idle_deadline = Some(now + self.settings.delay);
    }

    pub fn tick(&mut self, now: Instant, cursor: Point) -> Option<Point> {
        if self.mode == RuntimeMode::Paused {
            return None;
        }
        if self.mode == RuntimeMode::UserControl {
            if now < self.idle_deadline.expect("active runtime has a deadline") {
                return None;
            }
            self.mode = RuntimeMode::Wiggling;
            self.path = Some(MotionPath::new(self.settings, cursor, self.next_seed));
            self.next_seed = self.next_seed.wrapping_add(1);
        }
        Some(
            self.path.expect("wiggling runtime has a path").sample(
                now - self
                    .idle_deadline
                    .expect("wiggling runtime has a start time"),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point() -> Point {
        Point { x: 100.0, y: 100.0 }
    }

    #[test]
    fn defaults_are_diagonal_and_five_seconds() {
        let settings = Settings::default();
        assert_eq!(settings.profile, Profile::Diagonal);
        assert_eq!(settings.delay, Duration::from_secs(5));
        assert_eq!(settings.amplitude, 5.0);
        assert_eq!(settings.speed, 3.0);
    }

    #[test]
    fn invalid_settings_are_clamped() {
        let settings = Settings {
            profile: Profile::Linear,
            delay: Duration::from_secs(0),
            amplitude: -1.0,
            speed: 100.0,
            start_with_windows: false,
        }
        .validated();
        assert_eq!(settings.delay, Duration::from_secs(1));
        assert_eq!(settings.amplitude, 0.0);
        assert_eq!(settings.speed, 20.0);
    }

    #[test]
    fn runtime_stays_in_user_control_until_delay_expires() {
        let now = Instant::now();
        let mut runtime = Runtime::new(Settings::default(), now);
        assert_eq!(runtime.tick(now + Duration::from_secs(4), point()), None);
        assert_eq!(runtime.mode(), RuntimeMode::UserControl);
        assert!(runtime
            .tick(now + Duration::from_secs(5), point())
            .is_some());
        assert_eq!(runtime.mode(), RuntimeMode::Wiggling);
    }

    #[test]
    fn genuine_activity_immediately_returns_control_and_resets_delay() {
        let now = Instant::now();
        let mut runtime = Runtime::new(Settings::default(), now);
        assert!(runtime
            .tick(now + Duration::from_secs(5), point())
            .is_some());
        runtime.on_mouse_activity(now + Duration::from_secs(5));
        assert_eq!(runtime.mode(), RuntimeMode::UserControl);
        assert_eq!(runtime.tick(now + Duration::from_secs(9), point()), None);
        assert!(runtime
            .tick(now + Duration::from_secs(10), point())
            .is_some());
    }

    #[test]
    fn pause_blocks_activity_and_resume_starts_a_fresh_delay() {
        let now = Instant::now();
        let mut runtime = Runtime::new(Settings::default(), now);
        runtime.pause();
        runtime.on_mouse_activity(now + Duration::from_secs(10));
        assert_eq!(runtime.tick(now + Duration::from_secs(20), point()), None);
        runtime.resume(now + Duration::from_secs(20));
        assert_eq!(runtime.tick(now + Duration::from_secs(24), point()), None);
        assert!(runtime
            .tick(now + Duration::from_secs(25), point())
            .is_some());
    }

    #[test]
    fn diagonal_retraces_between_reference_and_upper_left() {
        let path = MotionPath::new(Settings::default(), point(), 1);
        let start = path.sample(Duration::ZERO);
        let corner = path.sample(Duration::from_secs(10));
        let end = path.sample(Duration::from_secs(20));
        assert_eq!(start, point());
        assert!(corner.x < point().x && corner.y < point().y);
        assert_eq!(end, point());
        assert!(path.sample(Duration::from_secs(15)).x > corner.x);
    }

    #[test]
    fn all_profiles_are_bounded_around_the_reference() {
        for profile in [
            Profile::Linear,
            Profile::Diagonal,
            Profile::Lissajous,
            Profile::Brownian,
        ] {
            let settings = Settings {
                profile,
                amplitude: 5.0,
                ..Settings::default()
            };
            let path = MotionPath::new(settings, point(), 42);
            for step in 0..600 {
                let sample = path.sample(Duration::from_millis(step * 100));
                assert!((sample.x - 100.0).abs() <= 5.0 + f64::EPSILON);
                assert!((sample.y - 100.0).abs() <= 5.0 + f64::EPSILON);
            }
        }
    }
}
