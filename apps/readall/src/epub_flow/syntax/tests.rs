use super::*;
use crate::{test_epub, test_font};
use readall_epub::EpubLimits;
use readall_font::FontLimits;

fn tokens(text: &str, language: Language) -> Vec<(String, Kind)> {
    lexer::scan(text, 0, language, 65536)
        .unwrap()
        .into_iter()
        .map(|t| (text[t.range].to_owned(), t.kind))
        .collect()
}
fn has(tokens: &[(String, Kind)], text: &str, kind: Kind) -> bool {
    tokens.iter().any(|(s, k)| s == text && *k == kind)
}
fn book(source: &str) -> Vec<u8> {
    test_epub::make_epub_with_resources(&[source], vec![])
}
fn source(code: &str, attributes: &str) -> String {
    format!(
        "<html><body><p>AAAA</p><pre {attributes}>{}</pre><p id='after'>WWWW</p></body></html>",
        code.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    )
}

#[test]
fn numeric_operators_shell_multiline_and_many_lifetimes_stay_separate() {
    let t = tokens("0x1e+2 0x1.fp-3 1e+4", Language::Cpp);
    assert!(has(&t, "0x1e", Kind::Number));
    assert!(has(&t, "2", Kind::Number));
    assert!(has(&t, "0x1.fp-3", Kind::Number));
    assert!(has(&t, "1e+4", Kind::Number));
    let t = tokens(
        "echo 'first\n# still string\nlast' # comment",
        Language::Shell,
    );
    assert!(has(&t, "'first\n# still string\nlast'", Kind::String));
    assert_eq!(t.iter().filter(|(_, k)| *k == Kind::Comment).count(), 1);
    let code = "'a ".repeat(12_000);
    let t = tokens(&code, Language::Rust);
    assert_eq!(t.len(), 12_000);
    assert!(t.iter().all(|(s, k)| s == "'a" && *k == Kind::Type));
}

#[test]
fn multiline_comments_keep_the_same_token_on_every_visible_page() {
    let code = format!("/*\n{}*/\nreturn 0;", "AAAA WWWW\n".repeat(24));
    let bytes = book(&source(&code, "class='language-c'"));
    let publication = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let chapter = Chapter::load(&publication, 0).unwrap();
    let font_bytes = test_font::make_layout_font(
        *b"latn",
        *b"liga",
        false,
        &(33..127).map(|ch| (ch, 2)).collect::<Vec<_>>(),
    );
    let font = Font::parse(&font_bytes, 0, FontLimits::default()).unwrap();
    let mut options = Options::parse(
        &[
            "--font",
            "fixture.ttf",
            "--width",
            "240",
            "--height",
            "160",
            "--margin",
            "16",
            "--font-size",
            "16",
        ]
        .map(Into::into),
    )
    .unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let first = renderer.render(&chapter, &options).unwrap();
    let comment = chapter
        .syntax
        .tokens
        .iter()
        .find(|t| t.kind == Kind::Comment)
        .unwrap();
    let paper = Theme::Paper.colors().0;
    let green = palette(Kind::Comment, [paper.r, paper.g, paper.b]);
    let mut colored_pages = 0;
    for page in 0..first.pages {
        options.page = Some(page);
        let frame = renderer.render(&chapter, &options).unwrap();
        if frame
            .hits
            .iter()
            .any(|h| h.start >= comment.range.start && h.end <= comment.range.end)
        {
            assert!(frame.surface.pixels().contains(&green));
            colored_pages += 1;
        }
    }
    assert!(colored_pages > 1);
}

