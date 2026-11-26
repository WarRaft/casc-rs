//! `casc-rs <game folder> [list [text]] | [read <name> [file]]`: the
//! build and how many files; the names (those with `text` in them, any
//! case), each with `*` when its data is on disk; a file's bytes, to
//! `file` or its size said.

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(folder) = args.first() else {
        eprintln!("casc-rs <game folder> [list [text]] | [read <name> [file]]");
        std::process::exit(2);
    };
    let s = match casc_rs::Storage::open(Path::new(folder)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    match args.get(1).map(String::as_str) {
        Some("list") => {
            let text = args.get(2).map(|t| t.to_ascii_lowercase());
            for e in &s.entries {
                if text.as_ref().is_none_or(|t| e.name.to_ascii_lowercase().contains(t)) {
                    println!("{}{}", if s.has(&e.name) { "* " } else { "  " }, e.name);
                }
            }
        }
        Some("read") => {
            let Some(name) = args.get(2) else {
                eprintln!("read: which file?");
                std::process::exit(2);
            };
            match s.read(name) {
                Ok(b) => match args.get(3) {
                    Some(out) => std::fs::write(out, b).unwrap_or_else(|e| eprintln!("{out}: {e}")),
                    None => println!("{name}: {} bytes", b.len()),
                },
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
        _ => println!("{}: build {}, {} files", folder, s.build, s.entries.len()),
    }
}
