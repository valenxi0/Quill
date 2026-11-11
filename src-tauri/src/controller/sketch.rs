use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;
use std::time::{Duration, Instant};

use quill_core::context::AppContext;
use quill_core::errors::DictationError;
use quill_core::pipeline::SegmentResult;
use quill_core::segments::{split, SAMPLE_RATE};
use quill_core::state::Phase;
use quill_core::text::finish::{has_speech, join};
use quill_core::speech::SpeechMap;
use quill_core::text::spoken::paste_chunks;

use crate::platform::{paste, uia};

use super::deliver::paste_in_pieces;
use super::{Controller, DictationView, Metrics, SessionConfig};

impl Controller {
    /// The Sketch shortcut: start a Sketch, or finish the one running.
    pub fn sketch(self: &Arc<Self>) {
        if self.sketch.lock().is_some() {
            if self.phase() == Phase::Recording {
                self.stop();
            }
            return;
        }
        if !self.stores.settings.read(|s| s.sketch.enabled) || self.phase().is_busy() {
            return;
        }
        let Some(monitor) = crate::platform::screen::monitor_under_pointer() else { return };
        if self.starting.swap(true, SeqCst) {
            return;
        }
        self.start_inner(Some(monitor));
        self.starting.store(false, SeqCst);
    }

    /// The Sketch bar's buttons and Ctrl+Z.
    pub fn sketch_action(self: &Arc<Self>, action: &str) {
        let Some(session) = self.sketch.lock().clone() else { return };
        match action {
            "pen" => session.toggle_pen(),
            "undo" => session.undo(),
            "cancel" => self.cancel(),
            "finish" if self.phase() == Phase::Recording => self.stop(),
            _ => {}
        }
    }

    pub fn sketch_view(&self) -> Option<crate::sketch::SketchView> {
        self.sketch.lock().as_ref().map(|s| s.view())
    }

    pub(super) fn end_sketch(&self) -> Option<Arc<crate::sketch::Session>> {
        if let Some(h) = &self.hook {
            h.set_sketching(false);
        }
        self.sketch.lock().take()
    }

