//! Only trained S1-mini control values can be rendered.

use crate::{context::AppContext, settings::CleanupLevel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Styling {
    Casual,
    SemiCasual,
    SemiFormal,
    Formal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Structure {
    Prose,
    Lists,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    General,
    Email,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Directive {
    pub styling: Styling,
    pub structure: Structure,
    pub context: Context,
}

impl Default for Directive {
    fn default() -> Self {
        Self { styling: Styling::SemiFormal, structure: Structure::Prose, context: Context::General }
    }
}

impl Directive {
    pub fn for_recording(level: CleanupLevel, app: AppContext, formal: bool) -> Option<Self> {
        if level == CleanupLevel::Verbatim || app == AppContext::Terminal {
            return None;
        }
        let polished = level != CleanupLevel::Light;
        Some(Self {
            styling: if !polished {
                Styling::Casual
            } else if formal {
                Styling::Formal
            } else {
                Styling::SemiFormal
            },
            structure: if level == CleanupLevel::Compose { Structure::Lists } else { Structure::Prose },
            context: if polished && app == AppContext::Email { Context::Email } else { Context::General },
        })
    }

    pub fn control_line(self) -> String {
        let styling = match self.styling {
            Styling::Casual => "casual",
            Styling::SemiCasual => "semi-casual",
            Styling::SemiFormal => "semi-formal",
            Styling::Formal => "formal",
        };
        let structure = match self.structure {
            Structure::Prose => "prose",
            Structure::Lists => "lists",
        };
        let context = match self.context {
            Context::General => "general",
            Context::Email => "email",
        };
        format!("[Styling: {styling}] [Structure: {structure}] [Context: {context}]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recording_mapping() {
        for level in [CleanupLevel::Verbatim, CleanupLevel::Light, CleanupLevel::Full, CleanupLevel::Compose] {
            for app in [AppContext::General, AppContext::Chat, AppContext::Email, AppContext::Terminal] {
                for formal in [false, true] {
                    let d = Directive::for_recording(level, app, formal);
                    if level == CleanupLevel::Verbatim || app == AppContext::Terminal {
                        assert_eq!(d, None);
                        continue;
                    }
                    let d = d.unwrap();
                    assert_eq!(
                        d.styling,
                        if level == CleanupLevel::Light {
                            Styling::Casual
                        } else if formal {
                            Styling::Formal
                        } else {
                            Styling::SemiFormal
                        }
                    );
                    assert_eq!(
                        d.structure,
                        if level == CleanupLevel::Compose { Structure::Lists } else { Structure::Prose }
                    );
                    assert_eq!(
                        d.context,
                        if level != CleanupLevel::Light && app == AppContext::Email {
                            Context::Email
                        } else {
                            Context::General
                        }
                    );
                }
            }
        }
        assert_eq!(Directive::default().control_line(), "[Styling: semi-formal] [Structure: prose] [Context: general]");
        assert_eq!(
            Directive::for_recording(CleanupLevel::Compose, AppContext::Email, true).unwrap().control_line(),
            "[Styling: formal] [Structure: lists] [Context: email]"
        );
    }
}
