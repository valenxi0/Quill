//! The live half of a Sketch: ink over the monitor under the pointer, a bar
//! with the controls, and a screenshot per group of marks, per click and per
//! settled screen change. The idea and its interaction come from Scrawl by
//! Kit Langton (github.com/kitlangton/scrawl, MIT), by way of Nano.
//!
//! Hold the draw key and move the pointer to draw, or turn on Draw and drag.
//! Clicks and scrolls otherwise pass through to the app underneath. A click,
//! a scroll or an app switch means the screen may have changed, so the next
//! mark starts a new screenshot.

use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use quill_core::segments::SAMPLE_RATE;
use quill_core::settings::DrawKey;
use quill_core::sketch::{
    clock, stroke_length, ChangeWatcher, Image, Moment, MomentKind, Place, Point, Stroke, MAXIMUM_FRAMES,
    MINIMUM_STROKE_LENGTH,
};
use serde::Serialize;

use crate::platform::mouse::{MouseEvent, MouseHook};
use crate::platform::screen::{self, Monitor};
use crate::shell::Shell;

/// What the Sketch bar shows.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SketchView {
    pub marks: usize,
    pub pen_on: bool,
    pub draw_key: String,
    pub finish_shortcut: String,
    /// Unix milliseconds, for the bar's clock.
    pub started_at: i64,
}

/// The bar's size in logical pixels.
pub const BAR_WIDTH: f64 = 560.0;
pub const BAR_HEIGHT: f64 = 52.0;

/// Where the bar sits on `m`, in physical pixels (left, top, right, bottom):
/// centred, above the dictation pill.
pub fn bar_rect(m: &Monitor) -> (i32, i32, i32, i32) {
    let s = m.scale as f64;
    let (wl, _, wr, wb) = m.work;
    let w = (BAR_WIDTH * s) as i32;
    let h = (BAR_HEIGHT * s) as i32;
    let left = wl + (wr - wl - w) / 2;
    // The pill's window is 72 px tall, 24 px above the taskbar.
    let bottom = wb - ((24.0 + 72.0 + 4.0) * s) as i32;
    (left, bottom - h, left + w, bottom)
}

struct Frame {
    image: Option<Arc<Image>>,
    strokes: Vec<Stroke>,
    place: Place,
    /// Bumped by each retake, so an older capture never lands after a newer one.
    shots: u64,
}

struct Click {
    sample: usize,
    point: Point,
    place: Place,
    image: Option<Arc<Image>>,
}

struct Change {
    sample: usize,
    image: Arc<Image>,
    place: Place,
}

#[derive(Default)]
struct Inner {
    frames: Vec<Frame>,
    drawing: Vec<Point>,
    drawing_start: usize,
    live_dirty: bool,
    screen_changed: bool,
    pen_on: bool,
    pen_down: bool,
    clicks: Vec<Click>,
    changes: Vec<Change>,
    start_shot: Option<Arc<Image>>,
    start_place: Place,
}

