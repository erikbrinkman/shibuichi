//! The binary to run prompt substitution
#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]

use clap::Parser;
use git2::{Branch, BranchType, Oid, Repository, StatusOptions};
use shibuichi::{expand, util::ParsedScpUrl, Domain, Info};
use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use url::Url;

/// preprocess an expanded zsh prompt string
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Zsh style prompts to apply additional expansion to
    ///
    /// Multple prompts can be provided, if so each will be output delimited by `sep`. This can be
    /// used to store these values in `$psvar` to allow usage in the main zsh prompt without having
    /// to change the prompt itself.
    #[clap(value_parser)]
    prompts: Vec<String>,

    /// Separator for each "prompt"
    ///
    /// If specifying multiple "prompts" they will be separated by this character. The default,
    /// newline, is readable, but if you want variables with a newline in them you can use
    /// alternate characters like null.
    #[clap(short, long, default_value_t = '\n')]
    sep: char,

    /// Use null separator
    ///
    /// Use this flag to overwrite the separator with the null character
    #[clap(short = '0', long)]
    null: bool,
}

fn parse_git_origin(origin: &str) -> Option<String> {
    // NOTE Url creates an owned copy instead of just referencing the string, so we can't just
    // return a reference here
    if let Some(domain) = Url::parse(origin).ok().as_ref().and_then(Url::domain) {
        Some(domain.to_owned())
    } else {
        ParsedScpUrl::parse(origin).map(|url| url.host().to_owned())
    }
}

#[derive(Default)]
enum CachedRepo {
    #[default]
    Unknown,
    NoRepo,
    Repo(Repository),
}

impl CachedRepo {
    fn get(&mut self) -> Option<&mut Repository> {
        match self {
            CachedRepo::NoRepo => None,
            CachedRepo::Repo(repo) => Some(repo),
            CachedRepo::Unknown => {
                *self = match Repository::discover(".") {
                    Ok(repo) => CachedRepo::Repo(repo),
                    Err(_) => CachedRepo::NoRepo,
                };
                self.get()
            }
        }
    }

    fn branch(&mut self) -> Option<String> {
        let repo = self.get()?;
        let head = repo.head().ok()?;
        let name = head.shorthand()?;
        Some(name.to_owned())
    }

    fn get_ahead_behind(
        repo: &Repository,
        local: Oid,
        upstream_branch: &Branch,
    ) -> Option<(usize, usize)> {
        let upstream = upstream_branch.get().target()?;
        repo.graph_ahead_behind(local, upstream).ok()
    }

    fn get_domain(repo: &Repository, upstream_branch: &Branch) -> Option<Domain> {
        let upstream_name = upstream_branch.name().ok()??;
        let remote_name = upstream_name.split('/').next()?;
        let remote = repo.find_remote(remote_name).ok()?;
        let url = parse_git_origin(remote.url()?)?;
        let domain = match url.as_ref() {
            "github.com" => Domain::Github,
            "gitlab.com" => Domain::Gitlab,
            "bitbucket.org" => Domain::BitBucket,
            "dev.azure.com" => Domain::Azure,
            _ => Domain::Git,
        };
        Some(domain)
    }

    fn remote_info(&mut self) -> Option<(Domain, usize, usize)> {
        let repo = self.get()?;
        let head = repo.head().ok()?;
        let branch = head.shorthand()?;
        let local = head.target()?;
        let local_branch = repo.find_branch(branch, BranchType::Local).ok()?;
        let upstream_branch = local_branch.upstream().ok()?;
        let (num_ahead, num_behind) =
            CachedRepo::get_ahead_behind(repo, local, &upstream_branch).unwrap_or_default();
        let domain = CachedRepo::get_domain(repo, &upstream_branch).unwrap_or(Domain::Git);
        Some((domain, num_ahead, num_behind))
    }

    fn stashes(&mut self) -> usize {
        let mut stashes = 0;
        if let Some(repo) = self.get() {
            repo.stash_foreach(|_, _, _| {
                stashes += 1;
                true
            })
            .unwrap();
        }
        stashes
    }

