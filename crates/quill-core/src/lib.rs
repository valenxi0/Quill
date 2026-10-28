//! Platform-independent core of Quill. Nothing here touches Windows, audio
//! devices, or models, so it is fully unit-testable.

pub mod context;
pub mod corrections;
pub mod engines;
pub mod errors;
pub mod history;
pub mod langid;
pub mod models;
pub mod pipeline;
pub mod resample;
pub mod segments;
pub mod settings;
pub mod sketch;
pub mod speech;
pub mod state;
pub mod stats;
pub mod store;
pub mod text;
