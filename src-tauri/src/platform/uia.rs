//! UI Automation on one dedicated MTA thread. Elements never leave it; they
//! are referred to by id, so the correction watcher can reread a field later.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;

use windows::core::{Interface, BSTR};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationValuePattern, UIA_TextPatternId, UIA_ValuePatternId,
};

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
}

impl Uia {
    /// The focused element, remembered under a new id.
    pub fn focused(&mut self) -> FocusInfo {
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
            self.elements.insert(id, element);
            FocusInfo { element_id: Some(id), name, is_password, pid }
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
