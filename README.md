# Butter - A fast Git churn calculator


### Usage

```
butter 0.1
Sam Doiron <samuel@hey.com>

USAGE:
    butter [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
        --directory <directory>    restrict checks to a directory (relative to repository root)
        --weeks <weeks>            number of weeks in the past to check
```

### Installation

1. Install Rust / Cargo (nightly). I recommend https://rustup.rs
2. `cargo install --bin https://github.com/samdoiron/butter.git`

### Why is it fast?

Butter uses a custom reimplementation of git diff that lazily
skips over unchanged or untracked files.

### How fast is it?

On my machine (15" 2017 MBP, 2.9GHz Quad-core i7) it can run through 180,000 commits
tracking ~70k files in around 1 minute.

For all reasonably sized repositories, calculation should be near instant.

### Missing features

- Rename support
