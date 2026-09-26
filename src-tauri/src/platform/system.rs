//! Microphone permission, CPU topology, preferred languages, and motion
//! settings (sections 2, 5.1, 5.3).

use serde::Serialize;
use windows::core::PCWSTR;
use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
use windows::Win32::System::Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};
use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MicPermission {
    Allowed,
    Denied,
}

fn reg_string(root: HKEY, path: &str, value: &str) -> Option<String> {
    let path: Vec<u16> = path.encode_utf16().chain([0]).collect();
    let value: Vec<u16> = value.encode_utf16().chain([0]).collect();
    let mut buf = [0u16; 64];
    let mut len = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            root,
            PCWSTR(path.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut len),
        )
    };
    if rc.is_err() {
        return None;
    }
    let chars = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars]))
}

const CONSENT: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

/// Windows' privacy switches: the machine-wide one, the user's, and the one
/// for desktop apps (`NonPackaged`).
pub fn microphone_permission() -> MicPermission {
    let denied = |root, path: &str| reg_string(root, path, "Value").is_some_and(|v| v.eq_ignore_ascii_case("Deny"));
    let non_packaged = format!(r"{CONSENT}\NonPackaged");
    if denied(HKEY_LOCAL_MACHINE, CONSENT) || denied(HKEY_CURRENT_USER, CONSENT) || denied(HKEY_CURRENT_USER, &non_packaged) {
        MicPermission::Denied
    } else {
        MicPermission::Allowed
    }
}

/// Performance cores on hybrid CPUs, physical cores otherwise, clamped 2..8.
pub fn inference_threads() -> usize {
    let classes = core_efficiency_classes();
    let n = if classes.is_empty() {
        std::thread::available_parallelism().map(|n| n.get() / 2).unwrap_or(4)
    } else {
        let top = *classes.iter().max().unwrap();
        classes.iter().filter(|c| **c == top).count()
    };
    n.clamp(2, 8)
}

fn core_efficiency_classes() -> Vec<u8> {
    unsafe {
        let mut len = 0u32;
        let _ = GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut len);
        if len == 0 {
            return vec![];
        }
        let mut buf = vec![0u8; len as usize];
        if GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            Some(buf.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX),
            &mut len,
        )
        .is_err()
        {
            return vec![];
        }
        let mut out = Vec::new();
        let mut offset = 0usize;
        while offset < len as usize {
            let info = &*(buf.as_ptr().add(offset) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX);
            out.push(info.Anonymous.Processor.EfficiencyClass);
            offset += info.Size as usize;
            if info.Size == 0 {
                break;
            }
        }
        out
    }
}

/// BCP-47 tags of the user's preferred UI languages, most preferred first.
pub fn preferred_languages() -> Vec<String> {
    unsafe {
        let mut count = 0u32;
        let mut len = 0u32;
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, None, &mut len).is_err() {
            return vec![];
        }
        let mut buf = vec![0u16; len as usize];
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len)
            .is_err()
        {
            return vec![];
        }
        buf.split(|c| *c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
    }
}

/// Windows "Show animations in Windows".
pub fn animations_enabled() -> bool {
    let mut enabled = windows::core::BOOL(1);
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    enabled.as_bool()
}