#[test]
fn c_code_colors_directives_types_calls_literals_and_comments_separately() {
    let code = "#define PAGE_SIZE 4096\n#define MAX_ARG_PAGES 32\n// exec.c return \"not string\"\nint do_execve() {\n    unsigned long p = 0x1FFFC;\n    p = copy_strings(argc, argv, page, p, 0);\n    return 1+2;\n}";
    assert_eq!(Language::detect(code), Some(Language::Cpp));
    let t = tokens(code, Language::Cpp);
    for (text, kind) in [
        ("#define", Kind::Directive),
        ("PAGE_SIZE", Kind::Constant),
        ("4096", Kind::Number),
        ("unsigned", Kind::Type),
        ("long", Kind::Type),
        ("do_execve", Kind::Function),
        ("copy_strings", Kind::Function),
        ("return", Kind::Keyword),
        ("0x1FFFC", Kind::Number),
        ("1", Kind::Number),
        ("2", Kind::Number),
        ("// exec.c return \"not string\"", Kind::Comment),
    ] {
        assert!(has(&t, text, kind), "{text}: {t:?}");
    }
    assert_eq!(t.iter().filter(|(_, k)| *k == Kind::Keyword).count(), 1);
}
#[test]
fn strings_comments_includes_and_cpp_raw_literals_do_not_relex_their_contents() {
    let code = "#include <stdio.h>\nconst char* url = \"https://x/\\\"/*not*/\"; /* comment\nint hidden; */ auto raw = R\"tag(//raw\n\"quoted\")tag\"; int f() { return 1.5e-3; }";
    let t = tokens(code, Language::Cpp);
    assert!(has(&t, "<stdio.h>", Kind::String));
    assert!(has(&t, "/* comment\nint hidden; */", Kind::Comment));
    assert!(has(&t, "R\"tag(//raw\n\"quoted\")tag\"", Kind::String));
    assert!(has(&t, "1.5e-3", Kind::Number));
    assert!(!t.iter().any(|(s, _)| s == "hidden" || s == "not"));
}
#[test]
fn rust_lifetimes_raw_strings_and_nested_comments_keep_state() {
    let code = "fn f<'a>(s: &'a str) { let text = r##\"// raw \"# still string\"##; /* outer /* inner */ end */ let c = 'x'; println!(\"{}\", s); }";
    let t = tokens(code, Language::Rust);
    assert!(has(&t, "'a", Kind::Type));
    assert!(has(&t, "r##\"// raw \"# still string\"##", Kind::String));
    assert!(has(&t, "/* outer /* inner */ end */", Kind::Comment));
    assert!(has(&t, "'x'", Kind::String));
    assert!(has(&t, "println", Kind::Function));
    assert!(!has(&t, "end", Kind::Keyword));
}
#[test]
fn python_shell_javascript_and_json_have_language_specific_rules() {
    let py = tokens(
        "def hello():\n    text = \"\"\"line\n# not comment\"\"\"\n    return f\"{42}\" # actual",
        Language::Python,
    );
    assert!(has(&py, "def", Kind::Keyword));
    assert!(has(&py, "hello", Kind::Function));
    assert!(has(&py, "\"\"\"line\n# not comment\"\"\"", Kind::String));
    assert!(has(&py, "f\"{42}\"", Kind::String));
    assert!(has(&py, "# actual", Kind::Comment));
    let sh = tokens(
        "#!/bin/sh\nif test -n \"$HOME\"; then\n echo ${#name} $HOME 'not # comment' # yes\nfi",
        Language::Shell,
    );
    assert!(has(&sh, "if", Kind::Keyword));
    assert!(has(&sh, "echo", Kind::Function));
    assert!(has(&sh, "${#name}", Kind::Constant));
    assert!(has(&sh, "$HOME", Kind::Constant));
    assert!(has(&sh, "'not # comment'", Kind::String));
    assert!(has(&sh, "# yes", Kind::Comment));
    let js = tokens(
        "const answer = () => { return `// ${42}\nstring`; }; // yes",
        Language::JavaScript,
    );
    assert!(has(&js, "const", Kind::Keyword));
    assert!(has(&js, "`// ${42}\nstring`", Kind::String));
    let json = tokens(
        "{\"if\":true, \"amount\":1.25, \"next\":null}",
        Language::Json,
    );
    assert!(has(&json, "\"if\"", Kind::String));
    assert!(has(&json, "true", Kind::Constant));
    assert!(has(&json, "1.25", Kind::Number));
}
#[test]
fn unmarked_detection_is_conservative_and_explicit_unknown_means_plain() {
    for prose in [
        "This is a plain text block.\nAnother line.",
        "return to the beginning",
        "日志: 4096 bytes",
        "0 1 2 3",
    ] {
        assert_eq!(Language::detect(prose), None);
    }
    for (text, language) in [
        ("#define A 1", Language::Cpp),
        ("fn main() {}", Language::Rust),
        ("def main():", Language::Python),
        ("#!/usr/bin/env bash\necho ok", Language::Shell),
        ("function main() {}", Language::JavaScript),
        ("{\"x\":true}", Language::Json),
    ] {
        assert_eq!(Language::detect(text), Some(language));
    }
    for attributes in [
        "class='language-unknown'",
        "class='nohighlight'",
        "class='language-text'",
        "class='language-plaintext'",
    ] {
        let bytes = book(&source("#define A 1", attributes));
        let publication = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
        assert!(
            Syntax::build(&publication.read_spine_content(0).unwrap())
                .unwrap()
                .tokens
                .is_empty()
        );
    }
}
#[test]
fn tokens_are_bounded_sorted_utf8_ranges_even_for_malformed_input() {
    let alphabet = [
        "汉", "é", "𐐀", "\"", "'", "/*", "*/", "\\", "\n", "r#", "#", "<", "()", "1e+2", "_", "$",
    ];
    let mut seed = 17_u64;
    for language in [
        Language::Cpp,
        Language::Rust,
        Language::Python,
        Language::Shell,
        Language::JavaScript,
        Language::Json,
    ] {
        for _ in 0..64 {
            let mut text = String::new();
            for _ in 0..128 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                text.push_str(alphabet[(seed >> 32) as usize % alphabet.len()]);
            }
            let found = lexer::scan(&text, 11, language, 65536).unwrap();
            let mut end = 11;
            for token in found {
                assert!(token.range.start >= end);
                assert!(token.range.end > token.range.start);
                assert!(
                    text.get(token.range.start - 11..token.range.end - 11)
                        .is_some()
                );
                end = token.range.end;
            }
        }
    }
    assert!(lexer::scan("int x = 1; return 2;", 0, Language::Cpp, 1).is_none());
}
#[test]
fn author_multicolor_spans_are_preserved_and_hidden_text_cannot_start_comments() {
    let bytes = book(
        "<html><body><pre class='language-c'><span style='color:blue'>int</span> f() { return 0; }</pre><pre class='language-c'>int <span hidden='hidden'>/*</span>x; return 1;</pre></body></html>",
    );
    let publication = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let content = publication.read_spine_content(0).unwrap();
    let syntax = Syntax::build(&content).unwrap();
    assert!(
        syntax
            .tokens
            .iter()
            .all(|t| t.range.start >= content.codes[1].range.start)
    );
    assert!(!syntax.tokens.iter().any(|t| t.kind == Kind::Comment));
    assert!(
        syntax
            .tokens
            .iter()
            .any(|t| &content.text[t.range.clone()] == "return" && t.kind == Kind::Keyword)
    );
}
#[test]
fn optional_highlight_budgets_fall_back_without_partial_block_tokens() {
    let bytes = book(&source("int f() { return 0; }", "class='language-c'"));
    let publication = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let content = publication.read_spine_content(0).unwrap();
    let original = content.text.clone();
    for budget in [
        Budget {
            block_bytes: 3,
            ..Budget::default()
        },
        Budget {
            chapter_bytes: 3,
            ..Budget::default()
        },
        Budget {
            tokens: 1,
            ..Budget::default()
        },
    ] {
        let result = Syntax::bounded(&content, budget).unwrap();
        assert!(result.tokens.is_empty());
        assert_eq!(result.skipped, 1);
        assert!(result.warning().is_some());
    }
    assert_eq!(content.text, original);
}
#[test]
fn palette_tracks_reader_or_author_background_without_changing_author_text_color() {
    for background in [
        [255, 255, 255],
        [250, 240, 210],
        [24, 28, 34],
        [0, 0, 0],
        [128, 128, 128],
        [30, 60, 240],
        [240, 220, 40],
    ] {
        for kind in [
            Kind::Keyword,
            Kind::Type,
            Kind::Comment,
            Kind::String,
            Kind::Number,
            Kind::Function,
            Kind::Directive,
            Kind::Constant,
        ] {
            let c = palette(kind, background);
            assert!(
                contrast(luminance(background), luminance([c.r, c.g, c.b])) >= 4.5,
                "{kind:?} {background:?}: {c:?}"
            );
        }
    }
    let syntax = Syntax {
        tokens: vec![Token {
            range: 0..3,
            kind: Kind::Type,
        }],
        skipped: 0,
    };
    let paper = syntax
        .painter(Theme::Paper)
        .color(&(0..3), TextStyle::default());
    let dark = syntax
        .painter(Theme::Dark)
        .color(&(0..3), TextStyle::default());
    assert_ne!(paper, dark);
    assert_eq!(
        syntax.painter(Theme::Dark).color(
            &(0..3),
            TextStyle {
                backdrop: Some([255, 255, 255]),
                ..TextStyle::default()
            }
        ),
        paper
    );
    assert_eq!(
        syntax.painter(Theme::Dark).color(
            &(10..13),
            TextStyle {
                color: [10, 20, 30],
                backdrop: Some([255, 255, 255]),
                ..TextStyle::default()
            }
        ),
        Color::rgba(10, 20, 30, 255)
    );
}

