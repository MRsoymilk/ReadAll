//! Tiny dependency-free UI drawing helpers used before full text shaping is available.
use readall_render::{Color, DrawCommand, Rect, Surface};
use std::error::Error;

pub(crate) type UiResult<T> = Result<T, Box<dyn Error>>;

pub(crate) fn draw_text(
    surface: &mut Surface,
    x: i32,
    y: i32,
    scale: u32,
    text: &str,
    color: Color,
) -> UiResult<()> {
    let mut commands = Vec::new();
    let mut cursor = x;
    for ch in text.chars() {
        if ch == ' ' {
            cursor += (6 * scale) as i32;
            continue;
        }
        let glyph = glyph(ch).unwrap_or_else(|| glyph('?').expect("question mark glyph"));
        for (row, bits) in glyph.into_iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    commands.push(DrawCommand::FillRect {
                        rect: Rect::new(
                            cursor + (col * scale) as i32,
                            y + (row as u32 * scale) as i32,
                            scale,
                            scale,
                        ),
                        color,
                    });
                }
            }
        }
        cursor += (6 * scale) as i32;
    }
    surface.draw(&commands)?;
    Ok(())
}

pub(crate) fn text_width(scale: u32, text: &str) -> u32 {
    text.chars().count() as u32 * 6 * scale
}

pub(crate) fn display_ascii(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in text.chars().take(max_chars) {
        if ch.is_ascii_graphic() || ch == ' ' {
            out.push(ch);
        } else {
            out.push('?');
        }
    }
    if text.chars().count() > max_chars && max_chars >= 3 {
        for _ in 0..3 {
            out.pop();
        }
        out.push_str("...");
    }
    out
}

pub(crate) fn glyph(ch: char) -> Option<[u8; 7]> {
    Some(match ch.to_ascii_uppercase() {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [31, 4, 4, 4, 4, 4, 31],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        ':' => [0, 12, 12, 0, 12, 12, 0],
        '/' => [1, 2, 2, 4, 8, 8, 16],
        '<' => [2, 4, 8, 16, 8, 4, 2],
        '>' => [8, 4, 2, 1, 2, 4, 8],
        '?' => [14, 17, 1, 2, 4, 0, 4],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_ascii_is_bounded_and_visible() {
        assert_eq!(display_ascii("book.epub", 20), "book.epub");
        assert_eq!(display_ascii("中文.epub", 20), "??.epub");
        assert_eq!(display_ascii("abcdefghijkl", 8), "abcde...");
    }

    #[test]
    fn home_alphabet_has_required_punctuation() {
        for ch in "READALL OPEN EPUB TXT FILE BROWSER ../BOOK.EPUB < > ?:".chars() {
            if ch != ' ' {
                assert!(glyph(ch).is_some(), "missing {ch}");
            }
        }
    }
}
