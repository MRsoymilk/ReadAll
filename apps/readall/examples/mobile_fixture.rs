//! Original fixtures for the host-JVM JNI smoke test. Never reads a user's books.
// Shared fixture module also contains helpers used only by other tests.
#[allow(dead_code)]
#[path = "../tests/support/epub.rs"]
mod epub;
#[path = "../tests/support/font.rs"]
mod font;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("expected an absolute fixture directory")?,
    );
    if !root.is_absolute() {
        return Err("fixture directory must be absolute".into());
    }
    fs::create_dir_all(&root)?;
    let body = format!(
        "<html><body><img src='sample.png'/>{}</body></html>",
        "<p>AAAA WWWW AAAA WWWW</p>".repeat(100)
    );
    let book = epub::make_epub_with_resources(
        &[&body, "<html><body><h1>WWWW</h1><p>AAAA</p></body></html>"],
        vec![(
            "sample.png",
            "image/png",
            epub::make_png(40, 20, [10, 90, 180, 255]),
        )],
    );
    let toc =
        epub::make_epub_with_resources(&["<html><body><p>AAAA WWWW</p></body></html>"; 30], vec![]);
    for (name, bytes) in [
        ("book.epub", book),
        ("toc.epub", toc),
        ("font.ttf", font::make_font()),
    ] {
        let path = root.join(name);
        if path.exists() {
            if fs::read(&path)? != bytes {
                return Err("refusing to overwrite a different fixture file".into());
            }
        } else {
            use std::io::Write;
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?
                .write_all(&bytes)?;
        }
    }
    println!("Original mobile test fixtures: {}", root.display());
    Ok(())
}
