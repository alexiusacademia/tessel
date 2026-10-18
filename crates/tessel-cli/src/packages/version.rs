//! Versions of libraries (`1.2.0`) and what a program asks for (`"1.2"`).
//!
//! A requirement is a version that its later, compatible versions also
//! meet, the way most package managers read it: `"1.2"` takes 1.2.0 and
//! anything after it before 2.0.0; `"0.3"` takes 0.3.x (before 1.0, a new
//! minor version may break things); `"1.2.4"` takes 1.2.4 and later 1.x.
//! `"=1.2.4"` takes that version only.

use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// A version written with all three numbers, like `1.2.0`.
    pub fn parse(text: &str) -> Result<Version, String> {
        let parts: Vec<&str> = text.split('.').collect();
        let nums: Option<Vec<u64>> = parts.iter().map(|p| number(p)).collect();
        match nums.as_deref() {
            Some([major, minor, patch]) => Ok(Version { major: *major, minor: *minor, patch: *patch }),
            _ => Err(format!("`{text}` isn't a version; write three numbers, like 1.0.0")),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A number with no sign, spaces or leading zeros.
fn number(p: &str) -> Option<u64> {
    if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) || (p.len() > 1 && p.starts_with('0')) {
        return None;
    }
    p.parse().ok()
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Req {
    /// As written, for showing it.
    text: String,
    base: Version,
    /// How many numbers were written (1 to 3).
    parts: usize,
    exact: bool,
}

impl Req {
    pub fn parse(text: &str) -> Result<Req, String> {
        let t = text.trim();
        let (exact, rest) = match t.strip_prefix('=') {
            Some(r) => (true, r.trim()),
            None => (false, t.strip_prefix('^').unwrap_or(t).trim()),
        };
        let nums: Option<Vec<u64>> = rest.split('.').map(number).collect();
        let bad = || format!("`{text}` isn't a version to ask for; write one like \"1.2\" (1.2 or a later 1.x) or \"=1.2.0\" (that one only)");
        let nums = nums.filter(|n| !n.is_empty() && n.len() <= 3).ok_or_else(bad)?;
        if exact && nums.len() != 3 {
            return Err(bad());
        }
        let get = |i: usize| nums.get(i).copied().unwrap_or(0);
        Ok(Req { text: t.to_string(), base: Version { major: get(0), minor: get(1), patch: get(2) }, parts: nums.len(), exact })
    }

    /// The requirement for a version and its compatible successors, as
    /// `tessel add` writes it: `"1.2"`, or `"0.3"` before 1.0.
    pub fn compatible(v: Version) -> Req {
        let text = if v.major == 0 && v.minor == 0 { v.to_string() } else { format!("{}.{}", v.major, v.minor) };
        Req::parse(&text).unwrap()
    }

    pub fn matches(&self, v: Version) -> bool {
        let b = self.base;
        if self.exact {
            return v == b;
        }
        if v < b {
            return false;
        }
        if b.major > 0 || self.parts == 1 {
            v.major == b.major
        } else if b.minor > 0 || self.parts == 2 {
            v.major == 0 && v.minor == b.minor
        } else {
            v == b
        }
    }
}

impl fmt::Display for Req {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn requirements_take_compatible_versions() {
        let r = Req::parse("1.2").unwrap();
        assert!(r.matches(v("1.2.0")) && r.matches(v("1.9.3")));
        assert!(!r.matches(v("1.1.9")) && !r.matches(v("2.0.0")));
        let r = Req::parse("0.3").unwrap();
        assert!(r.matches(v("0.3.7")) && !r.matches(v("0.4.0")) && !r.matches(v("0.2.9")));
        let r = Req::parse("0.0.3").unwrap();
        assert!(r.matches(v("0.0.3")) && !r.matches(v("0.0.4")));
        let r = Req::parse("1").unwrap();
        assert!(r.matches(v("1.0.0")) && r.matches(v("1.5.0")) && !r.matches(v("2.0.0")));
        let r = Req::parse("=1.2.4").unwrap();
        assert!(r.matches(v("1.2.4")) && !r.matches(v("1.2.5")));
        assert!(Req::parse("1.x").is_err() && Req::parse("=1.2").is_err() && Req::parse("").is_err());
        assert_eq!(Req::compatible(v("1.4.2")).to_string(), "1.4");
        assert_eq!(Req::compatible(v("0.2.1")).to_string(), "0.2");
        assert!(Version::parse("1.02.0").is_err() && Version::parse("1.2").is_err());
        assert!(v("1.10.0") > v("1.9.9"));
    }
}