#[test]
fn colored_render_keeps_pages_hits_copy_offsets_and_resume_identical() {
    let code = format!(
        "#define PAGE_SIZE 4096\n{}",
        "int f() {\n    // comment\n    return 42;\n}\n".repeat(8)
    );
    let bytes = book(&source(&code, "class='language-c'"));
    let publication = EpubBook::parse(&bytes, EpubLimits::default()).unwrap();
    let mut chapter = Chapter::load(&publication, 0).unwrap();
    let font_data = test_font::make_layout_font(
        *b"latn",
        *b"liga",
        false,
        &(33..127).map(|ch| (ch, 2)).collect::<Vec<_>>(),
    );
    let font = Font::parse(&font_data, 0, FontLimits::default()).unwrap();
    let mut options = Options::parse(
        &[
            "--font",
            "fixture.ttf",
            "--width",
            "300",
            "--height",
            "240",
            "--margin",
            "16",
            "--font-size",
            "16",
        ]
        .map(Into::into),
    )
    .unwrap();
    let mut renderer = EpubRenderer::new(&font, 16, false).unwrap();
    let first = renderer.render(&chapter, &options).unwrap();
    assert!(first.pages > 1);
    for page in 0..first.pages {
        options.page = Some(page);
        let colored = renderer.render(&chapter, &options).unwrap();
        let syntax = std::mem::take(&mut chapter.syntax);
        let plain = renderer.render(&chapter, &options).unwrap();
        chapter.syntax = syntax;
        assert_eq!(colored.pages, plain.pages);
        assert_eq!(colored.locator, plain.locator);
        let hits = |frame: &RenderedPage| {
            frame
                .hits
                .iter()
                .map(|h| (h.rect, h.start, h.end))
                .collect::<Vec<_>>()
        };
        assert_eq!(hits(&colored), hits(&plain));
        assert_ne!(colored.surface.pixels(), plain.surface.pixels());
        let offset = chapter.restore(&colored.locator).unwrap();
        let locator = publication.locator(0, offset).unwrap();
        assert_eq!(publication.restore(&locator).unwrap(), (0, offset));
    }
    assert_eq!(
        &chapter.text()[chapter.content.codes[0].range.clone()],
        code
    );
    assert_eq!(publication.read_spine_text(0).unwrap(), chapter.text());
    // Palette changes on redraw even when the layout and glyph masks are reused.
    options.page = Some(0);
    let mut dark_renderer = EpubRenderer::new(&font, 16, false)
        .unwrap()
        .with_preferences(Theme::Dark, 1.0);
    let dark = dark_renderer.render(&chapter, &options).unwrap();
    let directive = palette(
        Kind::Directive,
        [
            dark.surface.pixel(0, 0).unwrap().r,
            dark.surface.pixel(0, 0).unwrap().g,
            dark.surface.pixel(0, 0).unwrap().b,
        ],
    );
    assert!(dark.surface.pixels().contains(&directive));
}
