use std::cell::Cell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use quill_core::context::AppContext;
use quill_core::errors::DictationError;
use quill_core::pipeline::{prepare, DeliveryOptions, SegmentResult};
use quill_core::state::Phase;
use quill_core::text::continuation::{same_window, LastPaste, SameWindow};
use quill_core::text::spoken::paste_chunks;

use crate::platform::{input_activity, paste, target};

use super::{Controller, DictationView, Metrics, SessionConfig};

impl SameWindow for target::Target {
    fn same_window(&self, other: &Self) -> bool {
        same_window(self.hwnd, &self.exe, other.hwnd, &other.exe)
    }
}

impl Controller {
    pub(super) fn deliver(self: &Arc<Self>, cfg: &SessionConfig, result: SegmentResult, mut metrics: Metrics, stopped: Instant, rms: f32) {
        if !self.is_current(cfg.generation) {
            return;
        }
        let (raw, text) = (result.raw.clone(), result.text.clone());
        let options = DeliveryOptions {
            spoken_enter: cfg.spoken_enter,
            around: cfg.target.surroundings.as_ref(),
            protected: &cfg.protected,
        };
        let delivery = match prepare(&raw, &text, cfg.recipe.context, rms, &options) {
            Ok(d) => d,
            Err(e) => return self.fail_session(cfg.generation, e),
        };
        if !self.is_current(cfg.generation) {
            return;
        }
        let own = self.shell.own_windows();
        let still_current = || self.is_current(cfg.generation);

        if delivery.text.is_empty() {
            let pressed = paste::focus(&cfg.target, &own) && still_current() && paste::press_enter();
            if pressed {
                if let Some(last) = self.last_paste.lock().as_mut() {
                    last.pasted = false;
                }
            }
            self.outcome(
                Some(cfg.generation),
                DictationView {
                    phase: Phase::Completed,
                    outcome: Some(if pressed { "Enter pressed" } else { "Nothing to paste" }.into()),
                    context: cfg.recipe.context,
                    ..Default::default()
                },
            );
            return;
        }

        let text = delivery.text.trim().to_string();
        *self.last_paste.lock() = Some(LastPaste::delivered(text.clone(), cfg.target.clone()));
        let (restore, fits) = self.stores.settings.read(|s| (s.voice_dictation.restore_clipboard, s.voice_dictation.fits_to_cursor));
        let saved = if restore { paste::SavedClipboard::take() } else { None };
        let Some(mut written) = paste::write_clipboard_text(&delivery.text) else {
            if let Some(saved) = saved {
                saved.restore_now(None);
            }
            self.fail_session(cfg.generation, DictationError::ClipboardFailed);
            return;
        };
        metrics.stop_to_clipboard_ms = stopped.elapsed().as_millis() as u64;
        if fits {
            input_activity::watch();
        }
        let chunks = if cfg.recipe.context == AppContext::Terminal && cfg.splits_terminal_pastes {
            paste_chunks(&delivery.text)
        } else {
            vec![delivery.text.clone()]
        };
        let input_at_paste = Cell::new(None);
        let before_keys = || {
            input_at_paste.set(input_activity::count());
            still_current()
        };
        let pasted = if chunks.len() > 1 {
            let sent = paste_in_pieces(&cfg.target, &own, &chunks, before_keys);
            written = paste::write_clipboard_text(&delivery.text).unwrap_or(written);
            sent
        } else {
            paste::paste(&cfg.target, &own, before_keys)
        };
        let entered = pasted && delivery.press_enter && still_current() && {
            std::thread::sleep(Duration::from_millis(120));
            paste::press_enter()
        };
        let release_to_paste = stopped.elapsed();
        if pasted {
            if let Some(last) = self.last_paste.lock().as_mut() {
                *last = LastPaste { at: Instant::now(), input: input_at_paste.get(), pasted: true, entered, ..last.clone() };
            }
        }
        match (pasted, saved) {
            (true, Some(saved)) => {
                saved.restore_after(paste::restore_delay(&cfg.target), written);
            }
            (false, Some(saved)) if !still_current() => {
                saved.restore_now(Some(written));
            }
            _ => {}
        }
        if !pasted {
            input_activity::stop();
        }

        let outcome = match (pasted, entered) {
            (true, true) => "Pasted and sent",
            (true, false) => "Paste sent",
            _ => "Copied only",
        };
        let shown = self.outcome(
            Some(cfg.generation),
            DictationView {
                phase: Phase::Completed,
                outcome: Some(outcome.into()),
                error_title: None,
                error_detail: None,
                context: cfg.recipe.context,
            },
        );
        if !shown {
            return;
        }
        self.record(cfg, std::slice::from_ref(&result), &raw, &text, &metrics, release_to_paste, pasted);
        *self.last_metrics.lock() = Some(metrics);
        self.shell.state_changed();
        self.schedule_idle_unload();
    }

    pub fn paste_last(self: &Arc<Self>) {
        let Some(last) = self.last_paste.lock().clone() else { return };
        let own = self.shell.own_windows();
        let (restore, fits) = self.stores.settings.read(|s| (s.voice_dictation.restore_clipboard, s.voice_dictation.fits_to_cursor));
        let me = self.clone();
        std::thread::spawn(move || {
            let saved = if restore { paste::SavedClipboard::take() } else { None };
            let Some(written) = paste::write_clipboard_text(&last.text) else {
                if let Some(saved) = saved {
                    saved.restore_now(None);
                }
                return;
            };
            if fits {
                input_activity::watch();
            }
            let input_at_paste = Cell::new(None);
            if !paste::paste(&last.target, &own, || {
                input_at_paste.set(input_activity::count());
                true
            }) {
                input_activity::stop();
                return;
            }
            if let Some(saved) = saved {
                saved.restore_after(paste::restore_delay(&last.target), written);
            }
            let mut stored = me.last_paste.lock();
            if let Some(current) = stored.as_mut().filter(|c| c.at == last.at) {
                *current = LastPaste { at: Instant::now(), input: input_at_paste.get(), pasted: true, entered: false, ..last };
            }
        });
    }
}

pub(super) fn paste_in_pieces(target: &target::Target, own: &[isize], chunks: &[String], still_current: impl Fn() -> bool) -> bool {
    for (i, chunk) in chunks.iter().enumerate() {
        if i > 0 {
            // Let the app read the clipboard before it changes.
            std::thread::sleep(Duration::from_millis(150));
        }
        if !still_current() || !paste::set_clipboard_text(chunk) || !paste::paste(target, own, &still_current) {
            return false;
        }
    }
    true
}
