//! Windows-specific glue: keyboard hook, UI Automation, clipboard and
//! SendInput, permissions and system info.

pub mod apps;
pub mod audio_devices;
pub mod hotkey;
mod escape;
pub mod mouse;
pub mod other_audio;
pub mod paste;
pub mod screen;
pub mod system;
pub mod target;
pub mod icons;
pub mod uia;