pub struct Session {
    shell: Arc<dyn Shell>,
    pub monitor: Monitor,
    clock: Box<dyn Fn() -> usize + Send + Sync>,
    started: Instant,
    start_sample: usize,
    view: Mutex<SketchView>,
    inner: Arc<Mutex<Inner>>,
    hook: Mutex<Option<MouseHook>>,
    stopped: AtomicBool,
    threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

/// Everything worth considering for the screenshots.
pub struct Capture {
    pub moments: Vec<Moment>,
    pub monitor: Monitor,
}

impl Session {
    /// Shows the ink and the bar on `monitor` and starts watching the mouse
    /// and the screen. `clock` is the recording's sample count.
    pub fn start(
        shell: Arc<dyn Shell>,
        monitor: Monitor,
        draw_key: DrawKey,
        finish_shortcut: String,
        clock: impl Fn() -> usize + Send + Sync + 'static,
    ) -> anyhow::Result<Arc<Session>> {
        let start_sample = clock();
        let view = SketchView {
            marks: 0,
            pen_on: false,
            draw_key: draw_key.name().into(),
            finish_shortcut,
            started_at: chrono::Utc::now().timestamp_millis(),
        };
        let session = Arc::new(Session {
            shell,
            monitor,
            clock: Box::new(clock),
            started: Instant::now(),
            start_sample,
            view: Mutex::new(view.clone()),
            inner: Arc::new(Mutex::new(Inner { screen_changed: true, start_place: screen::front_place(), ..Default::default() })),
            hook: Mutex::new(None),
            stopped: AtomicBool::new(false),
            threads: Mutex::new(Vec::new()),
        });
        session.shell.sketch_show(&monitor, &view);

        let (tx, rx) = channel();
        *session.hook.lock() = Some(MouseHook::install(draw_key.modifier(), tx)?);
        let me = session.clone();
        let input = std::thread::Builder::new().name("quill-sketch-input".into()).spawn(move || loop {
            match rx.recv_timeout(Duration::from_millis(16)) {
                Ok(event) => me.handle(event),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            me.flush_live();
        })?;
        let me = session.clone();
        let watcher = std::thread::Builder::new().name("quill-sketch-watch".into()).spawn(move || me.watch())?;
        // The screen before anything happens, for words said before the first
        // mark or click.
        let me = session.clone();
        let first = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(600));
            if !me.stopped.load(SeqCst) {
                let shot = screen::capture(&me.monitor).map(Arc::new);
                me.inner.lock().start_shot = shot;
            }
        });
        session.threads.lock().extend([input, watcher, first]);
        Ok(session)
    }

    pub fn view(&self) -> SketchView {
        self.view.lock().clone()
    }

    fn publish(&self) {
        let (marks, pen_on) = {
            let inner = self.inner.lock();
            (inner.frames.iter().map(|f| f.strokes.len()).sum(), inner.pen_on)
        };
        let view = {
            let mut v = self.view.lock();
            v.marks = marks;
            v.pen_on = pen_on;
            v.clone()
        };
        self.shell.sketch_view(&view);
    }

    pub fn toggle_pen(&self) {
        let on = {
            let mut inner = self.inner.lock();
            inner.pen_on = !inner.pen_on;
            inner.pen_down = false;
            inner.pen_on
        };
        self.end_drawing();
        self.shell.sketch_pen(on);
        self.publish();
    }

    /// Removes the most recent mark.
    pub fn undo(&self) {
        {
            let mut inner = self.inner.lock();
            let Some(i) = inner.frames.iter().rposition(|f| !f.strokes.is_empty()) else { return };
            inner.frames[i].strokes.pop();
            if inner.frames[i].strokes.is_empty() {
                inner.frames.remove(i);
                if inner.frames.is_empty() {
                    inner.screen_changed = true;
                }
            }
        }
        self.shell.sketch_undo();
        self.publish();
    }

    fn seconds(&self, sample: usize) -> f64 {
        sample.saturating_sub(self.start_sample) as f64 / SAMPLE_RATE as f64
    }

    /// Monitor-relative physical pixels to the ink window's CSS pixels.
    fn css(&self, points: &[Point]) -> Vec<(f32, f32)> {
        points.iter().map(|p| (p.x / self.monitor.scale, p.y / self.monitor.scale)).collect()
    }

    fn local(&self, x: i32, y: i32) -> Point {
        Point { x: (x - self.monitor.left) as f32, y: (y - self.monitor.top) as f32 }
    }

    fn over_bar(&self, x: i32, y: i32) -> bool {
        let (l, t, r, b) = bar_rect(&self.monitor);
        x >= l && x < r && y >= t && y < b
    }

    fn handle(&self, event: MouseEvent) {
        let (pen_on, pen_down, drawing) = {
            let i = self.inner.lock();
            (i.pen_on, i.pen_down, !i.drawing.is_empty())
        };
        match event {
            MouseEvent::Move { x, y, draw } => {
                if pen_on {
                    if pen_down {
                        self.add(x, y);
                    }
                } else if !draw || !self.monitor.contains(x, y) {
                    if drawing {
                        self.end_drawing();
                    }
                } else {
                    self.add(x, y);
                }
            }
            MouseEvent::LeftDown { x, y } => {
                // A click on the bar doesn't change what is being marked.
                if self.over_bar(x, y) {
                    return;
                }
                if pen_on {
                    if self.monitor.contains(x, y) {
                        self.end_drawing();
                        self.inner.lock().pen_down = true;
                        self.add(x, y);
                    }
                    return;
                }
                self.inner.lock().screen_changed = true;
                if self.monitor.contains(x, y) {
                    self.click(x, y);
                }
            }
            MouseEvent::LeftUp { x, y } => {
                if pen_on && pen_down {
                    self.add(x, y);
                    self.inner.lock().pen_down = false;
                    self.end_drawing();
                }
            }
            MouseEvent::Other => {
                if !pen_on {
                    self.inner.lock().screen_changed = true;
                }
            }
        }
    }

    fn add(&self, x: i32, y: i32) {
        let point = self.local(x, y);
        let mut inner = self.inner.lock();
        if inner.drawing.is_empty() {
            inner.drawing_start = (self.clock)();
        } else if let Some(last) = inner.drawing.last() {
            if (last.x - point.x).hypot(last.y - point.y) < 0.75 {
                return;
            }
        }
        inner.drawing.push(point);
        inner.live_dirty = true;
    }

    fn flush_live(&self) {
        let points = {
            let mut inner = self.inner.lock();
            if !inner.live_dirty {
                return;
            }
            inner.live_dirty = false;
            inner.drawing.clone()
        };
        self.shell.sketch_live(&self.css(&points));
    }

    fn end_drawing(&self) {
        let (points, start) = {
            let mut inner = self.inner.lock();
            inner.live_dirty = false;
            (std::mem::take(&mut inner.drawing), inner.drawing_start)
        };
        self.shell.sketch_live(&[]);
        if stroke_length(&points) < MINIMUM_STROKE_LENGTH {
            return;
        }
        let middle = points[points.len() / 2];
        let (index, first, shots) = {
            let mut inner = self.inner.lock();
            if (inner.screen_changed || inner.frames.is_empty()) && inner.frames.len() < MAXIMUM_FRAMES {
                inner.frames.push(Frame { image: None, strokes: Vec::new(), place: Place::default(), shots: 0 });
                inner.screen_changed = false;
            }
            let number = inner.frames.iter().map(|f| f.strokes.len()).sum::<usize>() + 1;
            let index = inner.frames.len() - 1;
            let frame = &mut inner.frames[index];
            frame.strokes.push(Stroke { points: points.clone(), start_sample: start, number });
            frame.shots += 1;
            (index, frame.strokes.len() == 1, frame.shots)
        };
        self.shell.sketch_commit(&self.css(&points), &clock(self.seconds(start)));
        self.publish();
        // Each new mark retakes its group's screenshot, so the image is the
        // screen as it looked when the last of those marks was drawn.
        let monitor = self.monitor;
        let (mx, my) = (monitor.left + middle.x as i32, monitor.top + middle.y as i32);
        let shot = std::thread::spawn({
            let inner = self.inner.clone();
            move || {
                let place = first.then(|| {
                    let mut p = screen::place_at(mx, my);
                    p.element = None;
                    p
                });
                let image = screen::capture(&monitor).map(Arc::new);
                let mut inner = inner.lock();
                if let Some(frame) = inner.frames.get_mut(index) {
                    if let Some(place) = place {
                        frame.place = place;
                    }
                    if frame.shots == shots && image.is_some() {
                        frame.image = image;
                    }
                }
            }
        });
        self.threads.lock().push(shot);
    }

    /// What was clicked, as it looked at the click.
    fn click(&self, x: i32, y: i32) {
        let sample = (self.clock)();
        let point = self.local(x, y);
        let index = {
            let mut inner = self.inner.lock();
            if inner.clicks.len() >= 40 {
                return;
            }
            inner.clicks.push(Click { sample, point, place: Place::default(), image: None });
            inner.clicks.len() - 1
        };
        let monitor = self.monitor;
        let inner = self.inner.clone();
        let shot = std::thread::spawn(move || {
            let image = screen::capture(&monitor).map(Arc::new);
            let place = screen::place_at(x, y);
            let mut inner = inner.lock();
            if let Some(click) = inner.clicks.get_mut(index) {
                click.image = image;
                click.place = place;
            }
        });
        self.threads.lock().push(shot);
    }

    /// Five times a second: has the screen changed and settled, or has
    /// another app come forward?
    fn watch(&self) {
        let mut watcher = ChangeWatcher::new();
        let mut front = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0 as isize;
        while !self.stopped.load(SeqCst) {
            std::thread::sleep(Duration::from_millis(200));
            let now = self.started.elapsed().as_secs_f64();
            let fg = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() }.0 as isize;
            if fg != front {
                front = fg;
                self.inner.lock().screen_changed = true;
            }
            if let Some(grid) = screen::grid(&self.monitor) {
                watcher.feed(grid, now);
            }
            if let Some(start) = watcher.settled(now) {
                if let Some(image) = screen::capture(&self.monitor) {
                    let sample = self.start_sample + (start * SAMPLE_RATE as f64) as usize;
                    let mut inner = self.inner.lock();
                    inner.screen_changed = true;
                    if inner.changes.len() < 40 {
                        inner.changes.push(Change { sample, image: Arc::new(image), place: screen::front_place() });
                    }
                }
            }
        }
    }

    fn stop(&self) {
        if self.stopped.swap(true, SeqCst) {
            return;
        }
        // Dropping the hook closes the input channel.
        self.hook.lock().take();
        self.shell.sketch_hide();
    }

    fn join(&self) {
        loop {
            let threads: Vec<_> = std::mem::take(&mut *self.threads.lock());
            if threads.is_empty() {
                return;
            }
            for t in threads {
                let _ = t.join();
            }
        }
    }

    /// Ends the session and returns every screenshot worth considering.
    pub fn finish(&self) -> Capture {
        self.end_drawing();
        // The screen now, for marks whose screenshot hasn't landed. Taken
        // before the windows close, while the pill is still kept out of it.
        let now = screen::capture(&self.monitor).map(Arc::new);
        self.stop();
        self.join();
        let mut inner = self.inner.lock();
        let mut moments = Vec::new();
        if let Some(image) = inner.start_shot.clone() {
            moments.push(Moment {
                kind: MomentKind::Start,
                sample: self.start_sample,
                image,
                strokes: vec![],
                click: None,
                place: inner.start_place.clone(),
            });
        }
        for click in inner.clicks.drain(..) {
            let Some(image) = click.image else { continue };
            moments.push(Moment {
                kind: MomentKind::Click,
                sample: click.sample,
                image,
                strokes: vec![],
                click: Some(click.point),
                place: click.place,
            });
        }
        for frame in inner.frames.drain(..) {
            if frame.strokes.is_empty() {
                continue;
            }
            let image = match frame.image {
                Some(i) => Some(i),
                None => now.clone(),
            };
            let Some(image) = image else { continue };
            let sample = frame.strokes.iter().map(|s| s.start_sample).min().unwrap_or(0);
            moments.push(Moment { kind: MomentKind::Marks, sample, image, strokes: frame.strokes, click: None, place: frame.place });
        }
        for change in inner.changes.drain(..) {
            moments.push(Moment {
                kind: MomentKind::Change,
                sample: change.sample,
                image: change.image,
                strokes: vec![],
                click: None,
                place: change.place,
            });
        }
        if moments.is_empty() {
            if let Some(image) = now.clone() {
                moments.push(Moment {
                    kind: MomentKind::Start,
                    sample: self.start_sample,
                    image,
                    strokes: vec![],
                    click: None,
                    place: inner.start_place.clone(),
                });
            }
        }
        Capture { moments, monitor: self.monitor }
    }

    pub fn cancel(&self) {
        self.stop();
        let mut inner = self.inner.lock();
        inner.frames.clear();
        inner.clicks.clear();
        inner.changes.clear();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop();
    }
}
