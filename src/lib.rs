//! Module for expanding a prompt
use log::warn;
use std::io;
use std::io::Write;
use std::mem;
pub mod util;

/// The domain of the upstream remote, defaults to [Domain::Git]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Domain {
    Git = 0,
    Github = 1,
    Gitlab = 2,
    BitBucket = 3,
    Azure = 4,
}

/// Trait for any information necessary to proper expansion
pub trait Info {
    fn git_exists(&mut self) -> bool;
    fn git_dirty(&mut self) -> bool;
    fn git_modified(&mut self) -> bool;
    fn git_staged(&mut self) -> bool;
    fn git_remote_domain(&mut self) -> Domain;
    fn git_remote_ahead(&mut self) -> usize;
    fn git_remote_behind(&mut self) -> usize;
    fn git_branch(&mut self) -> &str;
    fn git_stashes(&mut self) -> usize;
}

/// parses out a single command from an iterator
fn parse_command(chars: &mut (impl Iterator<Item = char> + Clone)) -> (Option<char>, Option<i64>) {
    let mut start = chars.clone();
    let mut command = None;
    let mut neg = false;
    let mut num = None;
    while match chars.next() {
        Some('-') if num == None && !neg => {
            neg = true;
            true
        }
        None | Some('-') => {
            warn!("invalid numeric escape");
            // rewind
            num = None;
            mem::swap(chars, &mut start);
            false
        }
        Some(chr) => match chr.to_digit(10) {
            Some(dig) => {
                num = match num {
                    None => Some(dig as i64),
                    Some(prev) => Some(prev * 10 + dig as i64),
                };
                true
            }
            None => {
                command = Some(chr);
                false
            }
        },
    } {}
    (command, num.map(|n| if neg { -n } else { n }))
}

/// a [Write] implemetor that discards everything
struct NullWrite;

impl Write for NullWrite {
    fn write(&mut self, buff: &[u8]) -> Result<usize, io::Error> {
        Ok(buff.len())
    }
    fn flush(&mut self) -> Result<(), io::Error> {
        Ok(())
    }
}