    /// Transcribes the pieces between marks, stitches them with the mark
    /// numbers, draws the marks onto their screenshots, and pastes both into
    /// the field the Sketch was started from: images first, then the text.
    pub(super) fn deliver_sketch(
        self: &Arc<Self>,
        cfg: &SessionConfig,
        samples: &[f32],
        speech: Option<&SpeechMap>,
        session: &crate::sketch::Session,
        stopped: Instant,
    ) {
        use quill_core::sketch::{self as qs, render::Renderer, FrameLine, MomentKind};
        let capture = session.finish();
        if !self.is_current(cfg.generation) {
            return;
        }
        let seconds = samples.len() as f64 / SAMPLE_RATE as f64;
        let mut metrics = Metrics { audio_seconds: seconds, ..Default::default() };
        // Marks, clicks and settled screen changes, cut down to the
        // screenshots worth sending.
        let moments = qs::pick(capture.moments, seconds);
        let anchors = qs::anchors(&moments);
        let marks: Vec<(usize, usize)> = anchors.iter().enumerate().map(|(i, a)| (i, a.sample)).collect();
        let cuts = qs::cuts(&marks, samples);
        let mut results: Vec<SegmentResult> = Vec::new();
        let mut texts: Vec<String> = Vec::new();
        for range in qs::pieces(samples.len(), &cuts) {
            let piece = &samples[range.clone()];
            let voiced = speech.is_none_or(|m| m.has_speech(&range));
            if piece.len() < SAMPLE_RATE / 4 || !voiced || !has_speech(piece, SAMPLE_RATE as u32) {
                texts.push(String::new());
                continue;
            }
            // Long stretches between marks are recognized in pause-aware
            // parts, then cleaned together.
            let mut heard = Vec::new();
            for part in split(piece, 20.0) {
                match self.recognize(cfg, &piece[part], &mut metrics) {
                    Ok(h) => heard.push(h),
                    Err(e) => return self.fail_session(cfg.generation, e),
                }
            }
            let r = self.clean(cfg, &heard, &mut metrics);
            texts.push(r.text.clone());
            results.push(r);
        }
        if !self.is_current(cfg.generation) {
            return;
        }
        let raw = join(&results.iter().map(|r| r.raw.clone()).collect::<Vec<_>>());
        let spoke = !raw.trim().is_empty();
        if !spoke && moments.iter().all(|m| m.strokes.is_empty()) {
            return self.fail_session(cfg.generation, DictationError::NoSpeech);
        }
        let rate = SAMPLE_RATE as f64;
        // A single screenshot of an untouched screen needs no list.
        let listed = moments.len() > 1 || moments.iter().any(|m| m.kind != MomentKind::Start);
        let frames: Vec<FrameLine> = if listed {
            moments
                .iter()
                .map(|m| FrameLine { time: m.sample as f64 / rate, tag: m.tag(), place: m.place.description() })
                .collect()
        } else {
            Vec::new()
        };
        let body = qs::compose(&texts, &cuts, &anchors, &frames);

        // Every screenshot full size, each marked one followed by a crop
        // around its ink; the sheet of all of them is saved as well.
        let renderer = Renderer::new(std::fs::read(r"C:\Windows\Fonts\segoeuib.ttf").ok());
        let mut images: Vec<(String, qs::Image)> = Vec::new();
        let mut rendered = Vec::new();
        for (i, m) in moments.iter().enumerate() {
            let badge = listed.then(|| format!("#{}  {}", i + 1, qs::clock(m.sample as f64 / rate)));
            let frame = renderer.render(m, badge.as_deref());
            images.push((format!("frame-{}.png", i + 1), frame.clone()));
            let ink: Vec<qs::Point> = m.strokes.iter().flat_map(|s| s.points.iter().copied()).collect();
            if !ink.is_empty() {
                if let Some(crop) = renderer.crop(&frame, &ink, m.image.width) {
                    images.push((format!("frame-{}-zoom.png", i + 1), crop));
                }
            }
            rendered.push(frame);
        }
        let encoded: Vec<crate::sketch_store::Shot> = images
            .into_iter()
            .map(|(name, image)| {
                let png = qs::render::encode_png(&image);
                crate::sketch_store::Shot { name, image, png }
            })
            .collect();
        let saved = crate::sketch_store::save(renderer.contact_sheet(&rendered).as_ref(), &encoded);
        let m = capture.monitor;
        let folder = saved.as_ref().map(|s| s.folder.to_string_lossy().to_string());
        let header = qs::header(seconds, folder.as_deref(), Some((m.width as u32, m.height as u32, m.scale)));
        let text = if listed { format!("{header}\n{}\n{body}", qs::PREAMBLE) } else { format!("{header}\n{body}") };
        if !self.is_current(cfg.generation) {
            return;
        }

        // Copied first, with the screenshot folder in the text, so nothing
        // is lost if the paste can't happen.
        *self.last_paste.lock() = Some(quill_core::text::continuation::LastPaste {
            text: text.clone(),
            target: cfg.target.clone(),
            at: Instant::now(),
            input: None,
            pasted: false,
            entered: false,
        });
        if !paste::set_clipboard_text(&text) {
            return self.fail_session(cfg.generation, DictationError::ClipboardFailed);
        }
        metrics.stop_to_clipboard_ms = stopped.elapsed().as_millis() as u64;
        let own = self.shell.own_windows();
        let still_current = || self.is_current(cfg.generation);
        let terminal = cfg.recipe.context == AppContext::Terminal;
        let paths = saved.as_ref().map(|s| s.images.clone()).unwrap_or_default();
        // A GUI composer takes PNG data as an attachment; a terminal agent
        // such as Claude Code takes a pasted image path.
        let mut pasted = !encoded.is_empty() && paste::focus(&cfg.target, &own) && still_current();
        // Clicks while sketching move focus around the app; put it back in
        // the field the Sketch started from.
        if let (true, Some(id)) = (pasted, cfg.target.element_id) {
            if uia::run(400, move |u| u.set_focus(id)) == Some(true) {
                std::thread::sleep(Duration::from_millis(150));
            }
        }
        if pasted {
            for (i, shot) in encoded.iter().enumerate() {
                let written = if terminal {
                    paths.get(i).is_some_and(|p| paste::set_clipboard_text(&p.to_string_lossy()))
                } else {
                    paste::set_clipboard_image(&shot.png, shot.image.width, shot.image.height, &shot.image.rgba)
                };
                if !written || !paste::paste(&cfg.target, &own, still_current) {
                    pasted = false;
                    break;
                }
                // Electron reads the clipboard after the key event; give it
                // time before the clipboard changes under it.
                std::thread::sleep(Duration::from_millis(if terminal { 250 } else { 400 }));
            }
        }
        if pasted {
            let chunks = if terminal && cfg.splits_terminal_pastes { paste_chunks(&text) } else { vec![text.clone()] };
            pasted = paste_in_pieces(&cfg.target, &own, &chunks, still_current);
        }
        // Leave the whole text on the clipboard.
        paste::set_clipboard_text(&text);
        let release_to_paste = stopped.elapsed();
        if encoded.is_empty() {
            self.fail_session(cfg.generation, DictationError::ScreenCaptureFailed);
            return;
        }
        let shown = self.outcome(
            Some(cfg.generation),
            DictationView {
                phase: Phase::Completed,
                outcome: Some(if pasted { "Sketch pasted" } else { "Copied only" }.into()),
                context: cfg.recipe.context,
                ..Default::default()
            },
        );
        if !shown {
            return;
        }
        // History and stats get what was said; nothing is learned from a
        // Sketch's field.
        self.record(cfg, &results, &raw, &body, &metrics, release_to_paste, false);
        *self.last_metrics.lock() = Some(metrics);
        self.shell.state_changed();
        self.schedule_idle_unload();
    }
}
