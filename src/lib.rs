//! Module for expanding a prompt
#![warn(missing_docs)]
pub mod util;

use nom::{
    branch::alt,
    bytes::complete::{escaped, is_not, tag, take_until},
    character::complete::{anychar, char, i64, none_of, one_of},
    combinator::{map, opt},
    multi::{many0, separated_list0},
    sequence::{delimited, pair, preceded, separated_pair, terminated, tuple},
    Finish, IResult,
};
use std::collections::VecDeque;
use std::io;
use std::io::Write;
use std::path;
use std::path::Path;
use util::ContainsChar;

#[derive(Debug, PartialEq)]
struct Escape(char);

#[derive(Debug, PartialEq)]
struct NumericEscape(Option<i64>, char);

#[derive(Debug, PartialEq)]
struct DateFormat<'a>(&'a str);

#[derive(Debug, PartialEq)]
struct NamedColor<'a> {
    num: Option<i64>,
    code: char,
    name: &'a str,
}

#[derive(Debug, PartialEq)]
struct PathPrefix<'a> {
    num: Option<i64>,
    code: char,
    delim: char,
    prefix_subs: Vec<(&'a str, &'a str)>,
}

#[derive(Debug, PartialEq)]
struct EscapeLiteral<'a>(&'a str);

#[derive(Debug, PartialEq)]
struct Conditional<'a> {
    num: Option<i64>,
    code: char,
    delim: char,
    true_branch: Vec<Element<'a>>,
    false_branch: Vec<Element<'a>>,
}

#[derive(Debug, PartialEq)]
struct AdvancedConditional<'a> {
    code: char,
    delim: char,
    // NOTE we could make this slightly more efficient with a jagged array
    conditions: Vec<Vec<Element<'a>>>,
}

#[derive(Debug, PartialEq)]
struct Truncation<'a> {
    num: Option<i64>,
    code: char,
    replacement: &'a str,
}

