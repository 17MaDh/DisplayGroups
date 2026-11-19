//! Minimal EDID 1.x base-block parser: just the fields used for identification.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edid {
    /// Three-letter PnP manufacturer id, e.g. `SAM`.
    pub pnp_id: String,
    /// Product code as four upper-case hex digits, e.g. `721B`.
    pub product_code: String,
    /// Numeric serial from bytes 12..16 (0 when unused).
    pub serial_number: u32,
    /// Serial string from a `0xFF` descriptor, if present.
    pub serial_text: Option<String>,
    /// Monitor name from a `0xFC` descriptor, if present.
    pub name: Option<String>,
    pub week: u8,
    pub year: u16,
    /// Some monitors ship with a wrong checksum; we still use the data.
    pub checksum_ok: bool,
}

impl Edid {
    /// Best serial: the descriptor string, else the numeric serial if it looks real.
    pub fn serial(&self) -> Option<String> {
        if let Some(s) = self.serial_text.as_deref().filter(|s| !s.is_empty()) {
            return Some(s.to_string());
        }
        // 0 means "not used"; 0x01010101 is a common placeholder.
        match self.serial_number {
            0 | 0x0101_0101 | u32::MAX => None,
            n => Some(n.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EdidError {
    #[error("EDID is shorter than 128 bytes ({0})")]
    TooShort(usize),
    #[error("EDID header is missing")]
    BadHeader,
}

const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
const DESCRIPTOR_OFFSETS: [usize; 4] = [54, 72, 90, 108];

pub fn parse_edid(bytes: &[u8]) -> Result<Edid, EdidError> {
    if bytes.len() < 128 {
        return Err(EdidError::TooShort(bytes.len()));
    }
    let b = &bytes[..128];
    if b[..8] != HEADER {
        return Err(EdidError::BadHeader);
    }
    let checksum_ok = b.iter().fold(0u8, |acc, x| acc.wrapping_add(*x)) == 0;

    let mut serial_text = None;
    let mut name = None;
    for off in DESCRIPTOR_OFFSETS {
        let d = &b[off..off + 18];
        // Display descriptors start with 00 00 00 <tag>.
        if d[0] == 0 && d[1] == 0 && d[2] == 0 {
            match d[3] {
                0xFF => serial_text = descriptor_text(&d[5..18]),
                0xFC => name = descriptor_text(&d[5..18]),
                _ => {}
            }
        }
    }

    Ok(Edid {
        pnp_id: decode_pnp(u16::from_be_bytes([b[8], b[9]])),
        product_code: format!("{:04X}", u16::from_le_bytes([b[10], b[11]])),
        serial_number: u32::from_le_bytes([b[12], b[13], b[14], b[15]]),
        serial_text,
        name,
        week: b[16],
        year: 1990 + u16::from(b[17]),
        checksum_ok,
    })
}

/// Descriptor strings are up to 13 ASCII bytes, ended by 0x0A and padded with spaces.
fn descriptor_text(raw: &[u8]) -> Option<String> {
    let text: String = raw
        .iter()
        .take_while(|&&c| c != 0x0A && c != 0x00)
        .map(|&c| {
            if c.is_ascii_graphic() || c == b' ' {
                c as char
            } else {
                '?'
            }
        })
        .collect();
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Decodes the big-endian packed manufacturer id (three 5-bit letters).
pub fn decode_pnp(v: u16) -> String {
    [(v >> 10) & 0x1F, (v >> 5) & 0x1F, v & 0x1F]
        .iter()
        .map(|&c| {
            if (1..=26).contains(&c) {
                char::from(b'A' + c as u8 - 1)
            } else {
                '?'
            }
        })
        .collect()
}

/// `DISPLAYCONFIG_TARGET_DEVICE_NAME::edidManufactureId` holds the EDID bytes 8..10 read
/// as a little-endian integer, so the bytes must be swapped before decoding.
pub fn pnp_from_ccd(raw: u16) -> String {
    decode_pnp(raw.swap_bytes())
}

pub fn product_from_ccd(raw: u16) -> String {
    format!("{raw:04X}")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds a valid EDID base block with the given ids and descriptors.
    pub(crate) fn make_edid(
        pnp: &str,
        product: u16,
        serial: u32,
        name: Option<&str>,
        serial_text: Option<&str>,
    ) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[..8].copy_from_slice(&HEADER);
        let c: Vec<u16> = pnp.bytes().map(|x| u16::from(x - b'A' + 1)).collect();
        let packed = (c[0] << 10) | (c[1] << 5) | c[2];
        b[8..10].copy_from_slice(&packed.to_be_bytes());
        b[10..12].copy_from_slice(&product.to_le_bytes());
        b[12..16].copy_from_slice(&serial.to_le_bytes());
        b[16] = 12;
        b[17] = 32; // 2022
        let mut put = |slot: usize, tag: u8, text: &str| {
            let off = DESCRIPTOR_OFFSETS[slot];
            b[off + 3] = tag;
            let mut field = [b' '; 13];
            let bytes = text.as_bytes();
            field[..bytes.len()].copy_from_slice(bytes);
            if bytes.len() < 13 {
                field[bytes.len()] = 0x0A;
            }
            b[off + 5..off + 18].copy_from_slice(&field);
        };
        if let Some(n) = name {
            put(1, 0xFC, n);
        }
        if let Some(s) = serial_text {
            put(2, 0xFF, s);
        }
        let sum = b[..127].iter().fold(0u8, |a, x| a.wrapping_add(*x));
        b[127] = 0u8.wrapping_sub(sum);
        b
    }

    #[test]
    fn parses_ids_descriptors_and_checksum() {
        let raw = make_edid("SAM", 0x721B, 0, Some("SAMSUNG"), Some("H4ZR900123"));
        let e = parse_edid(&raw).unwrap();
        assert_eq!(e.pnp_id, "SAM");
        assert_eq!(e.product_code, "721B");
        assert_eq!(e.name.as_deref(), Some("SAMSUNG"));
        assert_eq!(e.serial().as_deref(), Some("H4ZR900123"));
        assert_eq!(e.year, 2022);
        assert!(e.checksum_ok);
    }

    #[test]
    fn numeric_serial_used_when_no_descriptor_and_placeholders_ignored() {
        let e = parse_edid(&make_edid("AUS", 0xAA06, 376_965_120, None, None)).unwrap();
        assert_eq!(e.serial().as_deref(), Some("376965120"));
        let e = parse_edid(&make_edid("AUS", 0xAA06, 0x0101_0101, None, None)).unwrap();
        assert_eq!(e.serial(), None);
    }

    #[test]
    fn bad_input_rejected_but_bad_checksum_tolerated() {
        assert_eq!(parse_edid(&[0; 10]), Err(EdidError::TooShort(10)));
        assert_eq!(parse_edid(&[1; 128]), Err(EdidError::BadHeader));
        let mut raw = make_edid("SAM", 1, 0, None, None);
        raw[127] ^= 0xFF;
        assert!(!parse_edid(&raw).unwrap().checksum_ok);
    }

    #[test]
    fn ccd_ids_from_the_test_machine_decode() {
        // Values read with DisplayConfigGetDeviceInfo on the real 3-display rig.
        assert_eq!(pnp_from_ccd(11596), "SAM");
        assert_eq!(pnp_from_ccd(45830), "AUS");
        assert_eq!(pnp_from_ccd(3363), "HXM");
        assert_eq!(product_from_ccd(29211), "721B");
        assert_eq!(product_from_ccd(43526), "AA06");
        assert_eq!(product_from_ccd(16627), "40F3");
    }
}
