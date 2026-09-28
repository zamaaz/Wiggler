use std::ffi::c_void;
use std::io;
use std::ptr::null_mut;

use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "Wiggler";

pub fn set_enabled(enabled: bool) -> io::Result<()> {
    if enabled {
        let key = open_key(KEY_READ | KEY_WRITE)?;
        let command = command_line()?;
        let bytes = encode_wide(&command);
        let result = unsafe {
            RegSetValueExW(
                key,
                encode_wide(VALUE_NAME).as_ptr(),
                0,
                REG_SZ,
                bytes.as_ptr() as *const u8,
                (bytes.len() * 2) as u32,
            )
        };
        unsafe {
            RegCloseKey(key);
        }
        result_to_io(result)
    } else {
        let key = open_key(KEY_WRITE)?;
        let result = unsafe { RegDeleteValueW(key, encode_wide(VALUE_NAME).as_ptr()) };
        unsafe {
            RegCloseKey(key);
        }
        if result == ERROR_FILE_NOT_FOUND {
            Ok(())
        } else {
            result_to_io(result)
        }
    }
}

pub fn repair() -> io::Result<()> {
    let expected = command_line()?;
    let key = open_key(KEY_READ | KEY_WRITE)?;
    let mut buffer = [0u16; 1024];
    let mut bytes = (buffer.len() * 2) as u32;
    let result = unsafe {
        RegGetValueW(
            key,
            null_mut(),
            encode_wide(VALUE_NAME).as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buffer.as_mut_ptr() as *mut c_void,
            &mut bytes,
        )
    };
    unsafe {
        RegCloseKey(key);
    }
    if result == ERROR_FILE_NOT_FOUND {
        return set_enabled(true);
    }
    result_to_io(result)?;
    let actual = String::from_utf16_lossy(&buffer[..(bytes as usize / 2).saturating_sub(1)]);
    if actual == expected {
        Ok(())
    } else {
        set_enabled(true)
    }
}

fn open_key(access: u32) -> io::Result<HKEY> {
    let mut key = null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            encode_wide(RUN_KEY).as_ptr(),
            0,
            null_mut(),
            REG_OPTION_NON_VOLATILE,
            access,
            null_mut(),
            &mut key,
            null_mut(),
        )
    };
    result_to_io(result).map(|_| key)
}

fn command_line() -> io::Result<String> {
    Ok(format!("\"{}\"", std::env::current_exe()?.display()))
}

fn encode_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn result_to_io(result: u32) -> io::Result<()> {
    if result == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32))
    }
}

const ERROR_SUCCESS: u32 = 0;
const ERROR_FILE_NOT_FOUND: u32 = 2;
