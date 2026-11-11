//! Which microphone a recording uses: the saved one, the Windows default,
//! or another mic when the default is a Bluetooth headset.

use serde::Serialize;

use crate::settings::SavedDevice;

/// An active capture endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputDevice {
    /// The WASAPI endpoint id.
    pub id: String,
    pub name: String,
    pub bluetooth: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// Open the default device, which follows Windows' default as it changes.
    SystemDefault,
    Device(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MicChoice {
    pub pick: Pick,
    /// The mic that will record, when known.
    pub name: Option<String>,
    /// The saved mic, when it isn't connected.
    pub missing: Option<String>,
    /// The Bluetooth default skipped in favor of `name`.
    pub avoided: Option<String>,
}

/// Bluetooth Classic audio (hands-free and A2DP) sits under these device
/// enumerators. Recording from a hands-free endpoint drops the headset into
/// call mode.
pub fn is_bluetooth_instance(instance_id: &str) -> bool {
    let upper = instance_id.to_ascii_uppercase();
    upper.starts_with("BTHENUM\\") || upper.starts_with("BTHHFENUM\\")
}

pub fn choose(devices: &[InputDevice], default_id: Option<&str>, saved: Option<&SavedDevice>, avoid_bluetooth: bool) -> MicChoice {
    let mut missing = None;
    if let Some(saved) = saved {
        if let Some(d) = devices.iter().find(|d| d.id == saved.id) {
            return MicChoice { pick: Pick::Device(d.id.clone()), name: Some(d.name.clone()), missing: None, avoided: None };
        }
        missing = Some(saved.name.clone());
    }
    let default = default_id.and_then(|id| devices.iter().find(|d| d.id == id));
    if let Some(def) = default.filter(|d| avoid_bluetooth && d.bluetooth) {
        if let Some(other) = devices.iter().find(|d| !d.bluetooth) {
            return MicChoice {
                pick: Pick::Device(other.id.clone()),
                name: Some(other.name.clone()),
                missing,
                avoided: Some(def.name.clone()),
            };
        }
    }
    MicChoice { pick: Pick::SystemDefault, name: default.map(|d| d.name.clone()), missing, avoided: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, name: &str, bluetooth: bool) -> InputDevice {
        InputDevice { id: id.into(), name: name.into(), bluetooth }
    }

    fn saved(id: &str, name: &str) -> SavedDevice {
        SavedDevice { id: id.into(), name: name.into() }
    }

    #[test]
    fn bluetooth_enumerators() {
        assert!(is_bluetooth_instance(r"BTHHFENUM\{0000111e-0000-1000-8000-00805f9b34fb}_VID&0002004c_PID&2002\7&1"));
        assert!(is_bluetooth_instance(r"bthenum\{0000110b-0000-1000-8000-00805f9b34fb}_LOCALMFG&0002\7&2"));
        assert!(!is_bluetooth_instance(r"USB\VID_1235&PID_8211&MI_00\7&2bc20a84&0&0000"));
        assert!(!is_bluetooth_instance(r"HDAUDIO\FUNC_01&VEN_10EC&DEV_1220"));
        assert!(!is_bluetooth_instance(""));
    }

    #[test]
    fn system_default_follows_windows() {
        let devices = [dev("usb", "Microphone (Scarlett)", false), dev("hda", "Mic (Realtek)", false)];
        let c = choose(&devices, Some("hda"), None, true);
        assert_eq!(c.pick, Pick::SystemDefault);
        assert_eq!(c.name.as_deref(), Some("Mic (Realtek)"));
        assert_eq!((c.missing, c.avoided), (None, None));
    }

    #[test]
    fn saved_mic_wins_even_if_bluetooth() {
        let devices = [dev("usb", "USB mic", false), dev("bt", "Headset", true)];
        let c = choose(&devices, Some("usb"), Some(&saved("bt", "Headset")), true);
        assert_eq!(c.pick, Pick::Device("bt".into()));
        assert_eq!(c.avoided, None);
    }

    #[test]
    fn missing_saved_mic_falls_back_to_default() {
        let devices = [dev("usb", "USB mic", false)];
        let c = choose(&devices, Some("usb"), Some(&saved("gone", "Yeti")), true);
        assert_eq!(c.pick, Pick::SystemDefault);
        assert_eq!(c.missing.as_deref(), Some("Yeti"));
        assert_eq!(c.name.as_deref(), Some("USB mic"));
    }

    #[test]
    fn bluetooth_default_is_avoided_when_another_mic_exists() {
        let devices = [dev("bt", "Headset", true), dev("hda", "Laptop mic", false)];
        let c = choose(&devices, Some("bt"), None, true);
        assert_eq!(c.pick, Pick::Device("hda".into()));
        assert_eq!(c.avoided.as_deref(), Some("Headset"));
        assert_eq!(c.name.as_deref(), Some("Laptop mic"));

        let off = choose(&devices, Some("bt"), None, false);
        assert_eq!(off.pick, Pick::SystemDefault);

        let only_bt = choose(&devices[..1], Some("bt"), None, true);
        assert_eq!(only_bt.pick, Pick::SystemDefault);
        assert_eq!(only_bt.avoided, None);

        let missing_then_bt = choose(&devices, Some("bt"), Some(&saved("gone", "Yeti")), true);
        assert_eq!(missing_then_bt.pick, Pick::Device("hda".into()));
        assert_eq!(missing_then_bt.missing.as_deref(), Some("Yeti"));
    }

    #[test]
    fn no_devices() {
        let c = choose(&[], None, None, true);
        assert_eq!(c.pick, Pick::SystemDefault);
        assert_eq!(c.name, None);
    }
}
