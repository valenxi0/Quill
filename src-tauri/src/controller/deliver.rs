use std::sync::Arc;
use std::time::{Duration, Instant};

use quill_core::context::AppContext;
use quill_core::errors::DictationError;
use quill_core::pipeline::{prepare, DeliveryOptions, SegmentResult};
use quill_core::state::Phase;
use quill_core::text::continuation::{same_window, LastPaste, SameWindow};
use quill_core::text::spoken::paste_chunks;

use crate::platform::{clicks, hotkey, paste, target};

use super::{Controller, DictationView, Metrics, SessionConfig};

impl SameWindow for target::Target {
    fn same_window(&self, other: &Self) -> bool {
        same_window(self.hwnd, &self.exe, other.hwnd, &other.exe)
    }
}

/// Keys typed and mouse buttons pressed so far; `None` unless both are
/// counted.
pub(super) fn input_count() -> Option<u32> {
    Some(hotkey::typed_keys()?.wrapping_add(clicks::clicks()?))
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

        // Only "press enter" was said: press it and deliver no text.
        if delivery.text.is_empty() {
            let pressed = paste::focus(&cfg.target, &own) && still_current() && paste::press_enter();
            if pressed {
                // The field moved on: the next dictation doesn't continue the last.
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

        // The clipboard is authoritative: write it first.
        let text = delivery.text.trim().to_string();
        *self.last_paste.lock() = Some(LastPaste {
            text: text.clone(),
            target: cfg.target.clone(),
            at: Instant::now(),
            input: None,
            pasted: false,
            entered: false,
        });
        let restore = self.stores.settings.read(|s| s.voice_dictation.restore_clipboard);
        let saved = if restore { paste::SavedClipboard::take() } else { None };
        let Some(mut written) = paste::write_clipboard_text(&delivery.text) else {
            self.fail_session(cfg.generation, DictationError::ClipboardFailed);
            return;
        };
        metrics.stop_to_clipboard_ms = stopped.elapsed().as_millis() as u64;
        let chunks = if cfg.recipe.context == AppContext::Terminal && cfg.splits_terminal_pastes {
            paste_chunks(&delivery.text)
        } else {
            vec![delivery.text.clone()]
        };
        // Checked again right before each keystroke: Esc can land while the
        // paste waits for the shortcut's modifiers to come up.
        let pasted = if chunks.len() > 1 {
            let sent = paste_in_pieces(&cfg.target, &own, &chunks, still_current);
            // Leave the whole text on the clipboard, not the last piece.
            written = paste::write_clipboard_text(&delivery.text).unwrap_or(written);
            sent
        } else {
            paste::paste(&cfg.target, &own, still_current)
        };
        let entered = pasted && delivery.press_enter && still_current() && {
            std::thread::sleep(Duration::from_millis(120));
            paste::press_enter()
        };
        let release_to_paste = stopped.elapsed();
        if pasted {
            if let Some(last) = self.last_paste.lock().as_mut() {
                *last = LastPaste { at: Instant::now(), input: input_count(), pasted: true, entered, ..last.clone() };
            }
        }
        // A failed paste leaves the transcript, so it can be pasted by hand.
        if let (true, Some(saved)) = (pasted, saved) {
            saved.restore_after(paste::restore_delay(&cfg.target), written);
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
            // Cancelled during delivery: nothing is recorded or learned.
            return;
        }
        self.record(cfg, std::slice::from_ref(&result), &raw, &text, &metrics, release_to_paste, pasted);
        *self.last_metrics.lock() = Some(metrics);
        self.shell.state_changed();
        self.schedule_idle_unload();
    }

    /// Paste the last transcript again, into the app it went to.
    pub fn paste_last(self: &Arc<Self>) {
        let Some(last) = self.last_paste.lock().clone() else { return };
        let own = self.shell.own_windows();
        let restore = self.stores.settings.read(|s| s.voice_dictation.restore_clipboard);
        let me = self.clone();
        std::thread::spawn(move || {
            let saved = if restore { paste::SavedClipboard::take() } else { None };
            let Some(written) = paste::write_clipboard_text(&last.text) else { return };
            if !paste::paste(&last.target, &own, || true) {
                return;
            }
            if let Some(saved) = saved {
                saved.restore_after(paste::restore_delay(&last.target), written);
            }
            let mut stored = me.last_paste.lock();
            // Only if no newer dictation replaced it meanwhile.
            if let Some(current) = stored.as_mut().filter(|c| c.at == last.at) {
                *current = LastPaste { at: Instant::now(), input: input_count(), pasted: true, entered: false, ..last };
            }
        });
    }
}

/// Pieces of a long terminal paste, one after another. Returns whether all
/// were sent.
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
