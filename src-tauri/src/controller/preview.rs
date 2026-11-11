use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;
use std::time::{Duration, Instant};

use quill_core::segments::pad;
use quill_core::state::Phase;
use quill_core::text::finish::join_seams;

use super::{Controller, PREVIEW_WINDOW, Session};

impl Controller {
    pub(super) fn poll_preview(self: &Arc<Self>, generation: u64, s: &mut Session) {
        let Some(p) = s.preview.as_mut() else { return };
        let Some(rec) = s.recording.as_ref() else { return };
        if Instant::now() < p.next_at || p.busy.load(SeqCst) || !self.engines.recognizer_loaded(p.engine) {
            return;
        }
        let committed = s.segmenter.committed();
        let (audio, len) = rec.with_samples(|samples| {
            let start = committed.max(samples.len().saturating_sub(PREVIEW_WINDOW));
            (samples[start.min(samples.len())..].to_vec(), samples.len())
        });
        let fresh = p.covered.max(committed)..len;
        if fresh.is_empty() || s.vad.as_ref().is_some_and(|v| !v.map.has_speech(&fresh)) {
            p.next_at = Instant::now() + Duration::from_millis(300);
            return;
        }
        p.covered = len;
        p.busy.store(true, SeqCst);
        let me = self.clone();
        let (engine, request, earlier, busy) = (p.engine, p.request.clone(), p.committed.clone(), p.busy.clone());
        let started = Instant::now();
        std::thread::spawn(move || {
            let heard = me
                .engines
                .with_recognizer(engine, |e| e.transcribe(&pad(&audio, 1.0), &request))
                .ok()
                .and_then(|(r, _)| r.ok())
                .map(|t| t.text);
            let spent = started.elapsed();
            if let Some(s) = me.session.lock().as_mut().filter(|s| s.generation == generation) {
                if let Some(p) = s.preview.as_mut() {
                    p.next_at = Instant::now() + spent.max(Duration::from_millis(700));
                }
            }
            busy.store(false, SeqCst);
            if let Some(heard) = heard {
                if me.is_current(generation) && me.phase() == Phase::Recording {
                    let mut parts = earlier.lock().clone();
                    parts.push(heard);
                    me.shell.preview(&join_seams(&parts));
                }
            }
        });
    }
}
