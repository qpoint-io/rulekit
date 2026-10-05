//! IP addresses and CIDR networks: a hand port of Go's `net.ParseIP` /
//! `net.ParseCIDR` acceptance (which delegate to `net/netip.ParseAddr`) and
//! of the text forms in `testdata/vectors/README.md` ("Text forms").

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// An IP address. An IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) is
/// normalized to IPv4 at construction, so equality, hashing, `is_v4` and the
/// text form all treat it as the IPv4 address it maps, as Go's `net.IP` does.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Ip(IpAddr);

impl Ip {
    /// Parses `s` exactly as Go's `net.ParseIP` accepts it.
    pub fn parse(s: &str) -> Option<Ip> {
        parse_addr(s.as_bytes()).map(Ip::from_addr)
    }

    /// Wraps `a`, normalizing an IPv4-mapped IPv6 address to IPv4.
    pub fn from_addr(a: IpAddr) -> Ip {
        match a {
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => Ip(IpAddr::V4(v4)),
                None => Ip(a),
            },
            IpAddr::V4(_) => Ip(a),
        }
    }

    pub fn addr(self) -> IpAddr {
        self.0
    }

    /// Go `ip.To4() != nil`: IPv4 or IPv4-mapped IPv6.
    pub fn is_v4(self) -> bool {
        self.0.is_ipv4()
    }
}

impl fmt::Display for Ip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            IpAddr::V4(v4) => write_v4(f, v4),
            IpAddr::V6(v6) => write_v6(f, v6),
        }
    }
}

/// Dotted decimal without leading zeros.
fn write_v4(f: &mut fmt::Formatter<'_>, a: Ipv4Addr) -> fmt::Result {
    let [a, b, c, d] = a.octets();
    write!(f, "{a}.{b}.{c}.{d}")
}

/// RFC 5952, as netip `Addr.appendTo6`: lowercase hex without leading zeros,
/// the first longest run of two or more zero groups replaced by `::`, no
/// embedded dotted form.
fn write_v6(f: &mut fmt::Formatter<'_>, a: Ipv6Addr) -> fmt::Result {
    let g = a.segments();
    // 8 means "no run": the loop index never reaches it.
    let (mut zero_start, mut zero_end) = (8, 8);
    for i in 0..8 {
        let mut j = i;
        while j < 8 && g[j] == 0 {
            j += 1;
        }
        // `>` (not `>=`) keeps the first run on a tie.
        if j - i >= 2 && j - i > zero_end - zero_start {
            zero_start = i;
            zero_end = j;
        }
    }
    let mut i = 0;
    while i < 8 {
        if i == zero_start {
            f.write_str("::")?;
            i = zero_end;
            if i >= 8 {
                break;
            }
        } else if i > 0 {
            f.write_str(":")?;
        }
        write!(f, "{:x}", g[i])?;
        i += 1;
    }
    Ok(())
}

/// A CIDR network: the network address with host bits cleared, and the
/// prefix length.
///
/// An IPv4-mapped network (`::ffff:10.0.0.0/104`) is stored as the IPv4
/// network it denotes (`10.0.0.0/8`): Go prints it that way, reports version
/// `v4` and prefix 8 (`cidrField`), and its `Contains` compares only the last
/// four bytes against IPv4 (or mapped) addresses, exactly as for an IPv4 net.
/// A mapped address with prefix < 96 masks away the `ffff`, so its network is
/// a plain IPv6 one (`::ffff:0.0.0.0/80` is `::/80`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Cidr {
    network: Ip,
    /// At most 32 for an IPv4 network, at most 128 for IPv6.
    prefix: u8,
}

impl Cidr {
    /// Parses `s` exactly as Go's `net.ParseCIDR` accepts it.
    pub fn parse(s: &str) -> Option<Cidr> {
        let (addr, mask) = s.split_once('/')?;
        let addr = parse_addr(addr.as_bytes())?;
        let bits = match addr {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        let n = dtoi(mask.as_bytes())?;
        if n > bits {
            return None;
        }
        // n <= 128 from here on.
        let prefix = n as u8;
        Some(match addr {
            IpAddr::V4(v4) => Cidr {
                network: Ip(IpAddr::V4(Ipv4Addr::from_bits(
                    v4.to_bits() & mask32(prefix),
                ))),
                prefix,
            },
            IpAddr::V6(v6) => {
                let net = Ipv6Addr::from_bits(v6.to_bits() & mask128(prefix));
                match net.to_ipv4_mapped() {
                    // The ffff in bytes 10..12 survives masking only when
                    // prefix >= 96, so this cannot underflow.
                    Some(v4) => Cidr {
                        network: Ip(IpAddr::V4(v4)),
                        prefix: prefix - 96,
                    },
                    None => Cidr {
                        network: Ip(IpAddr::V6(net)),
                        prefix,
                    },
                }
            }
        })
    }

    pub fn network(self) -> Ip {
        self.network
    }

    /// The prefix length; for an IPv4-mapped network, the IPv4 prefix
    /// (`::ffff:10.0.0.0/104` is 8), as Go's `cidrField`.
    pub fn prefix(self) -> u8 {
        self.prefix
    }

    pub fn is_v4(self) -> bool {
        self.network.is_v4()
    }

    /// Go `(*net.IPNet).Contains`: an IPv4 network contains only IPv4 (and
    /// IPv4-mapped) addresses, an IPv6 network only non-mapped IPv6 ones
    /// (`::/0` does not contain `1.2.3.4`).
    pub fn contains(self, ip: Ip) -> bool {
        match (self.network.0, ip.0) {
            (IpAddr::V4(n), IpAddr::V4(a)) => {
                let m = mask32(self.prefix);
                n.to_bits() & m == a.to_bits() & m
            }
            (IpAddr::V6(n), IpAddr::V6(a)) => {
                let m = mask128(self.prefix);
                n.to_bits() & m == a.to_bits() & m
            }
            _ => false,
        }
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.network, self.prefix)
    }
}

