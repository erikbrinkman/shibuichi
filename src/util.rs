//! Utilities for prompt expansion

/// A parsed SCP style url
///
/// SCP urls don't have an RFC, but this function looks for a url of the form:
/// `<username>@<host>:<path>` and can be specified via the regex `^([^@:]*)@([^@:]*):([^@:]*)$`
#[derive(Debug, PartialEq, Eq)]
pub struct ParsedScpUrl<'a> {
    raw_username: &'a str,
    raw_host: &'a str,
    raw_path: &'a str,
}

impl<'a> ParsedScpUrl<'a> {
    /// Parse an SCP style url
    #[must_use]
    pub fn parse(raw: &'a str) -> Option<Self> {
        let (raw_username, rest) = raw.split_once('@')?;
        let (raw_host, raw_path) = rest.split_once(':')?;
        if raw_username.contains(':')
            || raw_host.contains('@')
            || raw_path.contains(':')
            || raw_path.contains('@')
        {
            None
        } else {
            Some(ParsedScpUrl {
                raw_username,
                raw_host,
                raw_path,
            })
        }
    }

    /// Get the user name
    #[must_use]
    pub fn username(&self) -> &'a str {
        self.raw_username
    }

    /// Get the host
    #[must_use]
    pub fn host(&self) -> &'a str {
        self.raw_host
    }

    /// Get the path
    #[must_use]
    pub fn path(&self) -> &'a str {
        self.raw_path
    }
}

#[cfg(test)]
mod tests {
    use super::ParsedScpUrl;

    #[test]
    fn git_standard() {
        let parsed = ParsedScpUrl::parse("git@github.com:username/repo.git").unwrap();
        assert_eq!(parsed.username(), "git");
        assert_eq!(parsed.host(), "github.com");
        assert_eq!(parsed.path(), "username/repo.git");
    }

    #[test]
    fn degenerate() {
        let parsed = ParsedScpUrl::parse("@:").unwrap();
        assert_eq!(parsed.username(), "");
        assert_eq!(parsed.host(), "");
        assert_eq!(parsed.path(), "");
    }

    #[test]
    fn parse_errors() {
        assert_eq!(ParsedScpUrl::parse(""), None);
        assert_eq!(ParsedScpUrl::parse(":"), None);
        assert_eq!(ParsedScpUrl::parse("@"), None);
        assert_eq!(ParsedScpUrl::parse("g:t@github.com:path"), None);
        assert_eq!(ParsedScpUrl::parse("git@github@com:path"), None);
        assert_eq!(ParsedScpUrl::parse("git@github.com:p:th"), None);
        assert_eq!(ParsedScpUrl::parse("git@github.com:p@th"), None);
    }
}

/// Generic trait for anything that "has" chars
///
/// This is similar to the [`Pattern`][std::str::pattern::Pattern] trait, except that that's still
/// experimental, and currently models the inverse relation of this, but in principle these are
/// bijections.
pub trait ContainsChar {
    /// Return true if chr is present in self
    fn contains(&self, chr: char) -> bool;
}

impl ContainsChar for char {
    fn contains(&self, chr: char) -> bool {
        self == &chr
    }
}

impl ContainsChar for str {
    fn contains(&self, chr: char) -> bool {
        self.contains(chr)
    }
}

impl ContainsChar for [char] {
    fn contains(&self, chr: char) -> bool {
        self.contains(&chr)
    }
}

impl<const N: usize> ContainsChar for [char; N] {
    fn contains(&self, chr: char) -> bool {
        self.as_slice().contains(&chr)
    }
}

impl<'a, T: ContainsChar> ContainsChar for &'a T {
    fn contains(&self, chr: char) -> bool {
        (*self).contains(chr)
    }
}
