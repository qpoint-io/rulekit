//! MAC addresses: a port of `mac.go` `parseMAC` and Go's
//! `net.HardwareAddr.String` text form.

use std::fmt;

/// A 6- or 8-byte MAC address, stored inline. Bytes past `len` are always
/// zero, so the derived equality and hash only see the address.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Mac {
    len: u8,
    bytes: [u8; 8],
}

impl Mac {
    /// Parses hex pairs separated by `:` or `-` (`01:23:45:67:89:ab`) or
    /// groups of four hex digits separated by `.` (`0123.4567.89ab`), 6 or 8
    /// bytes. The separator is the first of `:`, `-`, `.` present anywhere in
    /// `s` (in that priority), as `mac.go`.
    pub fn parse(s: &str) -> Option<Mac> {
        let s = s.as_bytes();
        let (sep, width) = if s.contains(&b':') {
            (b':', 2)
        } else if s.contains(&b'-') {
            (b'-', 2)
        } else if s.contains(&b'.') {
            (b'.', 4)
        } else {
            return None;
        };
        let groups = s.iter().filter(|&&c| c == sep).count() + 1;
        let size = groups * width / 2;
        if size != 6 && size != 8 {
            return None;
        }
        let mut mac = Mac {
            len: size as u8,
            bytes: [0; 8],
        };
        let mut out = 0;
        for group in s.split(|&c| c == sep) {
            if group.len() != width {
                return None;
            }
            for pair in group.chunks_exact(2) {
                mac.bytes[out] = (hex_val(pair[0])? << 4) | hex_val(pair[1])?;
                out += 1;
            }
        }
        Some(mac)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }

    /// A MAC of exactly 6 or 8 bytes.
    pub fn from_bytes(b: &[u8]) -> Option<Mac> {
        if b.len() != 6 && b.len() != 8 {
            return None;
        }
        let mut bytes = [0; 8];
        bytes[..b.len()].copy_from_slice(b);
        Some(Mac {
            len: b.len() as u8,
            bytes,
        })
    }
}

/// Go `hex.DecodeString` digit: either case.
fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Lowercase hex pairs joined by `:`.
impl fmt::Display for Mac {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.as_bytes().iter().enumerate() {
            if i > 0 {
                f.write_str(":")?;
            }
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Mac;

    /// (input, Go parseMAC result as HardwareAddr.String(), or None).
    /// Expectations produced by running mac.go's parseMAC under Go 1.27.1.
    const CASES: &[(&str, Option<&str>)] = &[
        ("aa:bb:cc:dd:ee:ff", Some("aa:bb:cc:dd:ee:ff")),
        ("AA-BB-CC-DD-EE-FF", Some("aa:bb:cc:dd:ee:ff")),
        ("0123.4567.89AB.CDEF", Some("01:23:45:67:89:ab:cd:ef")),
        ("0123.4567.89ab", Some("01:23:45:67:89:ab")),
        ("01:23:45:67:89:ab:cd:ef", Some("01:23:45:67:89:ab:cd:ef")),
        ("01-23-45-67-89-ab-cd-ef", Some("01:23:45:67:89:ab:cd:ef")),
        ("01:23:45:67:89", None),
        ("01:23:45:67:89:ab:cd", None),
        ("01:23:45:67:89:ab:cd:ef:01", None),
        ("01:23:45:67:89:ab:cd:ef:01:23", None),
        ("0123.4567", None),
        ("0123.4567.89ab.cdef.0123", None),
        ("1:23:45:67:89:ab", None),
        ("001:23:45:67:89:ab", None),
        ("01:23:45:67:89:ag", None),
        ("01:23-45:67:89:ab", None),
        ("01-23-45-67-89-ab.", None),
        ("01-23-45-67.89-ab", None),
        ("012.34567.89ab", None),
        ("0123.4567.89a", None),
        ("0123:4567:89ab", None),
        ("012345678 9ab", None),
        ("0123456789ab", None),
        ("", None),
        (":::::", None),
        ("01:23:45:67:89:ab:", None),
        (" 01:23:45:67:89:ab", None),
        ("01:23:45:67:89:+b", None),
        ("é1:23:45:67:89:ab", None),
        ("00:00:00:00:00:00", Some("00:00:00:00:00:00")),
        ("ff-ff-ff-ff-ff-ff-ff-ff", Some("ff:ff:ff:ff:ff:ff:ff:ff")),
        ("fFfF.fFfF.fFfF", Some("ff:ff:ff:ff:ff:ff")),
    ];

    #[test]
    fn parse_matches_go() {
        for &(input, want) in CASES {
            let got = Mac::parse(input).map(|m| m.to_string());
            assert_eq!(got.as_deref(), want, "parse({input:?})");
        }
    }

    #[test]
    fn bytes_round_trip() {
        let m = Mac::parse("0123.4567.89ab").unwrap();
        assert_eq!(m.as_bytes(), &[0x01, 0x23, 0x45, 0x67, 0x89, 0xab]);
        assert_eq!(Mac::from_bytes(m.as_bytes()), Some(m));
        assert_eq!(Mac::from_bytes(&[1; 8]).unwrap().as_bytes(), &[1; 8]);
        assert_eq!(Mac::from_bytes(&[1; 7]), None);
        assert_eq!(Mac::from_bytes(&[]), None);
        // Equal addresses from different notations are equal values.
        assert_eq!(
            Mac::parse("AA-BB-CC-DD-EE-FF"),
            Mac::parse("aa:bb:cc:dd:ee:ff")
        );
        assert_ne!(
            Mac::parse("00:00:00:00:00:00"),
            Mac::parse("00:00:00:00:00:00:00:00")
        );
    }

    #[test]
    fn text_forms_vectors() {
        assert_eq!(
            Mac::parse("AA-BB-CC-DD-EE-FF").unwrap().to_string(),
            "aa:bb:cc:dd:ee:ff"
        );
        assert_eq!(
            Mac::parse("0123.4567.89AB.CDEF").unwrap().to_string(),
            "01:23:45:67:89:ab:cd:ef"
        );
    }
}
