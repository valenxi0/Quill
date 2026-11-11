use std::sync::Arc;
use std::time::Duration;

use super::Controller;

impl Controller {
    pub(super) fn schedule_idle_unload(self: &Arc<Self>) {
        let me = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(crate::engines::IDLE_UNLOAD + Duration::from_secs(1));
            if !me.phase().is_busy() && me.engines.unload_if_idle() {
                me.shell.state_changed();
            }
        });
    }

    /// Settings → "Load Engine", and the warm-up after a download.
    pub fn warm_selected(self: &Arc<Self>) {
        let (engine, hotwords) = self.stores.settings.read(|s| (s.voice_dictation.engine, s.voice_dictation.wants_hotwords()));
        self.engines.set_hotwords(hotwords);
        let info = engine.info();
        if info.required.iter().any(|a| !self.models.is_ready(*a)) {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            *me.warm_error.lock() = None;
            me.shell.state_changed();
            let result = me.engines.warm(engine);
            *me.warm_error.lock() = result.err();
            me.shell.state_changed();
            me.schedule_idle_unload();
        });
    }
}
