//! The binary to run prompt substitution
use clap::Parser;
use git2::{Branch, BranchType, Oid, Repository, StatusOptions};
use shibuichi::{expand, util::ParsedScpUrl, Domain, Info};
use std::env;
use std::io::stdout;
use std::path::{Path, PathBuf};
use url::Url;

/// preprocess an expanded zsh prompt string
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// zsh style prompt to apply additional expansion to
    #[clap(value_parser)]
    prompt: String,
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
struct CachedRepo(Option<Option<Repository>>);

impl CachedRepo {
    fn get(&mut self) -> Option<&mut Repository> {
        match &mut self.0 {
            Some(repo) => repo,
            repo @ None => {
                *repo = Some(Repository::discover(".").ok());
                repo.as_mut().unwrap()
            }
        }
        .as_mut()
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
        let (ahead, behind) =
            CachedRepo::get_ahead_behind(repo, local, &upstream_branch).unwrap_or_default();
        let domain = CachedRepo::get_domain(repo, &upstream_branch).unwrap_or(Domain::Git);
        Some((domain, ahead, behind))
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
    cached_path: Option<PathBuf>,
    cached_repo: CachedRepo,
    cached_remote_info: Option<(Domain, usize, usize)>,
    cached_branch: Option<String>,
    cached_stashes: Option<usize>,
    cached_status: Option<(bool, bool, bool)>,
}

impl Cache {
    fn git_remote_info(&mut self) -> &(Domain, usize, usize) {
        match &mut self.cached_remote_info {
            Some(info) => info,
            info @ None => {
                *info = Some(
                    self.cached_repo
                        .remote_info()
                        .unwrap_or((Domain::Git, 0, 0)),
                );
                info.as_ref().unwrap()
            }
        }
    }

    fn git_status(&mut self) -> &(bool, bool, bool) {
        match &mut self.cached_status {
            Some(status) => status,
            status @ None => {
                *status = Some(self.cached_repo.status().unwrap_or_default());
                status.as_ref().unwrap()
            }
        }
    }
}

impl Info for Cache {
    fn current_path(&mut self) -> &Path {
        match &mut self.cached_path {
            Some(buf) => buf,
            buf @ None => {
                *buf = Some(env::current_dir().unwrap_or_else(|_| PathBuf::new()));
                buf.as_ref().unwrap()
            }
        }
        .as_path()
    }

    fn git_exists(&mut self) -> bool {
        self.cached_repo.get().is_some()
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
        match &mut self.cached_branch {
            Some(branch) => branch,
            branch @ None => {
                *branch = Some(self.cached_repo.branch().unwrap_or_default());
                branch.as_ref().unwrap()
            }
        }
    }

    fn git_stashes(&mut self) -> usize {
        match &mut self.cached_stashes {
            Some(stashes) => *stashes,
            stashes @ None => {
                let res = self.cached_repo.stashes();
                *stashes = Some(res);
                res
            }
        }
    }
}

fn main() {
    let args = Args::parse();
    expand(args.prompt, &mut Cache::default(), &mut stdout()).unwrap();
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_parse_git_origin() {
        let domain = super::parse_git_origin("git@github.com:path/file.git").unwrap();
        assert_eq!(domain, "github.com");
    }
}