fn mask32(prefix: u8) -> u32 {
    u32::MAX.checked_shl(32 - u32::from(prefix)).unwrap_or(0)
}

fn mask128(prefix: u8) -> u128 {
    u128::MAX.checked_shl(128 - u32::from(prefix)).unwrap_or(0)
}

/// Go `net.dtoi` requiring the whole string: one or more ASCII digits (any
/// number of leading zeros; no sign, no space), failing once the value
/// reaches 0xFFFFFF.
fn dtoi(s: &[u8]) -> Option<u32> {
    const BIG: u32 = 0xFF_FFFF;
    if s.is_empty() {
        return None;
    }
    let mut n: u32 = 0;
    for &c in s {
        if !c.is_ascii_digit() {
            return None;
        }
        n = n * 10 + u32::from(c - b'0');
        if n >= BIG {
            return None;
        }
    }
    Some(n)
}

/// Go `netip.ParseAddr` minus zones (`net.ParseIP` rejects any zone, and an
/// empty zone is an error anyway). The first `.`, `:` or `%` picks the form.
/// A mapped IPv6 address is returned unnormalized, so callers can tell its
/// bit length (128) from an IPv4 one (32).
fn parse_addr(s: &[u8]) -> Option<IpAddr> {
    for &c in s {
        match c {
            b'.' => return parse_v4_fields(s).map(|o| IpAddr::V4(Ipv4Addr::from(o))),
            b':' => return parse_v6(s).map(IpAddr::V6),
            b'%' => return None,
            _ => {}
        }
    }
    None
}

/// Go `netip.parseIPv4Fields`: four decimal fields 0..=255, each non-empty
/// and without leading zeros.
fn parse_v4_fields(s: &[u8]) -> Option<[u8; 4]> {
    let mut fields = [0u8; 4];
    let mut val: u32 = 0;
    let mut pos = 0;
    let mut dig_len = 0;
    for (i, &c) in s.iter().enumerate() {
        if c.is_ascii_digit() {
            if dig_len == 1 && val == 0 {
                return None;
            }
            val = val * 10 + u32::from(c - b'0');
            dig_len += 1;
            if val > 255 {
                return None;
            }
        } else if c == b'.' {
            if i == 0 || i == s.len() - 1 || s[i - 1] == b'.' {
                return None;
            }
            if pos == 3 {
                return None;
            }
            fields[pos] = val as u8;
            pos += 1;
            val = 0;
            dig_len = 0;
        } else {
            return None;
        }
    }
    if pos < 3 {
        return None;
    }
    fields[3] = val as u8;
    Some(fields)
}

/// Go `netip.parseIPv6` for input without a zone (any `%` is rejected).
fn parse_v6(input: &[u8]) -> Option<Ipv6Addr> {
    if input.contains(&b'%') {
        return None;
    }
    let mut s = input;
    let mut ip = [0u8; 16];
    let mut ellipsis: Option<usize> = None;

    if s.len() >= 2 && s[0] == b':' && s[1] == b':' {
        ellipsis = Some(0);
        s = &s[2..];
        if s.is_empty() {
            return Some(Ipv6Addr::UNSPECIFIED);
        }
    }

    let mut i = 0;
    while i < 16 {
        let mut off = 0;
        let mut acc: u32 = 0;
        while off < s.len() {
            let c = s[off];
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => break,
            };
            // More than 4 digits in a group (so the value is < 2^16).
            if off > 3 {
                return None;
            }
            acc = (acc << 4) + u32::from(d);
            off += 1;
        }
        if off == 0 {
            return None;
        }

        // Followed by a dot: the trailing embedded IPv4, parsed from the
        // start of this group (its hex digits are reread as decimal).
        if off < s.len() && s[off] == b'.' {
            if ellipsis.is_none() && i != 12 {
                return None;
            }
            if i + 4 > 16 {
                return None;
            }
            ip[i..i + 4].copy_from_slice(&parse_v4_fields(s)?);
            s = &[];
            i += 4;
            break;
        }

        ip[i] = (acc >> 8) as u8;
        ip[i + 1] = acc as u8;
        i += 2;

        s = &s[off..];
        if s.is_empty() {
            break;
        }
        if s[0] != b':' || s.len() == 1 {
            return None;
        }
        s = &s[1..];

        if s[0] == b':' {
            if ellipsis.is_some() {
                return None;
            }
            ellipsis = Some(i);
            s = &s[1..];
            if s.is_empty() {
                break;
            }
        }
    }

    if !s.is_empty() {
        return None;
    }

    if i < 16 {
        // Too short without `::`; otherwise expand it.
        let e = ellipsis?;
        let n = 16 - i;
        ip.copy_within(e..i, e + n);
        ip[e..e + n].fill(0);
    } else if ellipsis.is_some() {
        // The `::` must stand for at least one zero group.
        return None;
    }
    Some(Ipv6Addr::from(ip))
}

