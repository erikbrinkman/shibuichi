Shibuichi
=========

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

The conditional expansion for `o`, `p`, `q`, and `x` is extended so that if no
number is passed, you can use a conditional of the form
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

Replace your zsh `PROMPT` `precmd` by processing it with `shibuichi` first.
This then allows you to use the expanded escape sequences.

For example:

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

My current prompt is:
```
%F{white}%K{black}%(?.%1(j.   .). %1(j..)  ) %n@%m %F{black}%K{blue}%F{black} %~ %F{blue}%(G.%(y.%K{yellow}.%K{green})%F{black} %0(o..)%1(o..)%2(o..)%3(o..)%4(o.ﴃ.)%1(p. .%1(q. .%1(x. .)))%1(p.%2(p.%3(p.%p.).).)%1(q.%2(q.%3(q.%q.).).)%1(x.%2(x.%3(x.%x.).).) %r%(m. .%(s. .))%(m..)%(s..) %(y.%F{yellow}.%F{green}).)%k%f 
```

Design
------

This was designed to be as agnostic to prompt style as possible. In particular,
this shouldn't make any choices about layout preferences, and instead seek to
provide manipulable output.

To Do
-----

- [ ] I have yet to come up with a good way of handling path prefixes, which
  would help emulate more of silvers features.

Naming
------

Shibuichi is an alloy of silver and copper because this was inspired by
[silver](https://github.com/reujab/silver).
