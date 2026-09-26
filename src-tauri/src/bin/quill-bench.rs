//! `quill-bench [engineId…]`: benchmark installed engines over
//! `%LOCALAPPDATA%\Quill\Eval\*.wav` + `.txt`, writing `%TEMP%\quill\bench.json`.
//! `quill-bench --download assetId…`: fetch models through Quill's downloader.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--s1") {
        if let Err(e) = quill_lib::bench::s1_speed() {
            eprintln!("{e}");
        }
        return;
    }
    if args.first().map(String::as_str) == Some("--download") {
        if let Err(e) = quill_lib::bench::download(&args[1..]) {
            eprintln!("download failed: {e}");
            std::process::exit(1);
        }
        return;
    }
    match quill_lib::bench::run((!args.is_empty()).then_some(args)) {
        Ok(path) => println!("wrote {}", path.display()),
        Err(e) => {
            eprintln!("bench failed: {e}");
            std::process::exit(1);
        }
    }
}