/// perform custom expansion on a character iterator
///
/// This terminates when it finds an unescaped version of char
fn expand_hit(
    info: &mut impl Info,
    chars: &mut (impl DoubleEndedIterator<Item = char> + Clone),
    out: &mut impl Write,
    stop: Option<char>,
) -> io::Result<()> {
    while let Some(chr) = chars.next() {
        if chr == '%' {
            match parse_command(chars) {
                // unterminated %
                (None, _) => write!(out, "%")?,
                // commands that don't have numeric arguments
                // NOTE for FfKk they can be trailed by {...} but since that can't contain special
                // characters we don't have to treat it differently
                (
                    Some(
                        ept @ ('%' | ')' | 'l' | 'M' | 'n' | 'y' | '#' | '?' | 'e' | 'h' | '!'
                        | 'i' | 'I' | 'j' | 'L' | 'T' | 't' | '@' | '*' | 'w' | 'W' | 'B'
                        | 'b' | 'E' | 'U' | 'u' | 'S' | 's' | 'm' | '_' | '^' | 'd' | '/'
                        | '~' | 'N' | 'c' | '.' | 'C' | 'v' | 'F' | 'f' | 'K' | 'k' | 'G'),
                    ),
                    None,
                ) => write!(out, "%{}", ept)?,
                // commands that have numeric arguments
                (
                    Some(
                        arg @ ('m' | '_' | '^' | 'd' | '/' | '~' | 'N' | 'c' | '.' | 'C' | 'v'
                        | 'F' | 'f' | 'K' | 'k' | 'G'),
                    ),
                    Some(num),
                ) => write!(out, "%{}{}", num, arg)?,
                // date string can have a special %D{...} after it
                (Some('D'), None) => {
                    write!(out, "%D")?;
                    match chars.next() {
                        Some('{') => {
                            write!(out, "{{")?;
                            for scan in chars.by_ref() {
                                write!(out, "{}", scan)?;
                                if scan == '}' {
                                    break;
                                }
                            }
                        }
                        None => (),
                        Some(_) => {
                            // rewind as not a {
                            chars.next_back();
                        }
                    }
                }
                // zsh escaped literals
                (Some('{'), num) => {
                    match num {
                        None => write!(out, "%{{")?,
                        Some(pad) => write!(out, "%{}{{", pad)?,
                    }
                    // scan until we see "%}" and ignore everything else
                    let mut last_paren = false;
                    for scan in chars.by_ref() {
                        write!(out, "{}", scan)?;
                        match scan {
                            '%' => last_paren = !last_paren,
                            '}' if last_paren => break,
                            _ => last_paren = false,
                        }
                    }
                }
                // truncation directives
                (Some(direc @ ('<' | '>' | '[')), num) => {
                    // NOTE we don't handle the non-standard deprecated form %[num...]
                    match num {
                        None => write!(out, "%{}", direc)?,
                        Some(pad) => write!(out, "%{}{}", pad, direc)?,
                    }
                    let tail = match direc {
                        '<' => '<',
                        '>' => '>',
                        '[' => ']',
                        _ => panic!(),
                    };
                    for scan in chars.by_ref() {
                        write!(out, "{}", scan)?;
                        if scan == tail {
                            break;
                        }
                    }
                }
                // conditionals
                (Some('('), num) => {
                    match (chars.next(), chars.next()) {
                        // built-in ternaries
                        (
                            Some(
                                pred @ ('!' | '#' | '?' | '_' | 'C' | '/' | 'c' | '.' | '~' | 'D'
                                | 'd' | 'e' | 'g' | 'j' | 'L' | 'l' | 'S' | 'T' | 't' | 'v'
                                | 'V' | 'w'),
                            ),
                            Some(delim),
                        ) => {
                            match num {
                                None => write!(out, "%(")?,
                                Some(pad) => write!(out, "%{}(", pad)?,
                            };
                            write!(out, "{}{}", pred, delim)?;
                            expand_hit(info, chars, out, Some(delim))?;
                            write!(out, "{}", delim)?;
                            expand_hit(info, chars, out, Some(')'))?;
                            write!(out, ")")?;
                        }
                        // custom ternaries
                        (
                            Some(pred @ ('G' | 'y' | 'm' | 's' | 'o' | 'p' | 'q' | 'x')),
                            Some(delim),
                        ) => {
                            let num = num.unwrap_or(0);
                            if match pred {
                                'G' => info.git_exists(),
                                'y' => info.git_dirty(),
                                'm' => info.git_modified(),
                                's' => info.git_staged(),
                                'o' => info.git_remote_domain() as i64 == num,
                                'p' => info.git_remote_ahead() as i64 >= num,
                                'q' => info.git_remote_behind() as i64 >= num,
                                'x' => info.git_stashes() as i64 >= num,
                                _ => panic!("unhandled custom ternary: '{}'", pred),
                            } {
                                expand_hit(info, chars, out, Some(delim))?;
                                expand_hit(info, chars, &mut NullWrite, Some(')'))?;
                            } else {
                                expand_hit(info, chars, &mut NullWrite, Some(delim))?;
                                expand_hit(info, chars, out, Some(')'))?;
                            };
                        }
                        // missing characters
                        (first, second) => {
                            match (first, second) {
                                (Some(flag), Some(_)) => warn!("invalid ternary flag: '{}'", flag),
                                _ => warn!("prompt ended during a ternary sequence"),
                            }
                            match num {
                                None => write!(out, "%(")?,
                                Some(pad) => write!(out, "%{}(", pad)?,
                            };
                            if first.is_some() {
                                chars.next_back();
                            }
                            if second.is_some() {
                                chars.next_back();
                            }
                        }
                    }
                }
                // custom expansions
                (Some('r'), None) => write!(out, "{}", info.git_branch())?,
                (Some('p'), None) => write!(out, "{}", info.git_remote_ahead())?,
                (Some('q'), None) => write!(out, "{}", info.git_remote_behind())?,
                (Some('x'), None) => write!(out, "{}", info.git_stashes())?,
                // any unhandled escape
                (Some(nxt), None) => {
                    warn!("use of unknown escape: '%{}'", nxt);
                    write!(out, "%{}", nxt)?;
                }
                (Some(nxt), Some(num)) => {
                    warn!("use of unknown escape: '%{}{}'", num, nxt);
                    write!(out, "%{}{}", num, nxt)?;
                }
            };
        } else if Some(chr) == stop {
            return Ok(());
        } else {
            write!(out, "{}", chr)?;
        }
    }
    if let Some(chr) = stop {
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("no matching ternary close: '{}'", chr),
        ))
    } else {
        Ok(())
    }
}

