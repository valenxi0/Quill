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
    /// A microphone, headset or handset, as opposed to a line input,
    /// "Stereo Mix", a capture card, or a virtual cable.
    pub is_mic: bool,
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

impl MicChoice {
    pub fn system_default(name: Option<String>, missing: Option<String>) -> Self {
        Self { pick: Pick::SystemDefault, name, missing, avoided: None }
    }

    /// The saved mic, found connected.
    pub fn saved(s: &SavedDevice) -> Self {
        Self { pick: Pick::Device(s.id.clone()), name: Some(s.name.clone()), missing: None, avoided: None }
    }

    fn device(d: &InputDevice, missing: Option<String>, avoided: Option<String>) -> Self {
        Self { pick: Pick::Device(d.id.clone()), name: Some(d.name.clone()), missing, avoided }
    }
}

/// Bluetooth Classic audio (hands-free and A2DP) sits under these device
/// enumerators. Recording from a hands-free endpoint drops the headset into
/// call mode.
pub fn is_bluetooth_instance(instance_id: &str) -> bool {
    let upper = instance_id.to_ascii_uppercase();
    upper.starts_with("BTHENUM\\") || upper.starts_with("BTHHFENUM\\")
}

/// `defaults` are Windows' default capture endpoints by role: the console
/// default first (what "System default" opens), then the multimedia and
/// communications defaults.
pub fn choose(devices: &[InputDevice], defaults: &[&str], saved: Option<&SavedDevice>, avoid_bluetooth: bool) -> MicChoice {
    let find = |id: &str| devices.iter().find(|d| d.id == id);
    let mut missing = None;
    if let Some(saved) = saved {
        if let Some(d) = find(&saved.id) {
            return MicChoice::device(d, None, None);
        }
        missing = Some(saved.name.clone());
    }
    let default = defaults.first().and_then(|id| find(id));
    if let Some(def) = default.filter(|d| avoid_bluetooth && d.bluetooth) {
        let other = defaults[1..]
            .iter()
            .filter_map(|id| find(id))
            .find(|d| !d.bluetooth)
            .or_else(|| devices.iter().find(|d| d.is_mic && !d.bluetooth));
        if let Some(other) = other {
            return MicChoice::device(other, missing, Some(def.name.clone()));
        }
    }
    MicChoice::system_default(default.map(|d| d.name.clone()), missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, name: &str, bluetooth: bool) -> InputDevice {
        InputDevice { id: id.into(), name: name.into(), bluetooth, is_mic: true }
    }

    fn line(id: &str, name: &str) -> InputDevice {
        InputDevice { id: id.into(), name: name.into(), bluetooth: false, is_mic: false }
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
        let c = choose(&devices, &["hda"], None, true);
        assert_eq!(c, MicChoice::system_default(Some("Mic (Realtek)".into()), None));
    }

    #[test]
    fn saved_mic_wins_even_if_bluetooth() {
        let devices = [dev("usb", "USB mic", false), dev("bt", "Headset", true)];
        let c = choose(&devices, &["usb"], Some(&saved("bt", "Headset")), true);
        assert_eq!(c.pick, Pick::Device("bt".into()));
        assert_eq!(c.avoided, None);
    }

    #[test]
    fn missing_saved_mic_falls_back_to_default() {
        let devices = [dev("usb", "USB mic", false)];
        let c = choose(&devices, &["usb"], Some(&saved("gone", "Yeti")), true);
        assert_eq!(c, MicChoice::system_default(Some("USB mic".into()), Some("Yeti".into())));
    }

    #[test]
    fn bluetooth_default_is_avoided_when_another_mic_exists() {
        let devices = [dev("bt", "Headset", true), dev("hda", "Laptop mic", false)];
        let c = choose(&devices, &["bt"], None, true);
        assert_eq!(c.pick, Pick::Device("hda".into()));
        assert_eq!(c.avoided.as_deref(), Some("Headset"));
        assert_eq!(c.name.as_deref(), Some("Laptop mic"));

        assert_eq!(choose(&devices, &["bt"], None, false).pick, Pick::SystemDefault);

        let only_bt = choose(&devices[..1], &["bt"], None, true);
        assert_eq!(only_bt, MicChoice::system_default(Some("Headset".into()), None));

        let missing_then_bt = choose(&devices, &["bt"], Some(&saved("gone", "Yeti")), true);
        assert_eq!(missing_then_bt.pick, Pick::Device("hda".into()));
        assert_eq!(missing_then_bt.missing.as_deref(), Some("Yeti"));
    }

    #[test]
    fn avoidance_prefers_another_windows_default() {
        let devices = [dev("bt", "Headset", true), dev("hda", "Laptop mic", false), dev("usb", "Desk mic", false)];
        let c = choose(&devices, &["bt", "bt", "usb"], None, true);
        assert_eq!(c.pick, Pick::Device("usb".into()), "the communications default, not the first mic");
    }

    #[test]
    fn avoidance_never_picks_a_line_input() {
        let devices = [dev("bt", "Headset", true), line("mix", "Stereo Mix"), line("cable", "CABLE Output"), dev("hda", "Laptop mic", false)];
        assert_eq!(choose(&devices, &["bt"], None, true).pick, Pick::Device("hda".into()));

        let no_mic = [dev("bt", "Headset", true), line("mix", "Stereo Mix")];
        let c = choose(&no_mic, &["bt"], None, true);
        assert_eq!(c, MicChoice::system_default(Some("Headset".into()), None), "the headset beats recording silence");

        let line_default = [dev("bt", "Headset", true), line("mix", "Stereo Mix"), dev("hda", "Laptop mic", false)];
        assert_eq!(choose(&line_default, &["bt", "bt", "bt"], None, true).pick, Pick::Device("hda".into()));
    }

    #[test]
    fn no_devices() {
        assert_eq!(choose(&[], &[], None, true), MicChoice::system_default(None, None));
    }
}
