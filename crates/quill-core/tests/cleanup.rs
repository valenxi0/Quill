use quill_core::{
    context::{classify, AppContext},
    pipeline::{process_segment, Cleaner, Recipe},
    settings::{CleanupLevel, VoiceDictation},
    text::{
        directive::Directive,
        finish::{join, join_segment},
        guard::{figures, reason, GuardReason},
        numbers::default_itn,
        vocabulary::{EntryKind, VocabEntry},
    },
};

fn recipe() -> Recipe {
    Recipe {
        directive: None,
        layout_cues: true,
        context: AppContext::General,
        writes_numbers: false,
        has_cleanup: true,
        vocabulary: vec![],
    }
}
fn layout(raw: &str) -> String {
    join(&[process_segment(raw, Some("en"), &recipe(), None, None).text])
}

#[test]
fn cue_examples() {
    for (raw, expected) in [
        (
            "Hi John, new line, thanks for the update. New paragraph. See you Friday.",
            "Hi John,\nThanks for the update.\n\nSee you Friday.",
        ),
        (
            "that covers the budget, new paragraph, on staffing we are fine",
            "that covers the budget.\n\nOn staffing we are fine",
        ),
        ("ship it today new paragraph next topic", "ship it today\n\nNext topic"),
        ("milk, new line eggs", "milk,\nEggs"),
        ("milk , new line eggs", "milk,\nEggs"),
        ("the@ new paragraph here", "the@\n\nHere"),
        ("new line second item", "Second item"),
        ("milk; newline: eggs", "milk;\nEggs"),
        ("hello: new paragraph next", "hello.\n\nNext"),
    ] {
        assert_eq!(layout(raw), expected, "{raw}");
    }
}

#[test]
fn literal_cues() {
    for raw in [
        "Add a new line at the end of the file.",
        "They launched a new line of products.",
        "The new paragraph reads better.",
        "We need a brand new line item for travel.",
        "Then start the new line of thought here",
        "Press the “new line” key.",
        "we drew new line art today",
        "say new, line now",
        "new (line) please",
        "new line— now",
        "new paragraph of text",
        "@newline now",
        "new line@ now",
        "new@ line now",
        "new @line now",
        "newline characters",
        "say \"newline\" now",
    ] {
        assert_eq!(layout(raw), raw);
    }
    for blocker in
        "a an the this that another each every one any no per my your our their its his her whole entire brand"
            .split_whitespace()
    {
        let raw = format!("{blocker} new paragraph here");
        assert_eq!(layout(&raw), raw);
        assert_eq!(layout(&format!("{blocker}, new paragraph here")), format!("{blocker}.\n\nHere"));
    }
    for follower in ["of", "item", "items", "character", "characters", "break", "breaks"] {
        let raw = format!("new line {follower}");
        assert_eq!(layout(&raw), raw);
        assert_eq!(layout(&format!("new line, {follower}")), quill_core::text::layout::capitalize(follower));
    }
}

#[test]
fn joins_and_segment_edges() {
    for (pieces, expected) in [
        (vec!["One.", "\n\n", "\n", "Two."], "One.\n\nTwo."),
        (vec!["Bring:", "- Sunscreen\n- Water"], "Bring:\n- Sunscreen\n- Water"),
        (vec!["It cost", "1.5 million."], "It cost 1.5 million."),
        (vec!["Bring:", "1) Water", "2. Tents"], "Bring:\n1) Water\n2. Tents"),
        (vec!["\n\nFirst\n", "\n\nSecond\n"], "First\n\nSecond"),
        (vec!["hello", ",", "world", "!"], "hello, world!"),
    ] {
        assert_eq!(join(&pieces.iter().map(|s| s.to_string()).collect::<Vec<_>>()), expected);
    }
    let a = process_segment("One. New paragraph", Some("en"), &recipe(), None, None).text;
    let b = process_segment("new line two", Some("en"), &recipe(), None, None).text;
    assert_eq!(a, "One.\n\n");
    assert_eq!(b, "\nTwo");
    assert_eq!(join(&[a, b]), "One.\n\nTwo");
    assert_eq!(join_segment(&["\n\n".into(), "\n".into()]), "\n\n");
}

#[test]
fn cues_and_snippets_bypass_cleaner() {
    struct Echo(Vec<String>);
    impl Cleaner for Echo {
        fn clean(&mut self, text: &str, _: Directive) -> Result<String, String> {
            self.0.push(text.into());
            Ok(text.into())
        }
    }
    let mut r = recipe();
    r.directive = Some(Directive::default());
    let snippet = VocabEntry::new(EntryKind::Snippet, "my signature", "lowercase signature");
    r.vocabulary.push(snippet.clone());
    let mut cleaner = Echo(vec![]);
    let out = process_segment("hi, new line my signature new paragraph bye", Some("en"), &r, Some(&mut cleaner), None);
    assert_eq!(out.text, "hi,\nlowercase signature\n\nBye");
    assert_eq!(out.hits, vec![snippet.id]);
    assert_eq!(cleaner.0, ["hi,", "Bye"]);
    r.vocabulary.push(VocabEntry::new(EntryKind::Snippet, "new line", "literal expansion"));
    assert_eq!(process_segment("new line", Some("en"), &r, None, None).text, "literal expansion");
    r.vocabulary.clear();
    r.layout_cues = false;
    assert_eq!(process_segment("new line here", Some("en"), &r, None, None).text, "new line here");
    r.layout_cues = true;
    r.context = AppContext::Terminal;
    r.directive = None;
    assert!(!process_segment("new paragraph here", Some("en"), &r, None, None).text.contains('\n'));
}