/// Expand a shibuichi prompt string
///
/// A shibuichi prompt string is a superset of [zsh prompt
/// expansions](https://zsh.sourceforge.io/Doc/Release/Prompt-Expansion.html). Standard zsh prompt
/// expansions are left alone, and this just adds several other expansions:
///
/// - `%r` - The short name of the current git branch. If not in a git repository this will be
///   empty.
/// - `%p` - An integer for the number of commits the current branch is *ahead* of its remote
///   tracking branch. If there is no remote tracking branch, this will render as a 0.
/// - `%q` - An integer for the number of commits the current branch is *behind* of its remote
///   tracking branch. If there is no remote tracking branch, this will render as a 0.
/// - `%x` - An integer for the number of current stashes.
///
/// In addition, this also adds a few more codes to the conditional substring expansion
/// `%(x.true-text.false-text)`. These codes are:
///
/// - `G` - True if in a git repository.
/// - `y` - True if the git repository is dirty.
/// - `m` - True if the git repository has modified files.
/// - `s` - True if the git repository has staged files.
/// - `o` - True if the domain of the remote tracking origin number matches `n`, where 0 is
/// reserved for all other domains (see [Domain]):
///    1. `github.com`
///    2. `gitlab.com`
///    3. `bitbucket.org`
///    4. `dev.azure.com`
/// - `p` - True if the remote tracking branch is at least `n` commits *ahead* of the current branch.
/// - `q` - True if the remote tracking branch is at least `n` commits *behind* of the current
/// branch.
/// - `x` - True if there are at least `n` stashes.
pub fn expand(
    prompt: impl AsRef<str>,
    info: &mut impl Info,
    out: &mut impl Write,
) -> io::Result<()> {
    let mut chars = prompt.as_ref().chars();
    expand_hit(info, &mut chars, out, None)
}

#[cfg(test)]
mod tests {
    use super::{expand, parse_command, Domain, Info};
    use std::str;