#[derive(Debug, PartialEq)]
enum Element<'a> {
    Character(char),
    Escape(Escape),
    NumericEscape(NumericEscape),
    DateFormat(DateFormat<'a>),
    NamedColor(NamedColor<'a>),
    EscapeLiteral(EscapeLiteral<'a>),
    Conditional(Conditional<'a>),
    AdvancedConditional(AdvancedConditional<'a>),
    Truncation(Truncation<'a>),
    PathPrefix(PathPrefix<'a>),
}

fn escape(input: &str) -> IResult<&str, Escape> {
    let chars = one_of("%)lMny#?eh!iIjLTt@*wWBbEUuSsDrpqx");
    map(preceded(char('%'), chars), Escape)(input)
}

fn numeric_escape(input: &str) -> IResult<&str, NumericEscape> {
    let pat = preceded(char('%'), pair(opt(i64), one_of("m_^d/~Nc.CvFfKkG")));
    map(pat, |(num, chr)| NumericEscape(num, chr))(input)
}

fn date_format(input: &str) -> IResult<&str, DateFormat> {
    map(delimited(tag("%D{"), is_not("}"), char('}')), DateFormat)(input)
}

fn named_color(input: &str) -> IResult<&str, NamedColor> {
    let (input, (_, num, code, _, name, _)) = tuple((
        char('%'),
        opt(i64),
        one_of("FK"),
        char('{'),
        is_not("}"),
        char('}'),
    ))(input)?;
    Ok((input, NamedColor { num, code, name }))
}

fn path_prefix(input: &str) -> IResult<&str, PathPrefix> {
    let (input, (_, num, code, _, delim)) =
        tuple((char('%'), opt(i64), one_of("d/"), char('{'), anychar))(input)?;
    let delim_str = format!("{}}}", delim);
    let (input, prefix_subs) = terminated(
        separated_list0(
            char(delim),
            separated_pair(
                alt((escaped(none_of(&*delim_str), '\\', anychar), tag(""))),
                char(delim),
                escaped(none_of(&*delim_str), '\\', anychar),
            ),
        ),
        char('}'),
    )(input)?;
    Ok((
        input,
        PathPrefix {
            num,
            code,
            delim,
            prefix_subs,
        },
    ))
}

fn escape_literal(input: &str) -> IResult<&str, EscapeLiteral> {
    // NOTE this will fail if we see `%{%%}`, but maybe that's okay?
    map(
        delimited(tag("%{"), take_until("%}"), tag("%}")),
        EscapeLiteral,
    )(input)
}

fn elements_until<'a>(
    stop: impl ContainsChar + 'a,
) -> impl Fn(&str) -> IResult<&str, (Vec<Element>, char)> + 'a {
    move |mut inp| {
        let mut elems = Vec::new();
        loop {
            let (nxt, elem) = element(inp)?;
            inp = nxt;
            match elem {
                Element::Character(chr) if stop.contains(chr) => {
                    return Ok((inp, (elems, chr)));
                }
                _ => {
                    elems.push(elem);
                }
            }
        }
    }
}

fn conditional(input: &str) -> IResult<&str, Conditional> {
    let (input, (_, num, _, code, delim)) = tuple((
        char('%'),
        opt(i64),
        char('('),
        one_of("!#?_C/c.~DdegjLlSTtvVwGymsopqx"),
        anychar,
    ))(input)?;
    let (input, (true_branch, _)) = elements_until(delim)(input)?;
    let (input, (false_branch, _)) = elements_until(')')(input)?;
    Ok((
        input,
        Conditional {
            num,
            code,
            delim,
            true_branch,
            false_branch,
        },
    ))
}

fn advanced_conditional(input: &str) -> IResult<&str, AdvancedConditional> {
    let (mut input, (code, delim)) = preceded(tag("%("), pair(one_of("opqx"), anychar))(input)?;
    let delims = [delim, ')'];
    let mut conditions = Vec::new();
    let mut found = delim;
    while found != ')' {
        let (inp, (elems, fnd)) = elements_until(&delims)(input)?;
        conditions.push(elems);
        input = inp;
        found = fnd;
    }
    Ok((
        input,
        AdvancedConditional {
            code,
            delim,
            conditions,
        },
    ))
}

fn truncation(input: &str) -> IResult<&str, Truncation> {
    let (input, (num, code)) = preceded(char('%'), pair(opt(i64), one_of("<>")))(input)?;
    let blocked = format!("\\{}", code);
    let (input, replacement) = terminated(
        alt((escaped(none_of(&*blocked), '\\', anychar), tag(""))),
        char(code),
    )(input)?;
    Ok((
        input,
        Truncation {
            num,
            code,
            replacement,
        },
    ))
}

fn element(input: &str) -> IResult<&str, Element> {
    alt((
        map(truncation, Element::Truncation),
        map(advanced_conditional, Element::AdvancedConditional),
        map(conditional, Element::Conditional),
        map(date_format, Element::DateFormat),
        map(named_color, Element::NamedColor),
        map(path_prefix, Element::PathPrefix),
        map(escape_literal, Element::EscapeLiteral),
        map(numeric_escape, Element::NumericEscape),
        map(escape, Element::Escape),
        map(anychar, Element::Character),
    ))(input)
}

/// The domain of the upstream remote, defaults to [Domain::Git]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Domain {
    /// Generic domain (not one of the others)
    Git = 0,
    /// `github.com`
    Github = 1,
    /// `gitlab.com`
    Gitlab = 2,
    /// `bitbucket.org`
    BitBucket = 3,
    /// `dev.azure.com`
    Azure = 4,
}

/// Trait for any information necessary to proper expansion
pub trait Info {
    /// Get the current path for display
    fn current_path(&mut self) -> &Path;
    /// Return true if inside a git repository
    fn git_exists(&mut self) -> bool;
    /// Return true if git repo is dirty
    fn git_dirty(&mut self) -> bool;
    /// Return true if git repo has modified files
    fn git_modified(&mut self) -> bool;
    /// Return true if git repo has staged files
    fn git_staged(&mut self) -> bool;
    /// Get remote domain of git repo
    fn git_remote_domain(&mut self) -> Domain;
    /// Get number of commits current branch is ahead of remote
    fn git_remote_ahead(&mut self) -> usize;
    /// Get number of commits current branch is behind remote
    fn git_remote_behind(&mut self) -> usize;
    /// Get name of the current git branch
    fn git_branch(&mut self) -> &str;
    /// Get the number of current stashes
    fn git_stashes(&mut self) -> usize;
}

trait Render {
    fn render(&self, out: &mut impl Write, info: &mut impl Info) -> io::Result<()>;
}

impl Render for Escape {
    fn render(&self, out: &mut impl Write, info: &mut impl Info) -> io::Result<()> {
        match self {
            Escape('r') => write!(out, "{}", info.git_branch()),
            Escape('p') => write!(out, "{}", info.git_remote_ahead()),
            Escape('q') => write!(out, "{}", info.git_remote_behind()),
            Escape('x') => write!(out, "{}", info.git_stashes()),
            Escape(chr) => write!(out, "%{}", chr),
        }
    }
}

impl Render for NumericEscape {
    fn render(&self, out: &mut impl Write, _: &mut impl Info) -> io::Result<()> {
        match self {
            NumericEscape(Some(num), chr) => write!(out, "%{}{}", num, chr),
            NumericEscape(None, chr) => write!(out, "%{}", chr),
        }
    }
}

impl<'a> Render for DateFormat<'a> {
    fn render(&self, out: &mut impl Write, _: &mut impl Info) -> io::Result<()> {
        let DateFormat(format) = self;
        write!(out, "%D{{{}}}", format)
    }
}

impl<'a> Render for NamedColor<'a> {
    fn render(&self, out: &mut impl Write, _: &mut impl Info) -> io::Result<()> {
        match self.num {
            Some(num) => write!(out, "%{}{}{{{}}}", num, self.code, self.name),
            None => write!(out, "%{}{{{}}}", self.code, self.name),
        }
    }
}

impl<'a> Render for PathPrefix<'a> {
    fn render(&self, out: &mut impl Write, info: &mut impl Info) -> io::Result<()> {
        let mut wd = info.current_path().to_owned();
        for (alias, prefix) in &self.prefix_subs {
            if let Ok(stripped) = wd.strip_prefix(prefix) {
                wd = [alias.as_ref(), stripped].iter().collect();
            }
        }
        match self.num.unwrap_or(0) {
            0 => (),
            num @ 1..=i64::MAX => {
                let mut comps = VecDeque::new();
                for comp in wd.iter() {
                    comps.push_back(comp);
                    if comps.len() == (num + 1) as usize {
                        comps.pop_front();
                    }
                }
                wd = comps.iter().collect();
            }
            num @ i64::MIN..=-1 => {
                let mut comps = Vec::new();
                for comp in wd.iter() {
                    comps.push(comp);
                    if comps.len() == -num as usize {
                        break;
                    }
                }
                wd = comps.iter().collect();
            }
        }
        let lossy = wd.to_string_lossy();
        let mut lossy_chars = lossy.chars();
        let output =
            if lossy_chars.next() == Some(path::MAIN_SEPARATOR) && lossy_chars.next().is_none() {
                &lossy
            } else {
                lossy.strip_suffix(path::MAIN_SEPARATOR).unwrap_or(&lossy)
            };
        write!(out, "{}", output)
    }
}

impl<'a> Render for EscapeLiteral<'a> {
    fn render(&self, out: &mut impl Write, _: &mut impl Info) -> io::Result<()> {
        let EscapeLiteral(literal) = self;
        write!(out, "%{{{}%}}", literal)
    }
}

impl<T: Render> Render for [T] {
    fn render(&self, out: &mut impl Write, info: &mut impl Info) -> io::Result<()> {
        for elem in self.iter() {
            elem.render(out, info)?;
        }
        Ok(())
    }
}

impl<'a> Render for Conditional<'a> {
    fn render(&self, out: &mut impl Write, info: &mut impl Info) -> io::Result<()> {
        match self.code {
            code @ ('G' | 'y' | 'm' | 's' | 'o' | 'p' | 'q' | 'x') => {
                let num = self.num.unwrap_or(0);
                if match code {
                    'G' => info.git_exists(),
                    'y' => info.git_dirty(),
                    'm' => info.git_modified(),
                    's' => info.git_staged(),
                    'o' => info.git_remote_domain() as i64 == num,
                    'p' => info.git_remote_ahead() as i64 >= num,
                    'q' => info.git_remote_behind() as i64 >= num,
                    'x' => info.git_stashes() as i64 >= num,
                    _ => panic!(),
                } {
                    self.true_branch.render(out, info)
                } else {
                    self.false_branch.render(out, info)
                }
            }
            code => {
                write!(out, "%")?;
                match self.num {
                    Some(num) => write!(out, "{}", num)?,
                    None => (),
                };
                write!(out, "({}{}", code, self.delim)?;
                self.true_branch.render(out, info)?;
                write!(out, "{}", self.delim)?;
                self.false_branch.render(out, info)?;
                write!(out, ")")
            }
        }
    }
}

impl<'a> Render for AdvancedConditional<'a> {
    fn render(&self, out: &mut impl Write, info: &mut impl Info) -> io::Result<()> {
        let ind = match self.code {
            'o' => info.git_remote_domain() as usize,
            'p' => info.git_remote_ahead(),
            'q' => info.git_remote_behind(),
            'x' => info.git_stashes(),
            _ => panic!(),
        };
        if ind < self.conditions.len() {
            self.conditions[ind].render(out, info)
        } else {
            self.conditions.last().unwrap().render(out, info)
        }
    }
}

impl<'a> Render for Truncation<'a> {
    fn render(&self, out: &mut impl Write, _: &mut impl Info) -> io::Result<()> {
        match self.num {
            Some(num) => write!(
                out,
                "%{}{}{}{}",
                num, self.code, self.replacement, self.code
            ),
            None => write!(out, "%{}{}{}", self.code, self.replacement, self.code),
        }
    }
}

impl<'a> Render for Element<'a> {
    fn render(&self, out: &mut impl Write, info: &mut impl Info) -> io::Result<()> {
        match self {
            Element::Character(chr) => write!(out, "{}", chr),
            Element::Escape(esc) => esc.render(out, info),
            Element::NumericEscape(num_esc) => num_esc.render(out, info),
            Element::DateFormat(dfmt) => dfmt.render(out, info),
            Element::NamedColor(color) => color.render(out, info),
            Element::EscapeLiteral(esc) => esc.render(out, info),
            Element::Conditional(cond) => cond.render(out, info),
            Element::AdvancedConditional(cond) => cond.render(out, info),
            Element::Truncation(trunc) => trunc.render(out, info),
            Element::PathPrefix(path) => path.render(out, info),
        }
    }
}

/// Parses the input into a vector of elements
///
/// This is the intermediate representation before re-rendering.
fn parse(input: &str) -> Vec<Element> {
    // NOTE unwrap should be safe because we always accept an arbitrary character
    let (rem, elems) = many0(element)(input).finish().unwrap();
    // NOTE should also be safe for same reason
    assert_eq!(rem, "");
    elems
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
///
/// Finally the directory command is extended in a slightly breaking change, where
///
/// - `%d{:replacement:prefix:...}`
/// - `%/{:replacement:prefix:...}` - takes multiple prefix-replacement pairs to apply to the path.
///   Any delimiter character can be specified, and backslash escapes are honored, but no other
///   expansion happens. `%~` and `%/{:~:$HOME}` should be roughly equivalent. Note, that this
///   tries the `PWD` variable first, and if it's missing uses a canonical working directory, which
///   may be different than that output by `%/`.
pub fn expand(
    prompt: impl AsRef<str>,
    info: &mut impl Info,
    out: &mut impl Write,
) -> io::Result<()> {
    // NOTE if we use fold_many0 we could avoid this outer vector allocation, but then it would
    // require much better io error handling
    let elems = parse(prompt.as_ref());
    elems.render(out, info)
}

#[cfg(test)]
mod parse_tests {
    use super::{
        parse, AdvancedConditional, Conditional, DateFormat, Element, Escape, EscapeLiteral,
        NamedColor, NumericEscape, PathPrefix, Truncation,
    };

    #[test]
    fn simple_escapes() {
        let expected = [
            Element::Escape(Escape('%')),
            Element::Character(' '),
            Element::NumericEscape(NumericEscape(Some(-3), '~')),
            Element::Character(' '),
            Element::Escape(Escape('D')),
            Element::Character(' '),
            Element::NumericEscape(NumericEscape(None, 'v')),
        ];
        let elems = parse("%% %-3~ %D %v");
        assert_eq!(elems, expected);
    }

    #[test]
    fn date_format() {
        let expected = [Element::DateFormat(DateFormat("%H:%M:%S.%."))];
        let elems = parse("%D{%H:%M:%S.%.}");
        assert_eq!(elems, expected);
    }

    #[test]
    fn named_color() {
        let expected = [
            Element::NamedColor(NamedColor {
                num: None,
                code: 'F',
                name: "red",
            }),
            Element::Character(' '),
            Element::NamedColor(NamedColor {
                num: Some(0),
                code: 'K',
                name: "black",
            }),
        ];
        let elems = parse("%F{red} %0K{black}");
        assert_eq!(elems, expected);
    }

    #[test]
    fn path_prefix() {
        let expected = [
            Element::PathPrefix(PathPrefix {
                num: None,
                code: 'd',
                delim: '.',
                prefix_subs: vec![],
            }),
            Element::Character(' '),
            Element::PathPrefix(PathPrefix {
                num: Some(-2),
                code: '/',
                delim: ':',
                prefix_subs: vec![("home", "/home/user")],
            }),
        ];
        let elems = parse("%d{.} %-2/{:home:/home/user}");
        assert_eq!(elems, expected);
    }

    #[test]
    fn escape_literal() {
        let expected = [Element::EscapeLiteral(EscapeLiteral("$terminfo[smacs]%G"))];
        let elems = parse("%{$terminfo[smacs]%G%}");
        assert_eq!(elems, expected);
    }

    #[test]
    fn truncation() {
        let expected = [
            Element::Truncation(Truncation {
                num: Some(8),
                code: '<',
                replacement: "..",
            }),
            Element::Truncation(Truncation {
                num: None,
                code: '<',
                replacement: "",
            }),
            Element::Character(' '),
            Element::Truncation(Truncation {
                num: None,
                code: '>',
                replacement: "\\>",
            }),
        ];
        let elems = parse("%8<..<%<< %>\\>>");
        assert_eq!(elems, expected);
    }

    #[test]
    fn conditional() {
        let expected = [Element::Conditional(Conditional {
            num: None,
            code: 'C',
            delim: '.',
            true_branch: vec![Element::Character('a')],
            false_branch: vec![Element::Conditional(Conditional {
                num: Some(1),
                code: 'g',
                delim: '#',
                true_branch: vec![Element::Character('b')],
                false_branch: vec![Element::Character('c')],
            })],
        })];
        let elems = parse("%(C.a.%1(g#b#c))");
        assert_eq!(elems, expected);
    }

    #[test]
    fn advanced_conditional() {
        let expected = [Element::AdvancedConditional(AdvancedConditional {
            code: 'o',
            delim: '.',
            conditions: vec![
                vec![Element::Character('a')],
                vec![Element::Character('b')],
                vec![Element::Character('c')],
            ],
        })];
        let elems = parse("%(o.a.b.c)");
        assert_eq!(elems, expected);
    }
}

#[cfg(test)]
mod expand_tests {
    use super::{expand, Domain, Info};
    use std::path::{Path, PathBuf};
    use std::str;

    struct NoInfo;

    impl Info for NoInfo {
        fn current_path(&mut self) -> &Path {
            "".as_ref()
        }

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
        let builtins =
            "%% %-3~ %D{%f-%K-%L} %F{red} %{seq%3G%} %v %(C.a.%(g#b#c)) %10<...<%~%<<%# ";
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
            "%(G.e.n) %(y.d.n)%(m#m#n)%(s.s.n) %(o.d.o)%1(o,g,n)%(o.d.g._) %(p.a.n)%1(p.o.n) %(q.b.n)%1(q.o.n) %(x.s.n)%1(x.o.n)",
            &mut NoInfo,
            &mut result,
        )
        .unwrap();
        assert_eq!(str::from_utf8(&result).unwrap(), "n nnn dnd an bn sn");
    }

    struct TestInfo {
        path: PathBuf,
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
        fn current_path(&mut self) -> &Path {
            &self.path
        }
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
    fn git() {
        let mut result = Vec::new();
        let prompt = "%(G.%(y.d.)%(m.m.)%(s.s.) %(o.g.h.l.b.a.) %1(p.%2(p.^%p.^).)%1(q.%2(q.v%q.v).)%1(x.%2(x.s%x.s).) %r.)";

        result.clear();
        let mut info = TestInfo {
            path: PathBuf::from("/dev/random"),
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
            path: PathBuf::from("/home/user/sub/sub/dir"),
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

    #[test]
    fn path() {
        let mut result = Vec::new();
        let mut info = TestInfo {
            path: PathBuf::from("/home/user/sub/dir"),
            dirty: true,
            modified: false,
            staged: false,
            domain: Domain::Azure,
            ahead: 0,
            behind: 2,
            branch: "feature",
            stashes: 3,
        };

        result.clear();
        expand("%/{:}", &mut info, &mut result).unwrap();
        assert_eq!(str::from_utf8(&result).unwrap(), "/home/user/sub/dir");

        result.clear();
        expand("%/{:missing:/dev:~:/home/user}", &mut info, &mut result).unwrap();
        assert_eq!(str::from_utf8(&result).unwrap(), "~/sub/dir");

        result.clear();
        expand("%-2/{:~:/home/user}", &mut info, &mut result).unwrap();
        assert_eq!(str::from_utf8(&result).unwrap(), "~/sub");

        result.clear();
        expand("%2/{:}", &mut info, &mut result).unwrap();
        assert_eq!(str::from_utf8(&result).unwrap(), "sub/dir");
    }

    #[test]
    fn path_truncation() {
        let mut result = Vec::new();

        result.clear();
        let mut info = TestInfo {
            path: PathBuf::from("/home/user"),
            dirty: true,
            modified: false,
            staged: false,
            domain: Domain::Azure,
            ahead: 0,
            behind: 2,
            branch: "feature",
            stashes: 3,
        };
        expand("%/{:~:/home/user}", &mut info, &mut result).unwrap();
        assert_eq!(str::from_utf8(&result).unwrap(), "~");

        result.clear();
        let mut info = TestInfo {
            path: PathBuf::from("/"),
            dirty: true,
            modified: false,
            staged: false,
            domain: Domain::Azure,
            ahead: 0,
            behind: 2,
            branch: "feature",
            stashes: 3,
        };
        expand("%/{:}", &mut info, &mut result).unwrap();
        assert_eq!(str::from_utf8(&result).unwrap(), "/");
    }
}
