use super::*;
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    let at = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(bytes);
    out.extend_from_slice(&crc32(&out[at..]).to_be_bytes());
}
fn image(width: usize, height: usize, depth: usize, kind: u8) -> (Vec<u8>, Vec<u8>) {
    let channels = match kind {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => panic!("kind"),
    };
    let maximum = (1_u32 << depth) - 1;
    let value = |x: usize, y: usize, c: usize| ((x * 37 + y * 19 + c * 53) as u32 & maximum) as u16;
    let palette: Vec<u8> = (0..1_usize << depth.min(8))
        .flat_map(|i| [(i * 31) as u8, (i * 73) as u8, (i * 117) as u8])
        .collect();
    let scale = |v: u16| ((u32::from(v) * 255 + maximum / 2) / maximum) as u8;
    let mut expected = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let p = match kind {
                0 => {
                    let v = scale(value(x, y, 0));
                    [v, v, v, 255]
                }
                2 => [
                    scale(value(x, y, 0)),
                    scale(value(x, y, 1)),
                    scale(value(x, y, 2)),
                    255,
                ],
                3 => {
                    let i = value(x, y, 0) as usize;
                    [
                        palette[i * 3],
                        palette[i * 3 + 1],
                        palette[i * 3 + 2],
                        if i == 0 { 0 } else { 255 },
                    ]
                }
                4 => {
                    let v = scale(value(x, y, 0));
                    [v, v, v, scale(value(x, y, 1))]
                }
                6 => [
                    scale(value(x, y, 0)),
                    scale(value(x, y, 1)),
                    scale(value(x, y, 2)),
                    scale(value(x, y, 3)),
                ],
                _ => unreachable!(),
            };
            expected.extend_from_slice(&p);
        }
    }
    let mut raw = Vec::new();
    let bpp = (channels * depth).div_ceil(8);
    for (pass, (x0, y0, dx, dy)) in [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ]
    .into_iter()
    .enumerate()
    {
        let xs: Vec<_> = (x0..width).step_by(dx).collect();
        if xs.is_empty() {
            continue;
        }
        let mut previous = Vec::new();
        for (py, y) in (y0..height).step_by(dy).enumerate() {
            let mut row = vec![0; (xs.len() * channels * depth).div_ceil(8)];
            for (px, x) in xs.iter().enumerate() {
                for c in 0..channels {
                    let sample = value(*x, y, c);
                    let i = px * channels + c;
                    if depth == 16 {
                        row[i * 2..i * 2 + 2].copy_from_slice(&sample.to_be_bytes());
                    } else if depth == 8 {
                        row[i] = sample as u8;
                    } else {
                        row[i * depth / 8] |= (sample as u8) << (8 - depth - i * depth % 8);
                    }
                }
            }
            let filter = ((pass + py) % 5) as u8;
            raw.push(filter);
            for i in 0..row.len() {
                let a = if i >= bpp { row[i - bpp] } else { 0 };
                let b = previous.get(i).copied().unwrap_or(0);
                let c = if i >= bpp {
                    previous.get(i - bpp).copied().unwrap_or(0)
                } else {
                    0
                };
                let prediction = match filter {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                    _ => paeth(a, b, c),
                };
                raw.push(row[i].wrapping_sub(prediction));
            }
            previous = row;
        }
    }
    let mut z = vec![0x78, 1, 1];
    let n = raw.len() as u16;
    z.extend_from_slice(&n.to_le_bytes());
    z.extend_from_slice(&(!n).to_le_bytes());
    z.extend_from_slice(&raw);
    let (mut a, mut b) = (1_u32, 0_u32);
    for v in raw {
        a = (a + u32::from(v)) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&(b << 16 | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut h = Vec::new();
    h.extend_from_slice(&(width as u32).to_be_bytes());
    h.extend_from_slice(&(height as u32).to_be_bytes());
    h.extend_from_slice(&[depth as u8, kind, 0, 0, 1]);
    chunk(&mut out, b"IHDR", &h);
    if kind == 3 {
        chunk(&mut out, b"PLTE", &palette);
        chunk(&mut out, b"tRNS", &[0]);
    }
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    (out, expected)
}
#[test]
fn seven_passes_restore_every_pixel_with_all_filters_and_legal_color_depths() {
    for (kind, depths) in [
        (0, &[1, 2, 4, 8, 16][..]),
        (2, &[8, 16]),
        (3, &[1, 2, 4, 8]),
        (4, &[8, 16]),
        (6, &[8, 16]),
    ] {
        for &depth in depths {
            for (width, height) in [(1, 1), (1, 9), (11, 1), (13, 11)] {
                let (bytes, expected) = image(width, height, depth, kind);
                let decoded = decode_png(&bytes, ImageLimits::default()).unwrap();
                assert_eq!(
                    decoded.pixels(),
                    expected,
                    "kind={kind} depth={depth} size={width}x{height}"
                );
            }
        }
    }
}
#[test]
fn interlaced_inputs_still_enforce_checksums_and_output_limits() {
    let (mut bytes, _) = image(13, 11, 8, 6);
    assert!(
        decode_png(
            &bytes,
            ImageLimits {
                max_pixels: 142,
                ..ImageLimits::default()
            }
        )
        .is_err()
    );
    let at = bytes.len() - 13;
    bytes[at] ^= 1;
    assert!(decode_png(&bytes, ImageLimits::default()).is_err());
}