    fn status(&mut self) -> Option<(bool, bool, bool)> {
        let repo = self.get()?;
        let mut dirty = false;
        let mut modified = false;
        let mut staged = false;

        if let Ok(statuses) = repo.statuses(Some(StatusOptions::new().include_untracked(true))) {
            for status in statuses.iter() {
                dirty = true;

                let status = status.status();
                if !modified && status.is_wt_new()
                    || status.is_wt_modified()
                    || status.is_wt_renamed()
                    || status.is_wt_typechange()
                {
                    modified = true;
                }
                if !staged && status.is_index_new()
                    || status.is_index_modified()
                    || status.is_index_deleted()
                    || status.is_index_renamed()
                    || status.is_index_typechange()
                {
                    staged = true;
                }

                if modified && staged {
                    break;
                }
            }
        }
        Some((dirty, modified, staged))
    }
}

#[derive(Default)]
struct Cache {
    path: Option<PathBuf>,
    repo: CachedRepo,
    remote_info: Option<(Domain, usize, usize)>,
    branch: Option<String>,
    stashes: Option<usize>,
    status: Option<(bool, bool, bool)>,
}

impl Cache {
    fn git_remote_info(&mut self) -> &(Domain, usize, usize) {
        match &mut self.remote_info {
            Some(info) => info,
            info @ None => {
                *info = Some(self.repo.remote_info().unwrap_or((Domain::Git, 0, 0)));
                info.as_ref().unwrap()
            }
        }
    }

    fn git_status(&mut self) -> &(bool, bool, bool) {
        match &mut self.status {
            Some(status) => status,
            status @ None => {
                *status = Some(self.repo.status().unwrap_or_default());
                status.as_ref().unwrap()
            }
        }
    }
}

impl Info for Cache {
    fn current_path(&mut self) -> &Path {
        match &mut self.path {
            Some(buf) => buf,
            buf @ None => {
                let path = if let Ok(pwd) = env::var("PWD") {
                    PathBuf::from(pwd)
                } else if let Ok(cwd) = env::current_dir() {
                    cwd
                } else {
                    PathBuf::new()
                };
                *buf = Some(path);
                buf.as_ref().unwrap()
            }
        }
        .as_path()
    }

    fn git_exists(&mut self) -> bool {
        self.repo.get().is_some()
    }

    fn git_dirty(&mut self) -> bool {
        self.git_status().0
    }

    fn git_modified(&mut self) -> bool {
        self.git_status().1
    }

    fn git_staged(&mut self) -> bool {
        self.git_status().2
    }

    fn git_remote_domain(&mut self) -> Domain {
        self.git_remote_info().0
    }

    fn git_remote_ahead(&mut self) -> usize {
        self.git_remote_info().1
    }

    fn git_remote_behind(&mut self) -> usize {
        self.git_remote_info().2
    }

    fn git_branch(&mut self) -> &str {
        match &mut self.branch {
            Some(branch) => branch,
            branch @ None => {
                *branch = Some(self.repo.branch().unwrap_or_default());
                branch.as_ref().unwrap()
            }
        }
    }

    fn git_stashes(&mut self) -> usize {
        match &mut self.stashes {
            Some(stashes) => *stashes,
            stashes @ None => {
                let res = self.repo.stashes();
                *stashes = Some(res);
                res
            }
        }
    }
}

fn main() {
    let args = Args::parse();
    let mut cache = Cache::default();
    let mut out = io::stdout().lock();
    let mut not_first = false;
    let sep = if args.null { '\0' } else { args.sep };
    for prompt in args.prompts {
        if not_first {
            write!(out, "{sep}").unwrap();
        } else {
            not_first = true;
        }

        expand(prompt, &mut cache, &mut out).unwrap();
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_parse_git_origin() {
        let domain = super::parse_git_origin("git@github.com:path/file.git").unwrap();
        assert_eq!(domain, "github.com");
    }
}
