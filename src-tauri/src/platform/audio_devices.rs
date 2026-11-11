//! Capture endpoints: their friendly names, the Windows default, and which
//! ones are Bluetooth headsets.

use quill_core::microphone::{choose, is_bluetooth_instance, InputDevice, MicChoice};
use quill_core::settings::SavedDevice;
use windows::core::PCWSTR;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Device_IDW, CM_Get_Parent, CM_Locate_DevNodeW, CM_LOCATE_DEVNODE_NORMAL, CR_SUCCESS, MAX_DEVICE_ID_LEN,
};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{eCapture, eConsole, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};

fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    unsafe {
        // Already initialized (in either model) is fine.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
    }
}

fn id_of(device: &IMMDevice) -> Option<String> {
    unsafe {
        let p = device.GetId().ok()?;
        let id = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        id
    }
}

fn name_of(device: &IMMDevice) -> String {
    unsafe {
        let Ok(store) = device.OpenPropertyStore(STGM_READ) else { return String::new() };
        let Ok(mut value) = store.GetValue(&PKEY_Device_FriendlyName) else { return String::new() };
        let name = match PropVariantToStringAlloc(&value) {
            Ok(p) => {
                let s = p.to_string().unwrap_or_default();
                CoTaskMemFree(Some(p.0 as _));
                s
            }
            Err(_) => String::new(),
        };
        let _ = PropVariantClear(&mut value);
        name
    }
}

/// Whether the endpoint hangs off a Bluetooth device: its parent devnodes
/// carry a Bluetooth enumerator.
pub fn is_bluetooth(endpoint_id: &str) -> bool {
    let instance: Vec<u16> = format!("SWD\\MMDEVAPI\\{endpoint_id}").encode_utf16().chain([0]).collect();
    let mut node = 0u32;
    unsafe {
        if CM_Locate_DevNodeW(&mut node, PCWSTR(instance.as_ptr()), CM_LOCATE_DEVNODE_NORMAL) != CR_SUCCESS {
            return false;
        }
        for _ in 0..4 {
            let mut parent = 0u32;
            if CM_Get_Parent(&mut parent, node, 0) != CR_SUCCESS {
                return false;
            }
            let mut buf = [0u16; MAX_DEVICE_ID_LEN as usize + 1];
            if CM_Get_Device_IDW(parent, &mut buf, 0) != CR_SUCCESS {
                return false;
            }
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            if is_bluetooth_instance(&String::from_utf16_lossy(&buf[..len])) {
                return true;
            }
            node = parent;
        }
    }
    false
}

/// Active capture endpoints, and the default one's id.
pub fn inputs() -> (Vec<InputDevice>, Option<String>) {
    let Ok(e) = enumerator() else { return (Vec::new(), None) };
    let default = unsafe { e.GetDefaultAudioEndpoint(eCapture, eConsole) }.ok().and_then(|d| id_of(&d));
    let mut devices = Vec::new();
    if let Ok(collection) = unsafe { e.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) } {
        let count = unsafe { collection.GetCount() }.unwrap_or(0);
        for i in 0..count {
            let Ok(d) = (unsafe { collection.Item(i) }) else { continue };
            let Some(id) = id_of(&d) else { continue };
            let bluetooth = is_bluetooth(&id);
            devices.push(InputDevice { name: name_of(&d), id, bluetooth });
        }
    }
    (devices, default)
}

/// The default capture endpoint's id, without listing every device.
fn default_input_id() -> Option<String> {
    let e = enumerator().ok()?;
    let d = unsafe { e.GetDefaultAudioEndpoint(eCapture, eConsole) }.ok()?;
    id_of(&d)
}

/// The mic a recording should open. Lists devices only when it must: a
/// saved mic, or a Bluetooth default to look past.
pub fn resolve(saved: Option<&SavedDevice>, avoid_bluetooth: bool) -> MicChoice {
    use quill_core::microphone::Pick;
    if saved.is_none() {
        if !avoid_bluetooth {
            return MicChoice { pick: Pick::SystemDefault, name: None, missing: None, avoided: None };
        }
        match default_input_id() {
            Some(id) if !is_bluetooth(&id) => return MicChoice { pick: Pick::SystemDefault, name: None, missing: None, avoided: None },
            None => return MicChoice { pick: Pick::SystemDefault, name: None, missing: None, avoided: None },
            Some(_) => {}
        }
    }
    let (devices, default) = inputs();
    choose(&devices, default.as_deref(), saved, avoid_bluetooth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads this PC's audio devices"]
    fn list_capture_endpoints() {
        let (devices, default) = inputs();
        println!("default: {default:?}");
        for d in &devices {
            println!("{} | {} | bluetooth={}", d.id, d.name, d.bluetooth);
        }
        let c = resolve(None, true);
        println!("resolve(None, avoid=true): {c:?}");
        assert!(!devices.is_empty(), "at least one capture endpoint");
    }
}
