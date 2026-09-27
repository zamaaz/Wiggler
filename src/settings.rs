use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::core::{Profile, Settings};

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn for_current_user() -> Self {
        let root = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        Self::new(root.join("Wiggler").join("settings.conf"))
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> io::Result<Settings> {
        match fs::read_to_string(&self.path) {
            Ok(contents) => Ok(parse(&contents).unwrap_or_default().validated()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, settings: Settings) -> io::Result<()> {
        let settings = settings.validated();
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let temporary = self.path.with_extension("tmp");
        fs::write(&temporary, serialize(settings))?;
        replace_saved_file(&temporary, &self.path)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(windows)]
fn replace_saved_file(temporary: &Path, target: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_WRITE_THROUGH, REPLACEFILE_WRITE_THROUGH,
    };

    let target_exists = target.exists();
    let temporary: Vec<u16> = temporary
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let target: Vec<u16> = target
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let replaced = if target_exists {
        unsafe {
            ReplaceFileW(
                target.as_ptr(),
                temporary.as_ptr(),
                std::ptr::null(),
                REPLACEFILE_WRITE_THROUGH,
                std::ptr::null(),
                std::ptr::null(),
            )
        }
    } else {
        unsafe { MoveFileExW(temporary.as_ptr(), target.as_ptr(), MOVEFILE_WRITE_THROUGH) }
    };
    if replaced == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_saved_file(temporary: &Path, target: &Path) -> io::Result<()> {
    fs::rename(temporary, target)
}

fn serialize(settings: Settings) -> String {
    format!(
        "profile={}\ndelay_seconds={}\namplitude_pixels={}\nspeed={}\nstart_with_windows={}\n",
        profile_name(settings.profile),
        settings.delay.as_secs(),
        settings.amplitude,
        settings.speed,
        settings.start_with_windows
    )
}

fn parse(contents: &str) -> Option<Settings> {
    let mut settings = Settings::default();
    for line in contents.lines() {
        let (key, value) = line.split_once('=')?;
        match key {
            "profile" => settings.profile = parse_profile(value)?,
            "delay_seconds" => settings.delay = Duration::from_secs(value.parse().ok()?),
            "amplitude_pixels" => settings.amplitude = value.parse().ok()?,
            "speed" => settings.speed = value.parse().ok()?,
            "start_with_windows" => settings.start_with_windows = value.parse().ok()?,
            _ => return None,
        }
    }
    Some(settings)
}

fn profile_name(profile: Profile) -> &'static str {
    match profile {
        Profile::Linear => "linear",
        Profile::Diagonal => "diagonal",
        Profile::Lissajous => "lissajous",
        Profile::Brownian => "brownian",
    }
}

fn parse_profile(value: &str) -> Option<Profile> {
    match value {
        "linear" => Some(Profile::Linear),
        "diagonal" => Some(Profile::Diagonal),
        "lissajous" => Some(Profile::Lissajous),
        "brownian" => Some(Profile::Brownian),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_path() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after the Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("wiggler-settings-{suffix}.conf"))
    }

    #[test]
    fn missing_file_loads_defaults() {
        let store = SettingsStore::new(temporary_path());
        assert_eq!(
            store.load().expect("missing settings are valid"),
            Settings::default()
        );
    }

    #[test]
    fn settings_round_trip_through_disk() {
        let path = temporary_path();
        let store = SettingsStore::new(&path);
        let expected = Settings {
            profile: Profile::Brownian,
            delay: Duration::from_secs(17),
            amplitude: 12.0,
            speed: 4.5,
            start_with_windows: true,
        };
        store.save(expected).expect("settings should save");
        assert_eq!(store.load().expect("settings should load"), expected);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn corrupt_settings_fall_back_to_defaults() {
        let path = temporary_path();
        fs::write(&path, "profile=not-a-profile\n").expect("test file should write");
        let store = SettingsStore::new(&path);
        assert_eq!(
            store.load().expect("corrupt settings are recoverable"),
            Settings::default()
        );
        let _ = fs::remove_file(path);
    }
}
