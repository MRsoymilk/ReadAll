use super::*;
fn styled(source: &str, css: &str) -> ExtractedText {
    let mut sheet = StyleSheet::default();
    sheet.append(css).unwrap();
    extract_styled(source.as_bytes(), 128 * 1024, Some(&mut sheet)).unwrap()
}

#[test]
fn execve_code_preserves_newlines_indentation_blank_lines_and_inline_spans() {
    let code = "#define PAGE_SIZE 4096\n#define MAX_ARG_PAGES 32\n\n// exec.c\nint do_execve(...) {\n\t// p = 0x1FFFC = 128K - 4\n    unsigned long p = PAGE_SIZE * MAX_ARG_PAGES - 4;\n    p = copy_strings(envc,envp,page,p,0);\n    p = copy_strings(argc,argv,page,p,0);\n    eip[3] = p;\n}";
    let source = format!(
        "<html><body><p>准备参数空间</p><pre><code>{}</code></pre><p id='after'>计算轨迹</p></body></html>",
        code.replace("unsigned", "<span class='keyword'>unsigned</span>")
    );
    let rich = styled(&source, ".keyword {color:blue}");
    assert_eq!(rich.text, format!("准备参数空间\n\n{code}\n\n计算轨迹"));
    assert_eq!(rich.anchor("after"), rich.text.find("计算轨迹"));
    assert!(
        rich.legacy_text
            .as_ref()
            .unwrap()
            .contains("#define PAGE_SIZE 4096 #define MAX_ARG_PAGES 32")
    );
    let at = rich.text.find("unsigned").unwrap();
    let run = rich.runs.iter().find(|r| r.range.contains(&at)).unwrap();
    assert_eq!(run.style.color, [0, 0, 255]);
    assert!(run.style.white_space.preserves_spaces());
}

#[test]
fn css_whitespace_inherits_and_normal_code_remains_inline() {
    let rich = styled(
        "<html><body><div class='source'>A\n  <span>W</span>\n\n\tA  </div><p>normal\n text <code>A\n W</code></p></body></html>",
        ".source {white-space:pre-wrap}",
    );
    assert_eq!(rich.text, "A\n  W\n\n\tA  \n\nnormal text A W");
    let collapsed = styled(
        "<html><body><pre style='white-space:normal'>  A\n W\t A </pre></body></html>",
        "",
    );
    assert_eq!(collapsed.text, "A W A");
    let line = styled(
        "<html><body><p style='white-space:pre-line'>  A  W \r\n \t A\n\n W  </p></body></html>",
        "",
    );
    assert_eq!(line.text, "A W\nA\n\nW");
}

#[test]
fn preserved_tabs_end_spaces_crlf_cdata_and_repeated_br_are_not_trimmed() {
    for mode in ["pre", "pre-wrap", "break-spaces"] {
        let source = format!(
            "<html><body><pre style='white-space:{mode}'>\n  A\r\n\t<span>W</span>\r<![CDATA[\nA]]>  \n</pre></body></html>"
        );
        let rich = styled(&source, "");
        assert_eq!(rich.text, "\n  A\n\tW\nA  \n", "{mode}");
    }
    assert_eq!(
        styled("<html><body><p>A<br/><br/>W</p></body></html>", "").text,
        "A\n\nW"
    );
    assert_eq!(
        styled("<html><body><pre>A<br/><br/>  W</pre></body></html>", "").text,
        "A\n\n  W"
    );
}

#[test]
fn unchanged_prose_retains_legacy_text_and_offsets() {
    let source = "<html><body><p>A\n <span>W</span>  A</p><p id='last'>W</p></body></html>";
    let legacy = extract_with_anchors(source.as_bytes(), 4096).unwrap();
    let rich = styled(source, "");
    assert_eq!(rich.text, legacy.text);
    assert_eq!(rich.anchor("last"), legacy.anchor("last"));
    assert!(rich.legacy_text.is_none());
}
