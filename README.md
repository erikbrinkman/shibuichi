Shibuichi
=========
[![crates.io](https://img.shields.io/crates/v/shibuichi)](https://crates.io/crates/shibuichi)
[![docs](https://docs.rs/shibuichi/badge.svg)](https://docs.rs/shibuichi)
[![license](https://img.shields.io/github/license/erikbrinkman/shibuichi)](LICENSE)

A custom zsh prompt expander written in rust.

Shibuichi handles a superset of [zsh prompt
expansions](https://zsh.sourceforge.io/Doc/Release/Prompt-Expansion.html).
Standard zsh prompt expansions are left alone, and this just adds several other
expansions, substituting them on the fly:

 - `%r` - The short name of the current git branch. If not in a git repository
   this will be empty.
 - `%p` - An integer for the number of commits the current branch is *ahead* of
   its remote tracking branch. If there is no remote tracking branch, this will
   render as a 0.
 - `%q` - An integer for the number of commits the current branch is *behind*
   of its remote tracking branch. If there is no remote tracking branch, this
   will render as a 0.
 - `%x` - An integer for the number of current stashes.

 In addition, this also adds a few more codes to the conditional substring expansion
 `%(x.true-text.false-text)`. These codes are:

 - `G` - True if in a git repository.
 - `y` - True if the git repository is dirty.
 - `m` - True if the git repository has modified files.
 - `s` - True if the git repository has staged files.
 - `o` - True if the domain of the remote tracking origin number matches `n`,
   where 0 is reserved for all other domains:
    1. `github.com`
    2. `gitlab.com`
    3. `bitbucket.org`
    4. `dev.azure.com`
 - `p` - True if the remote tracking branch is at least `n` commits *ahead* of
   the current branch.
 - `q` - True if the remote tracking branch is at least `n` commits *behind* of
   the current branch.
 - `x` - True if there are at least `n` stashes.

The conditional expansions for `o`, `p`, `q`, and `x` are extended so that if
no number is passed, you can use a conditional of the form
`%(x.0-text.1-text.2-text...)` to make a branch for each possible value. If the
integer is larger than the the number of conditionals, the final text will be
used.

Finally the directory command is extended in a slightly breaking change, where

- `%d{:replacement:prefix:...}`
- `%/{:replacement:prefix:...}` - takes multiple prefix-replacement pairs to
  apply to the path. Any delimiter character can be specified, and backslash
  escapes are honored, but no other expansion happens. `%~` and `%/{:~:$HOME}`
  should be roughly equivalent. Note, that this tries the `PWD` variable first,
  and if it's missing uses a canonical working directory, which may be
  different than that output by `%/`.

Installation
------------

```
cargo install shibuichi
```

Usage
-----

The easiest way to use `shibuichi` is to pass your old prompt through it in
your `precmd` function and then tweak the command with your extensions. For
example:

```
precmd() {
  PROMPT="$(shibuichi ' %r %# ')"
}
```

creates a simple prompt that shows your git branch.

Alternatively you can pass several "prompts" and store them in `psvar` for
referencing in your main prompt.

```
PROMPT=' %1v %2v %# '
precmd() {
  local IFS=$'\0'
  psvar=($(shibuichi -0 '%r' '%p'))
}
```

Note however that `zsh` won't further expand any referenced variables, so you
should only include custom expansions, but not builtin ones.

Both versions make it possible to be fault tolerant to the existence of
`shibuishi` by either falling back to a default prompt if it fails, or adding
branches for the existence of elements of `psvar`. The latter can be a bit
trickier because no expansion happens after taking a string from `psvar`, so
any expansion must be behind conditionals of the form `%x(V...)`.

### Detailed Example

My current prompt, inspired by silver, is:

```
%F{white}%K{black}%(?.%1(j.   .). %1(j..)  ) %n@%m %F{black}%K{blue}%F{black} %/{::$HOME} %F{blue}%(G.%(y.%K{yellow}.%K{green})%F{black} %(o.....ﴃ.)%1(p. .%1(q. .%1(x. .)))%(p....%p)%(q....%q)%(x....%x) %r%(m. .%(s. .))%(m..)%(s..) %(y.%F{yellow}.%F{green}).)%k%f 
```

Design
------

There were two major design decisions that influenced `shibuichi`:

1. `zsh` prompt expansion should handle everything it can. This shouldn't
   reimplement terminal colors, exit code checking, timestamps, etc.
2. This should be agnostic to prompt style. In particular, this shouldn't make
   any choices about layout preferences, or character choices, and instead seek
   to provide the same generality as `zsh` prompt expansion.

Naming
------

Shibuichi is an alloy of silver and copper because this was inspired by
[silver](https://github.com/reujab/silver).