#[cfg(test)]
mod tests {
    use super::{Cidr, Ip};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    // Expectations below were produced by Go 1.27.1 (net.ParseIP, net.ParseCIDR,
    // IP.String, IPNet.String, IPNet.Contains, rulekit fields.go cidrField),
    // and agree with a ~520k-case randomized differential run.

    /// (input, Some((text, is_v4)) or None): Go 1.27.1 net.ParseIP + IP.String.
    const IP_CASES: &[(&str, Option<(&str, bool)>)] = &[
        ("", None),
        ("0", None),
        ("1", None),
        ("1.2.3", None),
        ("1.2.3.4", Some(("1.2.3.4", true))),
        ("1.2.3.4.", None),
        (".1.2.3.4", None),
        ("1..2.3", None),
        ("1.2.3.4.5", None),
        ("01.2.3.4", None),
        ("1.2.3.04", None),
        ("0.0.0.0", Some(("0.0.0.0", true))),
        ("255.255.255.255", Some(("255.255.255.255", true))),
        ("256.1.1.1", None),
        ("1.2.3.-1", None),
        ("1.2.3.+1", None),
        (" 1.2.3.4", None),
        ("1.2.3.4 ", None),
        ("1.2.3.4%eth0", None),
        ("1.2.3.4/8", None),
        ("::", Some(("::", false))),
        (":::", None),
        ("::1", Some(("::1", false))),
        ("1::", Some(("1::", false))),
        ("1:", None),
        (":1", None),
        ("1::1::1", None),
        ("::ffff:1.2.3.4", Some(("1.2.3.4", true))),
        ("::FFFF:1.2.3.4", Some(("1.2.3.4", true))),
        ("::ffff:0102:0304", Some(("1.2.3.4", true))),
        ("::1.2.3.4", Some(("::102:304", false))),
        ("1.2.3.4::", None),
        ("::1.2.3.4:1", None),
        ("0:0:0:0:0:ffff:1.2.3.4", Some(("1.2.3.4", true))),
        ("0:0:0:0:0:0:1.2.3.4", Some(("::102:304", false))),
        ("1:2:3:4:5:6:1.2.3.4", Some(("1:2:3:4:5:6:102:304", false))),
        ("1:2:3:4:5:6:7:1.2.3.4", None),
        ("1:2:3:4:5::1.2.3.4", Some(("1:2:3:4:5:0:102:304", false))),
        ("1::2:3:4:5:6:1.2.3.4", None),
        ("1:2:3:4:5:6:7::1.2.3.4", None),
        ("1:2:3:4:5:6::1.2.3.4", None),
        ("::ffff:01.2.3.4", None),
        ("::ffff:1.2.3", None),
        ("::ffff:1.2.3.4.5", None),
        ("::ffff:256.2.3.4", None),
        ("::1234.1.1.1", None),
        ("::0255.1.1.1", None),
        ("::1a.2.3.4", None),
        ("::12345", None),
        ("::01234", None),
        ("::0fff", Some(("::fff", false))),
        ("1:2:3:4:5:6:7:8", Some(("1:2:3:4:5:6:7:8", false))),
        ("1:2:3:4:5:6:7:8:9", None),
        ("1:2:3:4:5:6:7", None),
        ("1:2:3:4:5:6:7::", Some(("1:2:3:4:5:6:7:0", false))),
        ("::1:2:3:4:5:6:7", Some(("0:1:2:3:4:5:6:7", false))),
        ("::1:2:3:4:5:6:7:8", None),
        ("1:2:3:4::5:6:7:8", None),
        ("1:2:3:4:5:6:7:8::", None),
        ("fe80::1%eth0", None),
        ("fe80::1%", None),
        ("%eth0", None),
        ("fe80::1%%", None),
        (
            "2001:0DB8:0000:0000:0000:FF00:0042:8329",
            Some(("2001:db8::ff00:42:8329", false)),
        ),
        ("1:0:0:2:0:0:0:3", Some(("1:0:0:2::3", false))),
        ("2001:db8:0:0:1:0:0:1", Some(("2001:db8::1:0:0:1", false))),
        (
            "2001:db8:0:1:1:1:1:1",
            Some(("2001:db8:0:1:1:1:1:1", false)),
        ),
        ("0:0:0:0:0:0:0:0", Some(("::", false))),
        ("0:0:0:0:0:0:0:1", Some(("::1", false))),
        ("1:0:0:0:0:0:0:0", Some(("1::", false))),
        ("0:1:0:0:0:0:0:0", Some(("0:1::", false))),
        ("0:0:1:0:0:0:0:0", Some(("0:0:1::", false))),
        ("1:0:1:0:1:0:1:0", Some(("1:0:1:0:1:0:1:0", false))),
        ("0:1:0:1:0:1:0:1", Some(("0:1:0:1:0:1:0:1", false))),
        ("1:0:0:1:0:0:1:0", Some(("1::1:0:0:1:0", false))),
        ("0:0:1:0:0:1:0:0", Some(("::1:0:0:1:0:0", false))),
        ("1:1:0:0:1:1:0:0", Some(("1:1::1:1:0:0", false))),
        ("::ffff:0:0", Some(("0.0.0.0", true))),
        ("::ffff:0.0.0.0", Some(("0.0.0.0", true))),
        ("::fffe:1.2.3.4", Some(("::fffe:102:304", false))),
        ("0:0:0:0:0:1:ffff:0102", Some(("::1:ffff:102", false))),
        ("::ffff", Some(("::ffff", false))),
        ("::ffff:", None),
        (":ffff::", None),
        ("1:2:3:4:5:6:7:8%z", None),
        ("[::1]", None),
        ("::1]", None),
        ("0x1.2.3.4", None),
        ("1.2.3.0x4", None),
        ("１.2.3.4", None),
        ("::ｆ", None),
        ("a:b:c:d:e:f:0:0", Some(("a:b:c:d:e:f::", false))),
        ("A:B::C:D", Some(("a:b::c:d", false))),
        ("::g", None),
        ("g::", None),
        ("1:2:3:4:5:6:7:8.", None),
        (":1:2:3:4:5:6:7", None),
        ("1.2.3.4:80", None),
        ("::1.2.3.4%x", None),
        ("64:ff9b::1.2.3.4", Some(("64:ff9b::102:304", false))),
        ("100::", Some(("100::", false))),
        ("::0:0:0", Some(("::", false))),
        ("0::0", Some(("::", false))),
        ("00000::", None),
        ("0000::", Some(("::", false))),
        ("1:2:3:4:5:6:7:", None),
        ("1::2:", None),
        ("::ffff:1.2.3.4:", None),
        (
            "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff",
            Some(("ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff", false)),
        ),
        ("::ffff:ffff:ffff", Some(("255.255.255.255", true))),
        ("::ffff:255.255.255.255", Some(("255.255.255.255", true))),
    ];