    #[test]
    fn easy_command() {
        let mut iter = "d".chars();
        let res = parse_command(&mut iter);
        assert_eq!(res, (Some('d'), None));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn pos_num_command() {
        let mut iter = "5d".chars();
        let res = parse_command(&mut iter);
        assert_eq!(res, (Some('d'), Some(5)));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn neg_num_command() {
        let mut iter = "-25d".chars();
        let res = parse_command(&mut iter);
        assert_eq!(res, (Some('d'), Some(-25)));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn double_neg() {
        let mut iter = "-25-d".chars();
        let res = parse_command(&mut iter);
        assert_eq!(res, (None, None));
        assert_eq!(iter.next(), Some('-'));
    }

    struct NoInfo;

    impl Info for NoInfo {
        fn git_exists(&mut self) -> bool {
            false
        }
        fn git_dirty(&mut self) -> bool {
            false
        }
        fn git_modified(&mut self) -> bool {
            false
        }
        fn git_staged(&mut self) -> bool {
            false
        }
        fn git_remote_domain(&mut self) -> Domain {
            Domain::Git
        }
        fn git_remote_ahead(&mut self) -> usize {
            0
        }
        fn git_remote_behind(&mut self) -> usize {
            0
        }
        fn git_branch(&mut self) -> &str {
            ""
        }
        fn git_stashes(&mut self) -> usize {
            0
        }
    }

    #[test]
    fn empty() {
        let mut result = Vec::new();
        expand("", &mut NoInfo, &mut result).unwrap();
        assert_eq!(str::from_utf8(&result).unwrap(), "");
    }

    #[test]
    fn builtin() {
        let builtins = "%% %-3~ %D{%f-%K-%L} %{seq%3G%} %v %(C.a.%(g#b#c)) %10<...<%~%<<%# ";
        let mut result = Vec::new();
        expand(builtins, &mut NoInfo, &mut result).unwrap();
        assert_eq!(str::from_utf8(&result).unwrap(), builtins);
    }

    #[test]
    fn empty_expansions() {
        let mut result = Vec::new();
        expand("r%r a%p b%q s%x", &mut NoInfo, &mut result).unwrap();
        assert_eq!(str::from_utf8(&result).unwrap(), "r a0 b0 s0");
    }

    #[test]
    fn empty_conditionals() {
        let mut result = Vec::new();
        expand(
            "%(G.e.n) %(y.d.n)%(m#m#n)%(s.s.n) %(o.d.o)%1(o,g,n) %(p.a.n)%1(p.o.n) %(q.b.n)%1(q.o.n) %(x.s.n)%1(x.o.n)",
            &mut NoInfo,
            &mut result,
        )
        .unwrap();
        assert_eq!(str::from_utf8(&result).unwrap(), "n nnn dn an bn sn");
    }

    struct TestInfo {
        dirty: bool,
        modified: bool,
        staged: bool,
        domain: Domain,
        ahead: usize,
        behind: usize,
        branch: &'static str,
        stashes: usize,
    }

    impl Info for TestInfo {
        fn git_exists(&mut self) -> bool {
            true
        }
        fn git_dirty(&mut self) -> bool {
            self.dirty
        }
        fn git_modified(&mut self) -> bool {
            self.modified
        }
        fn git_staged(&mut self) -> bool {
            self.staged
        }
        fn git_remote_domain(&mut self) -> Domain {
            self.domain
        }
        fn git_remote_ahead(&mut self) -> usize {
            self.ahead
        }
        fn git_remote_behind(&mut self) -> usize {
            self.behind
        }
        fn git_branch(&mut self) -> &str {
            self.branch
        }
        fn git_stashes(&mut self) -> usize {
            self.stashes
        }
    }

    #[test]
    fn manual() {
        let mut result = Vec::new();
        let prompt = "%(G.%(y.d.)%(m.m.)%(s.s.) %0(o.g.)%1(o.h.)%2(o.l.)%3(o.b.)%4(o.a.) %1(p.%2(p.^%p.^).)%1(q.%2(q.v%q.v).)%1(x.%2(x.s%x.s).) %r.)";

        result.clear();
        let mut info = TestInfo {
            dirty: true,
            modified: true,
            staged: true,
            domain: Domain::Github,
            ahead: 2,
            behind: 1,
            branch: "main",
            stashes: 1,
        };
        expand(prompt, &mut info, &mut result).unwrap();
        assert_eq!(str::from_utf8(&result).unwrap(), "dms h ^2vs main");

        result.clear();
        let mut info = TestInfo {
            dirty: true,
            modified: false,
            staged: false,
            domain: Domain::Azure,
            ahead: 0,
            behind: 2,
            branch: "feature",
            stashes: 3,
        };
        expand(prompt, &mut info, &mut result).unwrap();
        assert_eq!(str::from_utf8(&result).unwrap(), "d a v2s3 feature");
    }
}
