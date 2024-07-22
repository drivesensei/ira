#[cfg(target_os = "windows")]
use std::{error::Error, ffi::OsString, os::windows::ffi::OsStringExt};
#[cfg(target_os = "windows")]
use winapi::um::fileapi::GetVolumeInformationW;
#[cfg(target_os = "windows")]
use winapi::um::winnt::WCHAR;

#[cfg(target_os = "windows")]
pub fn get_volume_label(drive: &str) -> Result<String, Box<dyn Error>> {
    let mut volume_name: [WCHAR; 256] = [0; 256];
    let success = unsafe {
        GetVolumeInformationW(
            drive
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>()
                .as_ptr(),
            volume_name.as_mut_ptr(),
            volume_name.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };

    if success == 0 {
        return Err(Box::new(std::io::Error::last_os_error()));
    }

    let label = OsString::from_wide(&volume_name)
        .to_string_lossy()
        .into_owned();

    let clean_label: String = label.chars().filter(|c| c.is_ascii_graphic()).collect();

    Ok(clean_label)
}
