#![warn(rust_2018_idioms)]

use git2::{
    Repository
};
use anyhow::{Context, Result};

fn main() -> Result<()> {
    let repo = Repository::open(".").context("open repository")?;
    let mut walk = repo.revwalk()?;
    walk.push_head()?;
    walk.simplify_first_parent()?;

    let mut prev_commit = repo.find_commit(walk.next().expect("must have at least one commit")?)?;
    for oid in walk {
        let commit = repo.find_commit(oid?)?;
        prev_commit = commit;
    }

    println!("Walked {} commits", commit_count);

    Ok(())
}
