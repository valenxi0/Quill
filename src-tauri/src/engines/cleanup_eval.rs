//! Opt in with QUILL_CLEANUP_EVAL=1 and --ignored. Uses installed S1-mini weights.
use super::{s1::S1Mini, TextCleaner};
use quill_core::text::{
    directive::{Context, Directive, Structure, Styling},
    guard,
    numbers::default_itn,
};
use regex::RegexBuilder;
use serde::Deserialize;
use std::time::Instant;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    id: String,
    transcript: String,
    styling: String,
    structure: String,
    context: String,
    kept: Vec<String>,
    gone: Vec<String>,
    matches: Vec<String>,
    #[serde(default)]
    absent: Vec<String>,
    guard_expected: bool,
}

#[test]
#[ignore = "requires QUILL_CLEANUP_EVAL=1 and installed S1-mini"]
fn real_model_cleanup_eval() {
    if std::env::var("QUILL_CLEANUP_EVAL").as_deref() != Ok("1") {
        eprintln!("SKIPPED: set QUILL_CLEANUP_EVAL=1");
        return;
    }
    let path = crate::paths::models_dir().join("s1Mini/s1-mini-q4_k_m.gguf");
    if !path.is_file() {
        eprintln!("SKIPPED: S1-mini is not installed at {}", path.display());
        return;
    }
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!("cleanup-fixtures.json")).unwrap();
    let mut cleaner = S1Mini::new(path.clone(), crate::platform::system::inference_threads());
    cleaner.load().expect("load installed S1-mini");
    let itn = default_itn();
    let mut reports = Vec::new();
    let mut failed = 0;
    for f in fixtures {
        let directive = Directive {
            styling: match f.styling.as_str() {
                "casual" => Styling::Casual,
                "semi-formal" => Styling::SemiFormal,
                "formal" => Styling::Formal,
                _ => panic!("untrained styling"),
            },
            structure: match f.structure.as_str() {
                "prose" => Structure::Prose,
                "lists" => Structure::Lists,
                _ => panic!("untrained structure"),
            },
            context: match f.context.as_str() {
                "general" => Context::General,
                "email" => Context::Email,
                _ => panic!("untrained context"),
            },
        };
        let start = Instant::now();
        let result = cleaner.clean(&f.transcript, directive);
        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        let mut misses = Vec::new();
        let output = match result {
            Ok(s) => s,
            Err(e) => {
                misses.push(format!("cleaner: {e}"));
                String::new()
            }
        };
        let reason = guard::reason(&f.transcript, &output, itn.as_deref());
        let (delivered, restored) = guard::guard_with_itn(&f.transcript, &output, itn.as_deref());
        let lower = delivered.to_lowercase();
        for kept in f.kept {
            if !lower.contains(&kept.to_lowercase()) {
                misses.push(format!("missing substring: {kept}"));
            }
        }
        for gone in f.gone {
            let pattern = format!(r"\b{}\b", regex::escape(&gone));
            if RegexBuilder::new(&pattern).case_insensitive(true).build().unwrap().is_match(delivered) {
                misses.push(format!("retained words: {gone}"));
            }
        }
        for pattern in f.matches {
            if !RegexBuilder::new(&pattern).case_insensitive(true).multi_line(true).build().unwrap().is_match(delivered)
            {
                misses.push(format!("regex missed: {pattern}"));
            }
        }
        for absent in f.absent {
            if lower.contains(&absent.to_lowercase()) {
                misses.push(format!("unexpected substring: {absent}"));
            }
        }
        if restored != f.guard_expected {
            misses.push(format!("guard: {reason:?}"));
        }
        if !misses.is_empty() {
            failed += 1;
        }
        println!(
            "{}: {:.0} ms, {}\nOUTPUT: {}\nMISSES: {:?}",
            f.id,
            elapsed_ms,
            if misses.is_empty() { "PASS" } else { "FAIL" },
            output,
            misses
        );
        reports.push(serde_json::json!({ "id": f.id, "transcript": f.transcript, "directive": directive.control_line(), "output": output, "delivered": delivered, "elapsedMs": elapsed_ms, "guardFired": restored, "guardReason": reason.map(|r| format!("{r:?}")), "misses": misses }));
    }
    let report = std::env::var_os("QUILL_CLEANUP_REPORT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::paths::eval_dir().join("cleanup-report.json"));
    if let Some(parent) = report.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(
        &report,
        serde_json::to_string_pretty(
            &serde_json::json!({ "model": path, "fixtures": reports, "failed": failed, "total": reports.len() }),
        )
        .unwrap(),
    )
    .unwrap();
    println!("Report: {}", report.display());
    assert_eq!(failed, 0, "real S1-mini rubric failures; see {}", report.display());
}
