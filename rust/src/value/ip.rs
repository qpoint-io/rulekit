//! IP addresses and CIDR networks on `std::net` and `ipnet`, plus rulekit's
//! own rules: an IPv4-mapped IPv6 address or network is IPv4, and the text
//! forms of `testdata/vectors/README.md` ("Text forms"), which std's
//! `Display` produces once mapped values are normalized to IPv4.

use super::ValueParseError;
use ipnet::{IpNet, Ipv4Net};
use std::fmt;
use std::net::IpAddr;

/// An IP address. An IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) is
/// normalized to IPv4 at construction, so equality, hashing, `is_v4` and the
/// text form all treat it as the IPv4 address it maps.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Ip(IpAddr);

impl Ip {
    /// Parses `s` with std's `IpAddr` grammar: dotted decimal IPv4 without
    /// leading zeros, or IPv6 without a zone.
    pub fn parse(s: &str) -> Result<Ip, ValueParseError> {
        s.parse()
            .map(Ip::from_addr)
            .map_err(|_| ValueParseError::new("IP address", s, None))
    }

    /// Wraps `a`, normalizing an IPv4-mapped IPv6 address to IPv4.
    pub fn from_addr(a: IpAddr) -> Ip {
        Ip(a.to_canonical())
    }

    /// The address (never IPv4-mapped IPv6).
    pub fn addr(self) -> IpAddr {
        self.0
    }

    /// IPv4 or IPv4-mapped IPv6.
    pub fn is_v4(self) -> bool {
        self.0.is_ipv4()
    }
}

/// std's `Display` is the text form: dotted decimal for IPv4, RFC 5952 for
/// IPv6. Its one embedded-dotted case (`::ffff:a.b.c.d`) cannot occur, as
/// mapped addresses are IPv4 here.
impl fmt::Display for Ip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A CIDR network with host bits cleared.
///
/// An IPv4-mapped network (`::ffff:10.0.0.0/104`) is the IPv4 network it
/// denotes (`10.0.0.0/8`, prefix 8). A mapped address with prefix < 96
/// masks away the `ffff`, so its network is a plain IPv6 one
/// (`::ffff:0.0.0.0/80` is `::/80`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Cidr(IpNet);

impl Cidr {
    /// Parses `address/prefix`: the address as [`Ip::parse`] reads it, the
    /// prefix as ASCII decimal digits (leading zeros allowed) no larger than
    /// the address's bit length.
    pub fn parse(s: &str) -> Result<Cidr, ValueParseError> {
        Self::try_parse(s).ok_or_else(|| ValueParseError::new("CIDR", s, None))
    }

    fn try_parse(s: &str) -> Option<Cidr> {
        let (addr, prefix) = s.split_once('/')?;
        // `u8::from_str` also accepts a leading `+`.
        if !prefix.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let net = IpNet::new(addr.parse().ok()?, prefix.parse().ok()?).ok()?;
        Some(Cidr::from_net(net))
    }

    /// Wraps `net`, clearing host bits and normalizing an IPv4-mapped
    /// network to the IPv4 network it denotes.
    pub fn from_net(net: IpNet) -> Cidr {
        Cidr(match net.trunc() {
            IpNet::V6(n) => match (n.network().to_ipv4_mapped(), n.prefix_len().checked_sub(96)) {
                // A prefix of 96..=128 leaves at most 32 bits.
                (Some(v4), Some(p)) => IpNet::V4(Ipv4Net::new_assert(v4, p)),
                _ => IpNet::V6(n),
            },
            n => n,
        })
    }

    /// The network address.
    pub fn network(self) -> Ip {
        // Normalized at construction: an IPv6 network is never mapped.
        Ip(self.0.network())
    }

    /// The prefix length; for an IPv4-mapped network, the IPv4 prefix
    /// (`::ffff:10.0.0.0/104` is 8).
    pub fn prefix(self) -> u8 {
        self.0.prefix_len()
    }

    /// Whether this is an IPv4 network.
    pub fn is_v4(self) -> bool {
        matches!(self.0, IpNet::V4(_))
    }

    /// An IPv4 network contains only IPv4 (incl. mapped) addresses, an IPv6
    /// network only non-mapped IPv6 ones (`::/0` does not contain `1.2.3.4`).
    pub fn contains(self, ip: Ip) -> bool {
        self.0.contains(&ip.0)
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::{Cidr, Ip};
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    // Expectations below were produced by Go 1.27.1 (net.ParseIP, net.ParseCIDR,
    // IP.String, IPNet.String, IPNet.Contains, rulekit fields.go cidrField).
    // A differential run against Go (~8.6M parse inputs: random, mutated,
    // exhaustive short strings, structured IPv6 group shapes; 400k Contains
    // pairs) found no difference.

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
        // ipnet's own `FromStr` would accept leading-zero octets and reject
        // prefixes longer than 2 (IPv4) or 3 (IPv6) digits.
        ("010.0.0.1/8", None),
        ("::ffff:010.0.0.1/104", None),
        ("::/0008", Some(("::/8", 8, false))),
        ("10.0.0.1/0032", Some(("10.0.0.1/32", 32, true))),
        ("1.2.3.4/+32", None),
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
            let got = Ip::parse(input).ok().map(|ip| (ip.to_string(), ip.is_v4()));
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
            let got = Cidr::parse(input)
                .ok()
                .map(|c| (c.to_string(), c.prefix(), c.is_v4()));
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
}
