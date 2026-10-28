//! Windows-specific glue: keyboard hook, UI Automation, clipboard and
//! SendInput, permissions and system info.

pub mod apps;
pub mod hotkey;
mod escape;
pub mod mouse;
pub mod paste;
pub mod screen;
pub mod system;
pub mod target;
pub mod icons;
pub mod uia;