#[test]
fn figure_extraction_and_rejections() {
    assert_eq!(figures("$23,450 by 4:30 on March 3rd, 2026"), ["23450", "4", "30", "3", "2026"]);
    assert_eq!(figures("555-0198 and 1,234,567"), ["555", "0198", "1234567"]);
    let itn = default_itn();
    for (raw, cleaned, figure) in [
        (
            "call me back at five five five zero one nine eight before the fourteenth",
            "Call me at 555-0189 before the 14th.",
            "0189",
        ),
        ("the meeting is at four thirty", "The meeting is at 5:30.", "5"),
        ("three cats", "3 cats and 2 dogs", "2"),
    ] {
        assert_eq!(reason(raw, cleaned, itn.as_deref()), Some(GuardReason::UnspokenFigure(figure.into())));
    }
}

#[test]
fn guard_accepts_expected_rewrites() {
    let itn = default_itn();
    for (raw, cleaned) in [
        ("ship it at three no four", "Ship it at 4."),
        ("it's about a hundred dollars", "It's about $100."),
        ("we raised a million dollars", "We raised $1 million."),
        ("two and a half hours", "2.5 hours"),
        ("version 3 point 2 is out", "Version 3.2 is out."),
        ("room 204 on the 3rd floor", "Room 204 on the third floor."),
        ("first apples second pears third peaches", "1. Apples\n2. Pears\n3. Peaches"),
        ("Let's meet on Tuesday. Actually, scratch that. Let's meet on Thursday.", "Let's meet on Thursday."),
        ("Email jorge dot valen at example dot com", "Email jorge.valen@example.com"),
        (
            "Bring sunscreen tents water blankets and flashlights",
            "- Sunscreen\n- Tents\n- Water\n- Blankets\n- Flashlights",
        ),
        ("call five five five zero one nine eight", "Call 555-0198"),
        ("I don't think it won't work", "I do not think it will not work"),
    ] {
        assert_eq!(reason(raw, cleaned, itn.as_deref()), None, "{raw} -> {cleaned}");
    }
}

#[test]
fn sentence_loss_and_word_rules() {
    let first = "Priya reviewed Kubernetes deployment metrics yesterday.";
    let middle = "Jorge scheduled customer interviews across Berlin offices.";
    let last = "Morgan prepared release documentation covering database migrations.";
    let raw = format!("{first} {middle} {last}");
    assert!(matches!(reason(&raw, &format!("{first} {last}"), None), Some(GuardReason::DroppedSentence(_))));
    assert!(matches!(reason(&raw, first, None), Some(GuardReason::DroppedSentence(_))));
    assert_eq!(reason(&raw, &raw, None), None);
    assert_eq!(
        reason("cats dogs birds trees flowers", "Cats", None),
        Some(GuardReason::DroppedSentence("cats dogs birds trees flowers".into()))
    );
    assert_eq!(reason("cats dogs birds trees", "Cats", None), None);
    assert_eq!(reason("they're we're it's like really actually sixteen should", "", None), None);
    assert_eq!(reason("Kubernetes deployment metrics interviews customer", "Kubernetes deployments", None), None);
}

#[test]
fn settings_migration_and_email_context() {
    let vd: VoiceDictation = serde_json::from_str(r#"{"cleanupLevel":"full"}"#).unwrap();
    assert_eq!(vd.cleanup_level, CleanupLevel::Full);
    assert!(!vd.formal_register);
    assert!(vd.layout_cues);
    assert_eq!(serde_json::to_string(&CleanupLevel::Compose).unwrap(), "\"compose\"");
    for exe in ["OUTLOOK.EXE", "olk.exe", "HxOutlook.exe", "thunderbird.exe", "MailClient.exe"] {
        assert_eq!(classify(exe, "", None, true), AppContext::Email);
        assert_eq!(classify(exe, "", None, false), AppContext::General);
        assert_eq!(classify(exe, "Terminal", None, true), AppContext::Terminal);
    }
}

#[test]
fn casual_cleanup_cannot_lowercase_a_cue_boundary() {
    struct Lower;
    impl Cleaner for Lower {
        fn clean(&mut self, text: &str, _: Directive) -> Result<String, String> {
            Ok(text.to_lowercase())
        }
    }
    let mut r = recipe();
    r.directive = Directive::for_recording(CleanupLevel::Light, AppContext::General, false);
    let out = process_segment("hi, new line thanks", Some("en"), &r, Some(&mut Lower), None);
    assert_eq!(out.text, "hi,\nThanks");
}
