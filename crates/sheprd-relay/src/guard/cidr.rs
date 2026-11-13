//! Address ranges for manual bans: `203.0.113.7`, `203.0.113.0/24`,
//! `2001:db8::/32`. IPv4-mapped IPv6 addresses match their IPv4 range.

use std::{fmt, net::IpAddr, str::FromStr};

/// An address and how many leading bits of it a range keeps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cidr {
    base: IpAddr,
    bits: u8,
}

impl Cidr {
    /// Whether `address` falls in this range.
    #[must_use]
    pub fn contains(&self, address: IpAddr) -> bool {
        match (self.base, address.to_canonical()) {
            (IpAddr::V4(base), IpAddr::V4(address)) => {
                let mask = u32::MAX.checked_shl(32 - u32::from(self.bits)).unwrap_or(0);
                u32::from(base) & mask == u32::from(address) & mask
            }
            (IpAddr::V6(base), IpAddr::V6(address)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.bits))
                    .unwrap_or(0);
                u128::from(base) & mask == u128::from(address) & mask
            }
            _ => false,
        }
    }
}

impl FromStr for Cidr {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, ()> {
        let text = text.trim();
        let (address, bits) = match text.split_once('/') {
            Some((address, bits)) => (address, Some(bits)),
            None => (text, None),
        };
        let base: IpAddr = address.parse().map_err(|_| ())?;
        let base = base.to_canonical();
        let most = if base.is_ipv4() { 32 } else { 128 };
        let bits = match bits {
            Some(bits) => bits.parse::<u8>().map_err(|_| ())?,
            None => most,
        };
        if bits > most {
            return Err(());
        }
        Ok(Self { base, bits })
    }
}

impl fmt::Display for Cidr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.base, self.bits)
    }
}

/// Ranges from text: one per line or comma-separated, `#` comments and
/// blank entries ignored. The entries that are not ranges, by number.
#[must_use]
pub fn parse_list(text: &str) -> (Vec<Cidr>, Vec<usize>) {
    let mut ranges = Vec::new();
    let mut bad = Vec::new();
    let entries = text
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .flat_map(|line| line.split(','))
        .map(str::trim)
        .filter(|entry| !entry.is_empty());
    for (index, entry) in entries.enumerate() {
        match entry.parse() {
            Ok(range) => ranges.push(range),
            Err(()) => bad.push(index + 1),
        }
    }
    (ranges, bad)
}
