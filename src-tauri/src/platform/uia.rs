//! UI Automation on one dedicated MTA thread. Elements never leave it; they
//! are referred to by id, so the correction watcher can reread a field later.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;

use windows::core::{Interface, BSTR};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use quill_core::text::insertion::Surroundings;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern, IUIAutomationTextRange,
    IUIAutomationValuePattern, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, TextUnit_Character,
    UIA_TextPatternId, UIA_ValuePatternId,
};

/// Characters read before and after the caret.
const BEFORE_CHARS: i32 = 300;
const AFTER_CHARS: i32 = 40;

type Job = Box<dyn FnOnce(&mut Uia) + Send>;

pub struct Uia {
    automation: Option<IUIAutomation>,
    elements: HashMap<u64, IUIAutomationElement>,
    next_id: u64,
}

static WORKER: OnceLock<Sender<Job>> = OnceLock::new();

fn worker() -> &'static Sender<Job> {
    WORKER.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        std::thread::Builder::new()
            .name("quill-uia".into())
            .spawn(move || {
                let automation = unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                    CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()
                };
                let mut uia = Uia { automation, elements: HashMap::new(), next_id: 1 };
                for job in rx {
                    job(&mut uia);
                }
            })
            .expect("spawn UIA thread");
        tx
    })
}

/// Run `f` on the UIA thread and wait for its result, with a timeout so a
/// hung app can't stall dictation.
pub fn run<R: Send + 'static>(timeout_ms: u64, f: impl FnOnce(&mut Uia) -> R + Send + 'static) -> Option<R> {
    let (tx, rx) = channel();
    worker()
        .send(Box::new(move |uia| {
            let _ = tx.send(f(uia));
        }))
        .ok()?;
    rx.recv_timeout(std::time::Duration::from_millis(timeout_ms)).ok()
}

#[derive(Debug, Clone, Default)]
pub struct FocusInfo {
    pub element_id: Option<u64>,
    pub name: String,
    pub is_password: bool,
    pub pid: u32,
    /// The text around the caret, when asked for and readable.
    pub surroundings: Option<Surroundings>,
}

/// Text from a range, without the object placeholders and carriage returns
/// some apps put in.
unsafe fn range_text(range: &IUIAutomationTextRange, max: i32) -> Option<String> {
    let text = unsafe { range.GetText(max) }.ok()?.to_string();
    Some(text.replace("\r\n", "\n").replace('\r', "\n").replace('\u{FFFC}', ""))
}

/// Before, after, and the selection, through the element's TextPattern.
/// `None` when the app doesn't say where the caret is.
unsafe fn surroundings(element: &IUIAutomationElement) -> Option<Surroundings> {
    unsafe {
        let pattern = element.GetCurrentPattern(UIA_TextPatternId).ok()?.cast::<IUIAutomationTextPattern>().ok()?;
        let ranges = pattern.GetSelection().ok()?;
        if ranges.Length().ok()? < 1 {
            return None;
        }
        let selection = ranges.GetElement(0).ok()?;
        let before = selection.Clone().ok()?;
        before.MoveEndpointByRange(TextPatternRangeEndpoint_End, &selection, TextPatternRangeEndpoint_Start).ok()?;
        before.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -BEFORE_CHARS).ok()?;
        let after = selection.Clone().ok()?;
        after.MoveEndpointByRange(TextPatternRangeEndpoint_Start, &selection, TextPatternRangeEndpoint_End).ok()?;
        after.MoveEndpointByUnit(TextPatternRangeEndpoint_End, TextUnit_Character, AFTER_CHARS).ok()?;
        let around = Surroundings {
            before: range_text(&before, BEFORE_CHARS)?,
            after: range_text(&after, AFTER_CHARS)?,
            selection: range_text(&selection, 4000)?,
        };
        // An empty field can report its placeholder ("Message #general") as
        // its text. Text equal to the field's name or help text counts as an
        // empty field.
        let whole = format!("{}{}{}", around.before, around.selection, around.after);
        let whole = whole.trim();
        let label = |b: windows::core::Result<BSTR>| b.map(|b| b.to_string()).unwrap_or_default();
        if !whole.is_empty()
            && [label(element.CurrentName()), label(element.CurrentHelpText())].iter().any(|l| l.trim() == whole)
        {
            return Some(Surroundings::default());
        }
        Some(around)
    }
}

impl Uia {
    /// The focused element, remembered under a new id. With `around`, also
    /// the text around its caret.
    pub fn focused(&mut self, around: bool) -> FocusInfo {
        let Some(automation) = &self.automation else { return FocusInfo::default() };
        unsafe {
            let Ok(element) = automation.GetFocusedElement() else { return FocusInfo::default() };
            let name = element.CurrentName().map(|b| b.to_string()).unwrap_or_default();
            let is_password = element.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false);
            let pid = element.CurrentProcessId().unwrap_or(0) as u32;
            let id = self.next_id;
            self.next_id += 1;
            // Keep only a few elements alive.
            if self.elements.len() > 8 {
                let oldest = *self.elements.keys().min().unwrap();
                self.elements.remove(&oldest);
            }
            let surroundings = if around && !is_password { surroundings(&element) } else { None };
            self.elements.insert(id, element);
            FocusInfo { element_id: Some(id), name, is_password, pid, surroundings }
        }
    }

    /// The text of a remembered element, through ValuePattern or TextPattern.
    /// Never reads password fields.
    pub fn read_text(&mut self, id: u64) -> Option<String> {
        let element = self.elements.get(&id)?;
        unsafe {
            if element.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(true) {
                return None;
            }
            if let Ok(p) = element.GetCurrentPattern(UIA_ValuePatternId) {
                if let Ok(vp) = p.cast::<IUIAutomationValuePattern>() {
                    if let Ok(v) = vp.CurrentValue() {
                        let s: String = BSTR::to_string(&v);
                        if !s.is_empty() {
                            return Some(s);
                        }
                    }
                }
            }
            if let Ok(p) = element.GetCurrentPattern(UIA_TextPatternId) {
                if let Ok(tp) = p.cast::<IUIAutomationTextPattern>() {
                    if let Ok(range) = tp.DocumentRange() {
                        if let Ok(text) = range.GetText(-1) {
                            return Some(text.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    pub fn forget(&mut self, id: u64) {
        self.elements.remove(&id);
    }
}
