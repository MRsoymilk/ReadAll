//! Exercise preserved-code text and legacy position/annotation migration through
//! the shipped CLI, using isolated fixtures instead of a user's EPUB or state.
#[allow(dead_code)]
#[path = "support/epub.rs"]
mod epub;
use readall_epub::{EpubBook, EpubLimits, EpubLocator};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "readall-code-whitespace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(args: &[&str]) -> String {
    let result = Command::new(env!("CARGO_BIN_EXE_readall"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).unwrap()
}
const CODE: &str = "A\n\tW\n\n    A";
const LEGACY: &str = "AAAA\n\nA W A\n\nWWWW";
fn data() -> Vec<u8> {
    epub::make_epub_with_resources(
        &[
            "<html><body><p>AAAA</p><pre>A\n\tW\n\n    A</pre><p id='after'>WWWW</p><img src='a.png'/></body></html>",
            "<html><body>AAAA WWWW</body></html>",
        ],
        vec![(
            "a.png",
            "image/png",
            epub::make_png(8, 8, [20, 40, 60, 255]),
        )],
    )
}
#[test]
fn v3_text_image_and_fragment_anchors_migrate_without_changing_plain_chapters() {
    let bytes = data();
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let text = book.read_spine_text(0).unwrap();
    assert_eq!(text, format!("AAAA\n\n{CODE}\n\nWWWW"));
    let old: EpubLocator = format!("epub-v1:{}:0:{}", book.id(), LEGACY.find("WWWW").unwrap())
        .parse()
        .unwrap();
    assert_eq!(book.restore(&old).unwrap(), (0, text.find("WWWW").unwrap()));
    let upgraded = book.normalize_locator(&old).unwrap();
    assert!(upgraded.to_string().starts_with("epub-v3:"));
    assert_eq!(
        upgraded.to_string().parse::<EpubLocator>().unwrap(),
        upgraded
    );
    assert_eq!(
        book.locator_for_fragment(0, "after").unwrap().unwrap(),
        upgraded
    );
    let image: EpubLocator = format!("epub-v2:{}:0:{}:0", book.id(), LEGACY.len())
        .parse()
        .unwrap();
    let current_image = book.normalize_locator(&image).unwrap();
    assert_eq!(current_image, book.image_locator(0, 0).unwrap());
    assert_eq!(current_image.image_index(), Some(0));
    assert!(current_image.to_string().ends_with(":0"));
    assert!(
        book.locator(1, 2)
            .unwrap()
            .to_string()
            .starts_with("epub-v1:")
    );
    for bad in [
        format!("epub-v3:{}:0:2", book.id()),
        format!("epub-v3:{}:0:2:bad", book.id()),
        format!("epub-v3:{}:0:2:-:extra", book.id()),
    ] {
        assert!(bad.parse::<EpubLocator>().is_err());
    }
    assert!(
        book.restore(&format!("epub-v3:{}:0:999999:-", book.id()).parse().unwrap())
            .is_err()
    );
}
#[test]
fn legacy_annotation_ends_are_migrated_in_memory_and_saved_only_on_explicit_edit() {
    let tmp = Temp::new();
    let bytes = data();
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let path = tmp.0.join("book.epub");
    fs::write(&path, &bytes).unwrap();
    let state = tmp.0.join("state");
    fs::create_dir(&state).unwrap();
    let old_start = LEGACY.find("A W A").unwrap();
    let old_end = old_start + 5;
    let locator = format!("epub-v1:{}:0:{old_start}", book.id());
    let file = state.join(format!("{}.annotations", book.id()));
    let original =
        format!("readall-annotations-v1\n1|highlight|{locator}|{old_end}|6c6567616379\n");
    fs::write(&file, &original).unwrap();
    let result = run(&[
        "annotations",
        path.to_str().unwrap(),
        "--data-dir",
        state.to_str().unwrap(),
    ]);
    let fields: Vec<_> = result.lines().next().unwrap().split('\t').collect();
    let current: EpubLocator = fields[2].parse().unwrap();
    let end: usize = fields[3].parse().unwrap();
    let text = book.read_spine_text(0).unwrap();
    assert_eq!(&text[current.utf8_offset() as usize..end], CODE);
    assert!(fields[2].starts_with("epub-v3:"));
    assert_eq!(fs::read_to_string(&file).unwrap(), original);
    run(&[
        "note",
        path.to_str().unwrap(),
        fields[2],
        "new note",
        "--data-dir",
        state.to_str().unwrap(),
    ]);
    let saved = fs::read_to_string(&file).unwrap();
    assert!(saved.contains("epub-v3:"));
    assert!(!saved.contains("epub-v1:"));
    assert_eq!(fs::read(&path).unwrap(), bytes);
}
#[test]
fn external_css_code_text_export_and_search_share_preserved_offsets() {
    let source = "<html><head><link rel='stylesheet' href='styles/code.css'/></head><body><div class='source'><code>#define PAGE_SIZE 4096\n#define MAX_ARG_PAGES 32\n\nint do_execve() {\n    return 0;\n}</code></div></body></html>";
    let bytes = epub::make_epub_with_resources(
        &[source],
        vec![(
            "styles/code.css",
            "text/css",
            b".source {white-space:pre-wrap}".to_vec(),
        )],
    );
    let book = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let text = book.read_spine_text(0).unwrap();
    assert!(text.contains("4096\n#define"));
    assert!(text.contains("{\n    return 0;\n}"));
    let report = book
        .search("return 0;", true, readall_epub::SearchLimits::default())
        .unwrap();
    assert_eq!(report.hits.len(), 1);
    let hit = &report.hits[0];
    assert_eq!(
        &text[hit.locator.utf8_offset() as usize..hit.end_offset],
        "return 0;"
    );
    assert!(hit.locator.to_string().starts_with("epub-v3:"));
    let tmp = Temp::new();
    let path = tmp.0.join("code.epub");
    fs::write(&path, &bytes).unwrap();
    assert!(run(&["epub-text", path.to_str().unwrap()]).contains(&text));
    assert_eq!(fs::read(&path).unwrap(), bytes);
}
