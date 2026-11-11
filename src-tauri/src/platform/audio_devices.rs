use quill_core::microphone::{choose, is_bluetooth_instance, InputDevice, MicChoice};
use quill_core::settings::SavedDevice;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Device_IDW, CM_Get_Parent, CM_Locate_DevNodeW, CM_LOCATE_DEVNODE_NORMAL, CR_SUCCESS, MAX_DEVICE_ID_LEN,
};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    eCapture, eCommunications, eConsole, eMultimedia, Handset, Headset, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, Microphone,
    PKEY_AudioEndpoint_FormFactor, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc, PropVariantToUInt32, PROPVARIANT};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};

fn enumerator() -> windows::core::Result<IMMDeviceEnumerator> {
    unsafe {
        // Already initialized (in either model) is fine.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
    }
}

fn take_string(p: PWSTR) -> Option<String> {
    unsafe {
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as _));
        s
    }
}

fn id_of(device: &IMMDevice) -> Option<String> {
    take_string(unsafe { device.GetId() }.ok()?)
}

fn property<T>(device: &IMMDevice, key: &PROPERTYKEY, read: impl FnOnce(&PROPVARIANT) -> Option<T>) -> Option<T> {
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let mut value = store.GetValue(key).ok()?;
        let out = read(&value);
        let _ = PropVariantClear(&mut value);
        out
    }
}

fn name_of(device: &IMMDevice) -> String {
    property(device, &PKEY_Device_FriendlyName, |v| take_string(unsafe { PropVariantToStringAlloc(v) }.ok()?)).unwrap_or_default()
}

fn is_mic(device: &IMMDevice) -> bool {
    let form = property(device, &PKEY_AudioEndpoint_FormFactor, |v| unsafe { PropVariantToUInt32(v).ok() });
    form.is_some_and(|f| [Microphone, Headset, Handset].iter().any(|m| m.0 as u32 == f))
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

fn defaults(e: &IMMDeviceEnumerator) -> Vec<String> {
    [eConsole, eMultimedia, eCommunications]
        .into_iter()
        .filter_map(|role| unsafe { e.GetDefaultAudioEndpoint(eCapture, role) }.ok().and_then(|d| id_of(&d)))
        .collect()
}

pub fn inputs() -> (Vec<InputDevice>, Vec<String>) {
    let Ok(e) = enumerator() else { return (Vec::new(), Vec::new()) };
    let mut devices = Vec::new();
    if let Ok(collection) = unsafe { e.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) } {
        let count = unsafe { collection.GetCount() }.unwrap_or(0);
        for i in 0..count {
            let Ok(d) = (unsafe { collection.Item(i) }) else { continue };
            let Some(id) = id_of(&d) else { continue };
            devices.push(InputDevice { name: name_of(&d), is_mic: is_mic(&d), bluetooth: is_bluetooth(&id), id });
        }
    }
    (devices, defaults(&e))
}

fn is_active_input(e: &IMMDeviceEnumerator, id: &str) -> bool {
    unsafe { e.GetDevice(&HSTRING::from(id)) }.and_then(|d| unsafe { d.GetState() }).is_ok_and(|s| s == DEVICE_STATE_ACTIVE)
}

pub fn resolve(saved: Option<&SavedDevice>, avoid_bluetooth: bool) -> MicChoice {
    let Ok(e) = enumerator() else { return MicChoice::system_default(None, None) };
    if let Some(s) = saved {
        if is_active_input(&e, &s.id) {
            return MicChoice::saved(s);
        }
    } else if !avoid_bluetooth {
        return MicChoice::system_default(None, None);
    } else {
        let default = unsafe { e.GetDefaultAudioEndpoint(eCapture, eConsole) }.ok().and_then(|d| id_of(&d));
        if default.is_none_or(|id| !is_bluetooth(&id)) {
            return MicChoice::system_default(None, None);
        }
    }
    let (devices, defaults) = inputs();
    let defaults: Vec<&str> = defaults.iter().map(String::as_str).collect();
    choose(&devices, &defaults, saved, avoid_bluetooth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads this PC's audio devices"]
    fn list_capture_endpoints() {
        let (devices, defaults) = inputs();
        println!("defaults (console, multimedia, communications): {defaults:?}");
        for d in &devices {
            println!("{} | {} | bluetooth={} mic={}", d.id, d.name, d.bluetooth, d.is_mic);
        }
        println!("resolve(None, avoid=true): {:?}", resolve(None, true));
        let saved = SavedDevice { id: devices[0].id.clone(), name: devices[0].name.clone() };
        println!("resolve(saved, avoid=true): {:?}", resolve(Some(&saved), true));
        assert!(!devices.is_empty(), "at least one capture endpoint");
    }
}
