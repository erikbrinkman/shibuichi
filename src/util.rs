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
    pub fn username(&self) -> &'a str {
        self.raw_username
    }

    /// Get the host
    pub fn host(&self) -> &'a str {
        self.raw_host
    }

    /// Get the path
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
