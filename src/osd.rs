//! On-screen messages for hotkeys: text drawn with a built-in 5x7 pixel font
//! into a small bitmap that the display shader puts in the top-left corner.

/// The bitmap is this many texels wide at most; longer messages are cut.
pub const WIDTH: u32 = 512;
/// One row of text: 7 glyph rows plus a texel of outline above and below.
pub const HEIGHT: u32 = 9;
/// Texel values: glyph, and the dark outline that keeps it readable on any colour.
pub const INK: u8 = 255;
pub const OUTLINE: u8 = 128;

/// Each glyph is 7 rows of 5 bits, the leftmost pixel in the highest bit.
#[rustfmt::skip]
const FONT: &[(char, [u8; 7])] = &[
    ('A', [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
    ('B', [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110]),
    ('C', [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110]),
    ('D', [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110]),
    ('E', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111]),
    ('F', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000]),
    ('G', [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111]),
    ('H', [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
    ('I', [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
    ('J', [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100]),
    ('K', [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001]),
    ('L', [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111]),
    ('M', [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001]),
    ('N', [0b10001, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001]),
    ('O', [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('P', [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000]),
    ('Q', [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101]),
    ('R', [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001]),
    ('S', [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110]),
    ('T', [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100]),
    ('U', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('V', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100]),
    ('W', [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010]),
    ('X', [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001]),
    ('Y', [0b10001, 0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100]),
    ('Z', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111]),
    ('0', [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110]),
    ('1', [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
    ('2', [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111]),
    ('3', [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110]),
    ('4', [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010]),
    ('5', [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110]),
    ('6', [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110]),
    ('7', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000]),
    ('8', [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110]),
    ('9', [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100]),
    (':', [0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000]),
    ('.', [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100]),
    (',', [0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b00100, 0b01000]),
    ('-', [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000]),
    ('+', [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000]),
    ('/', [0b00000, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b00000]),
    ('(', [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010]),
    (')', [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000]),
    ('%', [0b11000, 0b11001, 0b00010, 0b00100, 0b01000, 0b10011, 0b00011]),
    ('\'', [0b01100, 0b00100, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000]),
];

/// Horizontal texels per character: 5 for the glyph, 1 gap.
const ADVANCE: u32 = 6;

/// Draws `text` in capitals, with a one-texel outline. Returns the bitmap
/// (WIDTH x HEIGHT, row by row) and how many texels wide the text is.
pub fn render(text: &str) -> (Vec<u8>, u32) {
    let mut ink = vec![false; (WIDTH * HEIGHT) as usize];
    let max_chars = ((WIDTH - 2) / ADVANCE) as usize;
    let mut chars = 0;
    for (i, c) in text.chars().flat_map(char::to_uppercase).take(max_chars).enumerate() {
        chars = i + 1;
        let Some((_, rows)) = FONT.iter().find(|(f, _)| *f == c) else { continue };
        for (y, bits) in rows.iter().enumerate() {
            for x in 0..5 {
                if bits & (0b10000 >> x) != 0 {
                    let (px, py) = (1 + i as u32 * ADVANCE + x, 1 + y as u32);
                    ink[(py * WIDTH + px) as usize] = true;
                }
            }
        }
    }
    let width = (chars as u32 * ADVANCE + 1).min(WIDTH);
    let at = |x: i32, y: i32| {
        (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y) && ink[(y as u32 * WIDTH + x as u32) as usize]
    };
    let mut out = vec![0u8; ink.len()];
    for y in 0..HEIGHT as i32 {
        for x in 0..WIDTH as i32 {
            out[(y as u32 * WIDTH + x as u32) as usize] = if at(x, y) {
                INK
            } else if (-1..=1).any(|dy| (-1..=1).any(|dx| at(x + dx, y + dy))) {
                OUTLINE
            } else {
                0
            };
        }
    }
    (out, width)
}
