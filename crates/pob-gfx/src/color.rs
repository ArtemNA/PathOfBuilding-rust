//! Colours of the SimpleGraphic host: `SetDrawColor` tints and the `^N` / `^xRRGGBB` escape palette.
//!
//! Palette measured by the SG oracle (UI-ARCH §11, scene `palette`): `^0..^9` = (0,0,0) (1,0,0) (0,1,0) (0,0,1) (1,1,0)
//! **(1,0,1)** (0,1,1) (1,1,1) (0.7,0.7,0.7) (0.4,0.4,0.4); an escape sets r,g,b and **resets alpha to 1**.

use crate::text::escape_len;

/// RGBA tint, components 0..1 (`SetDrawColor`).
pub type Rgba = [f32; 4];

pub const WHITE: Rgba = [1.0, 1.0, 1.0, 1.0];

/// `^0`…`^9` (UI-ARCH §11; 178/255 and 102/255 in screenshots = 0.7 and 0.4).
pub const PALETTE: [[f32; 3]; 10] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [1.0, 1.0, 0.0],
    [1.0, 0.0, 1.0],
    [0.0, 1.0, 1.0],
    [1.0, 1.0, 1.0],
    [0.7, 0.7, 0.7],
    [0.4, 0.4, 0.4],
];

fn hex(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => unreachable!("escape_len checked hex digits"),
    }
}

/// The escape at `bytes[at]`: its byte length and colour (alpha 1), or `None` when there is no escape.
pub fn escape_color(bytes: &[u8], at: usize) -> Option<(usize, Rgba)> {
    let len = escape_len(bytes, at);
    let rgb = match len {
        2 => PALETTE[usize::from(bytes[at + 1] - b'0')],
        8 => {
            let c = |k: usize| f32::from(hex(bytes[at + 2 + 2 * k]) * 16 + hex(bytes[at + 3 + 2 * k])) / 255.0;
            [c(0), c(1), c(2)]
        }
        _ => return None,
    };
    Some((len, [rgb[0], rgb[1], rgb[2], 1.0]))
}

/// `SetDrawColor(escapeStr)`: the colour of the escape at the start of `s` ("invalid color escape sequence" otherwise).
/// Trailing text after the escape is ignored [H: not measured].
pub fn parse_escape(s: &str) -> Option<Rgba> {
    escape_color(s.as_bytes(), 0).map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_parse() {
        assert_eq!(parse_escape("^5"), Some([1.0, 0.0, 1.0, 1.0]));
        assert_eq!(parse_escape("^xFF8000"), Some([1.0, 128.0 / 255.0, 0.0, 1.0]));
        assert_eq!(parse_escape("^x00ff7F"), Some([0.0, 1.0, 127.0 / 255.0, 1.0]));
        assert_eq!(parse_escape("bad"), None);
        assert_eq!(parse_escape("^x12345"), None);
    }
}