    /// Go `ParseCIDR` result: `IPNet.String`, `cidrField` prefix, version v4.
    type CidrWant = Option<(&'static str, u8, bool)>;

    /// (input, Some((text, prefix, is_v4)) or None): Go net.ParseCIDR + IPNet.String + cidrField.
    const CIDR_CASES: &[(&str, CidrWant)] = &[
        ("10.1.2.3/8", Some(("10.0.0.0/8", 8, true))),
        ("2001:DB8::1/32", Some(("2001:db8::/32", 32, false))),
        ("::ffff:10.0.0.0/104", Some(("10.0.0.0/8", 8, true))),
        ("0.0.0.0/0", Some(("0.0.0.0/0", 0, true))),
        ("::/0", Some(("::/0", 0, false))),
        ("::ffff:0.0.0.0/80", Some(("::/80", 80, false))),
        ("::ffff:0.0.0.0/96", Some(("0.0.0.0/0", 0, true))),
        ("::ffff:1.2.3.4/95", Some(("::fffe:0:0/95", 95, false))),
        ("::ffff:1.2.3.4/96", Some(("0.0.0.0/0", 0, true))),
        ("::ffff:1.2.3.4/97", Some(("0.0.0.0/1", 1, true))),
        ("::ffff:1.2.3.4/128", Some(("1.2.3.4/32", 32, true))),
        ("::ffff:1.2.3.4/90", Some(("::ffc0:0:0/90", 90, false))),
        ("::ffff:1.2.3.4/79", Some(("::/79", 79, false))),
        ("::ffff:1.2.3.4/81", Some(("::8000:0:0/81", 81, false))),
        ("::ffff:1.2.3.4/88", Some(("::ff00:0:0/88", 88, false))),
        ("::ffff:1.2.3.4/129", None),
        ("1.2.3.4/33", None),
        ("1.2.3.4/32", Some(("1.2.3.4/32", 32, true))),
        ("/8", None),
        ("1.2.3.4/", None),
        ("1.2.3.4", None),
        ("::1.2.3.4/104", Some(("::100:0/104", 104, false))),
        ("::/129", None),
        ("fe80::1%eth0/64", None),
        ("1.2.3.4//8", None),
        ("::ffff:1.2.3.4/0", Some(("::/0", 0, false))),
        ("::ffff:1:2/104", Some(("0.0.0.0/8", 8, true))),
        ("0:0:0:0:0:ffff:a00:0/104", Some(("10.0.0.0/8", 8, true))),
        ("::fffe:1.2.3.4/104", Some(("::fffe:100:0/104", 104, false))),
        (
            "1::ffff:1.2.3.4/104",
            Some(("1::ffff:100:0/104", 104, false)),
        ),
        ("01.2.3.4/8", None),
        ("1.2.3/8", None),
        ("10.0.0.1/08", Some(("10.0.0.0/8", 8, true))),
        ("10.0.0.1/008", Some(("10.0.0.0/8", 8, true))),
        (
            "10.0.0.1/0000000000000000000000000008",
            Some(("10.0.0.0/8", 8, true)),
        ),
        ("10.0.0.1/+8", None),
        ("10.0.0.1/-0", None),
        ("10.0.0.1/ 8", None),
        ("10.0.0.1/8 ", None),
        (" 10.0.0.1/8", None),
        ("10.0.0.1/8/8", None),
        ("10.0.0.1/16777215", None),
        ("10.0.0.1/16777214", None),
        ("10.0.0.1/99999999999", None),
        ("10.0.0.1/0x10", None),
        ("10.0.0.1/1e1", None),
        ("10.0.0.1/٣", None),
        ("10.0.0.1/31", Some(("10.0.0.0/31", 31, true))),
        ("255.255.255.255/1", Some(("128.0.0.0/1", 1, true))),
        (
            "2001:db8::ffff:1:2/127",
            Some(("2001:db8::ffff:1:2/127", 127, false)),
        ),
        (
            "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/65",
            Some(("ffff:ffff:ffff:ffff:8000::/65", 65, false)),
        ),
        (
            "1:2:3:4:5:6:7:8/128",
            Some(("1:2:3:4:5:6:7:8/128", 128, false)),
        ),
        (
            "::ffff:255.255.255.255/120",
            Some(("255.255.255.0/24", 24, true)),
        ),
        ("::/128", Some(("::/128", 128, false))),
        ("1::/0128", Some(("1::/128", 128, false))),
        ("1::/00", Some(("::/0", 0, false))),
        ("::1.2.3.4/96", Some(("::/96", 96, false))),
    ];

    /// (cidr, ip, Go (*net.IPNet).Contains).
    const CONTAINS_CASES: &[(&str, &str, bool)] = &[
        ("::ffff:10.0.0.0/104", "10.1.2.3", true),
        ("::ffff:10.0.0.0/104", "::ffff:10.1.2.3", true),
        ("::ffff:10.0.0.0/104", "11.0.0.0", false),
        ("::ffff:10.0.0.0/104", "::", false),
        ("::ffff:10.0.0.0/104", "::1", false),
        ("::ffff:10.0.0.0/104", "::ffff:0:0", false),
        ("::ffff:10.0.0.0/104", "0.0.0.0", false),
        ("::ffff:10.0.0.0/104", "::a01:203", false),
        ("::ffff:10.0.0.0/104", "1.2.3.4", false),
        ("::ffff:10.0.0.0/104", "::ffff:1.2.3.4", false),
        ("::ffff:10.0.0.0/104", "1.2.3.5", false),
        ("::ffff:10.0.0.0/104", "1.2.3.6", false),
        ("::ffff:10.0.0.0/104", "::fffe:1.2.3.4", false),
        ("::ffff:10.0.0.0/104", "::1.2.3.4", false),
        ("::ffff:10.0.0.0/104", "2001:db8::1", false),
        ("::ffff:10.0.0.0/104", "2001:db9::", false),
        ("::ffff:10.0.0.0/104", "::ffff:1.0.0.0", false),
        ("10.0.0.0/8", "10.1.2.3", true),
        ("10.0.0.0/8", "::ffff:10.1.2.3", true),
        ("10.0.0.0/8", "11.0.0.0", false),
        ("10.0.0.0/8", "::", false),
        ("10.0.0.0/8", "::1", false),
        ("10.0.0.0/8", "::ffff:0:0", false),
        ("10.0.0.0/8", "0.0.0.0", false),
        ("10.0.0.0/8", "::a01:203", false),
        ("10.0.0.0/8", "1.2.3.4", false),
        ("10.0.0.0/8", "::ffff:1.2.3.4", false),
        ("10.0.0.0/8", "1.2.3.5", false),
        ("10.0.0.0/8", "1.2.3.6", false),
        ("10.0.0.0/8", "::fffe:1.2.3.4", false),
        ("10.0.0.0/8", "::1.2.3.4", false),
        ("10.0.0.0/8", "2001:db8::1", false),
        ("10.0.0.0/8", "2001:db9::", false),
        ("10.0.0.0/8", "::ffff:1.0.0.0", false),
        ("::ffff:0.0.0.0/80", "10.1.2.3", false),
        ("::ffff:0.0.0.0/80", "::ffff:10.1.2.3", false),
        ("::ffff:0.0.0.0/80", "11.0.0.0", false),
        ("::ffff:0.0.0.0/80", "::", true),
        ("::ffff:0.0.0.0/80", "::1", true),
        ("::ffff:0.0.0.0/80", "::ffff:0:0", false),
        ("::ffff:0.0.0.0/80", "0.0.0.0", false),
        ("::ffff:0.0.0.0/80", "::a01:203", true),
        ("::ffff:0.0.0.0/80", "1.2.3.4", false),
        ("::ffff:0.0.0.0/80", "::ffff:1.2.3.4", false),
        ("::ffff:0.0.0.0/80", "1.2.3.5", false),
        ("::ffff:0.0.0.0/80", "1.2.3.6", false),
        ("::ffff:0.0.0.0/80", "::fffe:1.2.3.4", true),
        ("::ffff:0.0.0.0/80", "::1.2.3.4", true),
        ("::ffff:0.0.0.0/80", "2001:db8::1", false),
        ("::ffff:0.0.0.0/80", "2001:db9::", false),
        ("::ffff:0.0.0.0/80", "::ffff:1.0.0.0", false),
        ("::ffff:0.0.0.0/96", "10.1.2.3", true),
        ("::ffff:0.0.0.0/96", "::ffff:10.1.2.3", true),
        ("::ffff:0.0.0.0/96", "11.0.0.0", true),
        ("::ffff:0.0.0.0/96", "::", false),
        ("::ffff:0.0.0.0/96", "::1", false),
        ("::ffff:0.0.0.0/96", "::ffff:0:0", true),
        ("::ffff:0.0.0.0/96", "0.0.0.0", true),
        ("::ffff:0.0.0.0/96", "::a01:203", false),
        ("::ffff:0.0.0.0/96", "1.2.3.4", true),
        ("::ffff:0.0.0.0/96", "::ffff:1.2.3.4", true),
        ("::ffff:0.0.0.0/96", "1.2.3.5", true),
        ("::ffff:0.0.0.0/96", "1.2.3.6", true),
        ("::ffff:0.0.0.0/96", "::fffe:1.2.3.4", false),
        ("::ffff:0.0.0.0/96", "::1.2.3.4", false),
        ("::ffff:0.0.0.0/96", "2001:db8::1", false),
        ("::ffff:0.0.0.0/96", "2001:db9::", false),
        ("::ffff:0.0.0.0/96", "::ffff:1.0.0.0", true),
        ("::/0", "10.1.2.3", false),
        ("::/0", "::ffff:10.1.2.3", false),
        ("::/0", "11.0.0.0", false),
        ("::/0", "::", true),
        ("::/0", "::1", true),
        ("::/0", "::ffff:0:0", false),
        ("::/0", "0.0.0.0", false),
        ("::/0", "::a01:203", true),
        ("::/0", "1.2.3.4", false),
        ("::/0", "::ffff:1.2.3.4", false),
        ("::/0", "1.2.3.5", false),
        ("::/0", "1.2.3.6", false),
        ("::/0", "::fffe:1.2.3.4", true),
        ("::/0", "::1.2.3.4", true),
        ("::/0", "2001:db8::1", true),
        ("::/0", "2001:db9::", true),
        ("::/0", "::ffff:1.0.0.0", false),
        ("0.0.0.0/0", "10.1.2.3", true),
        ("0.0.0.0/0", "::ffff:10.1.2.3", true),
        ("0.0.0.0/0", "11.0.0.0", true),
        ("0.0.0.0/0", "::", false),
        ("0.0.0.0/0", "::1", false),
        ("0.0.0.0/0", "::ffff:0:0", true),
        ("0.0.0.0/0", "0.0.0.0", true),
        ("0.0.0.0/0", "::a01:203", false),
        ("0.0.0.0/0", "1.2.3.4", true),
        ("0.0.0.0/0", "::ffff:1.2.3.4", true),
        ("0.0.0.0/0", "1.2.3.5", true),
        ("0.0.0.0/0", "1.2.3.6", true),
        ("0.0.0.0/0", "::fffe:1.2.3.4", false),
        ("0.0.0.0/0", "::1.2.3.4", false),
        ("0.0.0.0/0", "2001:db8::1", false),
        ("0.0.0.0/0", "2001:db9::", false),
        ("0.0.0.0/0", "::ffff:1.0.0.0", true),
        ("::ffff:1.2.3.4/90", "10.1.2.3", false),
        ("::ffff:1.2.3.4/90", "::ffff:10.1.2.3", false),
        ("::ffff:1.2.3.4/90", "11.0.0.0", false),
        ("::ffff:1.2.3.4/90", "::", false),
        ("::ffff:1.2.3.4/90", "::1", false),
        ("::ffff:1.2.3.4/90", "::ffff:0:0", false),
        ("::ffff:1.2.3.4/90", "0.0.0.0", false),
        ("::ffff:1.2.3.4/90", "::a01:203", false),
        ("::ffff:1.2.3.4/90", "1.2.3.4", false),
        ("::ffff:1.2.3.4/90", "::ffff:1.2.3.4", false),
        ("::ffff:1.2.3.4/90", "1.2.3.5", false),
        ("::ffff:1.2.3.4/90", "1.2.3.6", false),
        ("::ffff:1.2.3.4/90", "::fffe:1.2.3.4", true),
        ("::ffff:1.2.3.4/90", "::1.2.3.4", false),
        ("::ffff:1.2.3.4/90", "2001:db8::1", false),
        ("::ffff:1.2.3.4/90", "2001:db9::", false),
        ("::ffff:1.2.3.4/90", "::ffff:1.0.0.0", false),
        ("::1.2.3.4/104", "10.1.2.3", false),
        ("::1.2.3.4/104", "::ffff:10.1.2.3", false),
        ("::1.2.3.4/104", "11.0.0.0", false),
        ("::1.2.3.4/104", "::", false),
        ("::1.2.3.4/104", "::1", false),
        ("::1.2.3.4/104", "::ffff:0:0", false),
        ("::1.2.3.4/104", "0.0.0.0", false),
        ("::1.2.3.4/104", "::a01:203", false),
        ("::1.2.3.4/104", "1.2.3.4", false),
        ("::1.2.3.4/104", "::ffff:1.2.3.4", false),
        ("::1.2.3.4/104", "1.2.3.5", false),
        ("::1.2.3.4/104", "1.2.3.6", false),
        ("::1.2.3.4/104", "::fffe:1.2.3.4", false),
        ("::1.2.3.4/104", "::1.2.3.4", true),
        ("::1.2.3.4/104", "2001:db8::1", false),
        ("::1.2.3.4/104", "2001:db9::", false),
        ("::1.2.3.4/104", "::ffff:1.0.0.0", false),
        ("::ffff:1.2.3.4/128", "10.1.2.3", false),
        ("::ffff:1.2.3.4/128", "::ffff:10.1.2.3", false),
        ("::ffff:1.2.3.4/128", "11.0.0.0", false),
        ("::ffff:1.2.3.4/128", "::", false),
        ("::ffff:1.2.3.4/128", "::1", false),
        ("::ffff:1.2.3.4/128", "::ffff:0:0", false),
        ("::ffff:1.2.3.4/128", "0.0.0.0", false),
        ("::ffff:1.2.3.4/128", "::a01:203", false),
        ("::ffff:1.2.3.4/128", "1.2.3.4", true),
        ("::ffff:1.2.3.4/128", "::ffff:1.2.3.4", true),
        ("::ffff:1.2.3.4/128", "1.2.3.5", false),
        ("::ffff:1.2.3.4/128", "1.2.3.6", false),
        ("::ffff:1.2.3.4/128", "::fffe:1.2.3.4", false),
        ("::ffff:1.2.3.4/128", "::1.2.3.4", false),
        ("::ffff:1.2.3.4/128", "2001:db8::1", false),
        ("::ffff:1.2.3.4/128", "2001:db9::", false),
        ("::ffff:1.2.3.4/128", "::ffff:1.0.0.0", false),
        ("1.2.3.4/32", "10.1.2.3", false),
        ("1.2.3.4/32", "::ffff:10.1.2.3", false),
        ("1.2.3.4/32", "11.0.0.0", false),
        ("1.2.3.4/32", "::", false),
        ("1.2.3.4/32", "::1", false),
        ("1.2.3.4/32", "::ffff:0:0", false),
        ("1.2.3.4/32", "0.0.0.0", false),
        ("1.2.3.4/32", "::a01:203", false),
        ("1.2.3.4/32", "1.2.3.4", true),
        ("1.2.3.4/32", "::ffff:1.2.3.4", true),
        ("1.2.3.4/32", "1.2.3.5", false),
        ("1.2.3.4/32", "1.2.3.6", false),
        ("1.2.3.4/32", "::fffe:1.2.3.4", false),
        ("1.2.3.4/32", "::1.2.3.4", false),
        ("1.2.3.4/32", "2001:db8::1", false),
        ("1.2.3.4/32", "2001:db9::", false),
        ("1.2.3.4/32", "::ffff:1.0.0.0", false),
        ("2001:db8::/32", "10.1.2.3", false),
        ("2001:db8::/32", "::ffff:10.1.2.3", false),
        ("2001:db8::/32", "11.0.0.0", false),
        ("2001:db8::/32", "::", false),
        ("2001:db8::/32", "::1", false),
        ("2001:db8::/32", "::ffff:0:0", false),
        ("2001:db8::/32", "0.0.0.0", false),
        ("2001:db8::/32", "::a01:203", false),
        ("2001:db8::/32", "1.2.3.4", false),
        ("2001:db8::/32", "::ffff:1.2.3.4", false),
        ("2001:db8::/32", "1.2.3.5", false),
        ("2001:db8::/32", "1.2.3.6", false),
        ("2001:db8::/32", "::fffe:1.2.3.4", false),
        ("2001:db8::/32", "::1.2.3.4", false),
        ("2001:db8::/32", "2001:db8::1", true),
        ("2001:db8::/32", "2001:db9::", false),
        ("2001:db8::/32", "::ffff:1.0.0.0", false),
        ("1.2.3.4/31", "10.1.2.3", false),
        ("1.2.3.4/31", "::ffff:10.1.2.3", false),
        ("1.2.3.4/31", "11.0.0.0", false),
        ("1.2.3.4/31", "::", false),
        ("1.2.3.4/31", "::1", false),
        ("1.2.3.4/31", "::ffff:0:0", false),
        ("1.2.3.4/31", "0.0.0.0", false),
        ("1.2.3.4/31", "::a01:203", false),
        ("1.2.3.4/31", "1.2.3.4", true),
        ("1.2.3.4/31", "::ffff:1.2.3.4", true),
        ("1.2.3.4/31", "1.2.3.5", true),
        ("1.2.3.4/31", "1.2.3.6", false),
        ("1.2.3.4/31", "::fffe:1.2.3.4", false),
        ("1.2.3.4/31", "::1.2.3.4", false),
        ("1.2.3.4/31", "2001:db8::1", false),
        ("1.2.3.4/31", "2001:db9::", false),
        ("1.2.3.4/31", "::ffff:1.0.0.0", false),
    ];

    #[test]
    fn parse_ip_matches_go() {
        for &(input, want) in IP_CASES {
            let got = Ip::parse(input).map(|ip| (ip.to_string(), ip.is_v4()));
            assert_eq!(
                got.as_ref().map(|(t, v)| (t.as_str(), *v)),
                want,
                "Ip::parse({input:?})"
            );
        }
    }

    #[test]
    fn parse_cidr_matches_go() {
        for &(input, want) in CIDR_CASES {
            let got = Cidr::parse(input).map(|c| (c.to_string(), c.prefix(), c.is_v4()));
            assert_eq!(
                got.as_ref().map(|(t, p, v)| (t.as_str(), *p, *v)),
                want,
                "Cidr::parse({input:?})"
            );
        }
    }

    #[test]
    fn contains_matches_go() {
        for &(cidr, ip, want) in CONTAINS_CASES {
            let c = Cidr::parse(cidr).unwrap();
            let a = Ip::parse(ip).unwrap();
            assert_eq!(c.contains(a), want, "{cidr} contains {ip}");
        }
    }

    #[test]
    fn mapped_is_normalized() {
        let mapped = Ip::from_addr(IpAddr::V6(Ipv4Addr::new(1, 2, 3, 4).to_ipv6_mapped()));
        assert_eq!(mapped.addr(), IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)));
        assert_eq!(mapped, Ip::parse("1.2.3.4").unwrap());
        assert_eq!(Ip::parse("::ffff:1.2.3.4"), Ip::parse("1.2.3.4"));
        let compat = Ip::from_addr(IpAddr::V6(Ipv4Addr::new(1, 2, 3, 4).to_ipv6_compatible()));
        assert!(!compat.is_v4());
        assert_eq!(
            compat.addr(),
            IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0x102, 0x304))
        );

        let c = Cidr::parse("::ffff:10.0.0.0/104").unwrap();
        assert_eq!(c, Cidr::parse("10.0.0.0/8").unwrap());
        assert_eq!(c.network(), Ip::parse("10.0.0.0").unwrap());
        let c = Cidr::parse("::ffff:0.0.0.0/80").unwrap();
        assert_eq!(c.network(), Ip::parse("::").unwrap());
        assert_eq!(c.prefix(), 80);
    }

    #[test]
    fn text_forms_vectors() {
        let ip = |s| Ip::parse(s).unwrap().to_string();
        let cidr = |s| Cidr::parse(s).unwrap().to_string();
        assert_eq!(ip("10.0.0.1"), "10.0.0.1");
        assert_eq!(
            ip("2001:0DB8:0000:0000:0000:FF00:0042:8329"),
            "2001:db8::ff00:42:8329"
        );
        assert_eq!(ip("1:0:0:2:0:0:0:3"), "1:0:0:2::3");
        assert_eq!(ip("2001:db8:0:0:1:0:0:1"), "2001:db8::1:0:0:1");
        assert_eq!(ip("2001:db8:0:1:1:1:1:1"), "2001:db8:0:1:1:1:1:1");
        assert_eq!(ip("0:0:0:0:0:0:0:0"), "::");
        assert_eq!(ip("::1"), "::1");
        assert_eq!(ip("::ffff:1.2.3.4"), "1.2.3.4");
        assert_eq!(ip("::1.2.3.4"), "::102:304");
        assert_eq!(cidr("10.1.2.3/8"), "10.0.0.0/8");
        assert_eq!(cidr("2001:DB8::1/32"), "2001:db8::/32");
        assert_eq!(cidr("::ffff:10.0.0.0/104"), "10.0.0.0/8");
        assert_eq!(cidr("0.0.0.0/0"), "0.0.0.0/0");
        assert_eq!(cidr("::/0"), "::/0");
    }
}
