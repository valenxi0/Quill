//! Windows-specific glue: keyboard hook, UI Automation, clipboard and
//! SendInput, permissions and system info.

pub mod hotkey;
mod escape;
pub mod paste;
pub mod system;
pub mod target;
pub mod uia;
