---
title: Libraries
description: Use libraries other people publish with tessel add and import, and publish your own with tessel publish, from its git repository to the library index.
---

A **library** is a [module](/language/programs/#modules) that someone has
published for any program to use: a folder of `.tsl` files with a
`tessel.toml` that gives it a name and a version. Published libraries are
listed in the **library index**, a public repository on GitHub
([alexiusacademia/tessel-packages](https://github.com/alexiusacademia/tessel-packages)),
and their code stays in their own git repositories. Browse them, with each
one's README and versions, at
[tessel-packages.syncster.dev](https://tessel-packages.syncster.dev).

## Using a library

```sh
tessel search text       # what's published, by name or description
tessel add textkit       # use the newest version of textkit
```

`tessel add` writes the library into the `[dependencies]` of the project's
`tessel.toml` (making one if there isn't one):

```toml
[dependencies]
textkit = "1.2"
```

and the program imports it by name, like any module:

```tessel
import textkit

fn main() {
    print(textkit.shout("hello"))
}
```

```text
HELLO!
```

The library's `public` declarations are used by their full name,
`textkit.shout`, as with any module. For a long name, give it a shorter one
in the file with [`as`](/language/programs/#another-name-for-a-module):
`import textkit as tk`, then `tk.shout("hello")`. A library sees only its own code and
the libraries it uses, never the program's.

### Versions

`"1.2"` asks for version 1.2.0 or a later 1.x: versions that keep working
with code written for 1.2. Before 1.0, `"0.3"` asks for a later 0.3.x only.
`"=1.2.0"` asks for that version and no other. `tessel add textkit@1.2`
writes a version of your choice.

The libraries a library uses are fetched too, and every library has one
version in a program: one that meets what everyone asks for. When no
version does, Tessel says who asks for what:

```text
tessel: no version of `geokit` is what everyone asks for: this project asks
for "0.2", textkit 1.2.0 asks for "0.3"; the published versions are 0.3.1
```

### `tessel.lock`

The versions Tessel picks are written to `tessel.lock`, next to
`tessel.toml`. Commit it with the code: then every build, on any computer,
uses exactly the same code. Tessel works out new versions only when
`tessel.toml` asks for something the lock doesn't have, or when you ask:

```sh
tessel deps              # the libraries the project uses, and what they use
tessel deps update       # move to the newest versions tessel.toml allows
tessel deps update textkit
```

```text
textkit 1.2.0
  └─ geokit 0.3.1
```

The code of each version is downloaded once into Tessel's folder
(`packages/` in `~/.tessel`, or `%LOCALAPPDATA%\Tessel` on Windows) and
shared by every project. Each download is checked against the checksum
recorded when the version was published, so code changed after
publishing is refused.

A program can only import the libraries in its own `[dependencies]`. To
use one that a library uses, add it as well.

`tessel remove textkit` takes a library out.

### From git, or from a folder

A library doesn't have to be in the index. `tessel.toml` can name a tag of
a git repository, or a folder on your computer (handy while you write a
library and a program together):

```toml
[dependencies]
geokit = { git = "https://github.com/someone/geokit", tag = "v0.3.0" }
mine = { path = "../mine" }
```

```sh
tessel add geokit --git https://github.com/someone/geokit --tag v0.3.0
tessel add mine --path ../mine
```

A library from GitHub is downloaded as an archive; from other hosts it
needs `git`.

## Writing a library

```sh
tessel new textkit --template library
```

makes a folder with the library's code, its `tessel.toml` and a README. The
`[library]` table is what's published:

```toml
[library]
name = "textkit"
version = "1.2.0"
description = "Text helpers: shouting, padding and wrapping"
license = "MIT"

[dependencies]
geokit = "0.3"
```

| Key | What it is |
|---|---|
| `name` | How programs import it. Lowercase letters, digits and `_`, starting with a letter. |
| `version` | Three numbers, like `1.2.0`. Raise the last for fixes, the middle for additions, the first for changes that break code written for the version before. |
| `description` | What it's for, in a sentence. `tessel search` shows it. |
| `license` | The licence the code is under, like `"MIT"` or `"Apache-2.0"`. |
| `repository` | Where its code is, like `"https://github.com/someone/textkit"`. Without it, Tessel uses the git repository's `origin`. |

Its code is the `.tsl` files next to `tessel.toml`, and what's `public` in
them is what programs can use. Subfolders can be modules of its own, which
it imports as usual. A library has no `app` and no `fn main`; `tessel
check .` checks it without one.

To try a library from a program before publishing it, use it from its
folder: `tessel add textkit --path ../textkit`.

## Publishing

Libraries are published from their git repository, on GitHub or another
git host. Commit the code (with `tessel.toml`), then in its folder:

```sh
tessel publish --dry-run   # check, without publishing
tessel publish
```

`tessel publish`:

1. checks the `[library]` table, and that the code compiles (with the
   libraries it uses) and has something `public`;
2. makes sure the name is free (or already yours), the version is new,
   everything in the repository is committed and the tag `v1.2.0`, if
   it's there already, is on the commit being published;
3. tags the commit `v1.2.0` and pushes the tag to `origin`;
4. downloads that tag back from the repository, as programs will, and
   works out its checksum;
5. opens GitHub's page for proposing the version's entry to the index,
   `packages/textkit/1.2.0.toml`. Press **Propose new file**, then **Create
   pull request**.

With `--dry-run` it makes every check of steps 1 and 2 and stops there:
when it says the library is ready to publish, `tessel publish` goes
through.

The index checks the pull request: the entry is well formed, its tag is
the commit it names, the code has the checksum and compiles, the
libraries it uses are published, and the name belongs to the same
repository as its earlier versions. Once it's merged, `tessel add textkit`
finds it, and it's on [tessel-packages.syncster.dev](https://tessel-packages.syncster.dev)
a minute or two later, with its README.

A published version can't change: its tag mustn't move, and a fix is a new
version. A published library can only use published libraries, not git
tags or folders.
