#![warn(rust_2018_idioms)]

use git2::{
    Repository,
    Oid, DiffOptions, TreeWalkMode, TreeWalkResult, ObjectType
};

use anyhow::{Context, Result};

use log::{debug, info, trace, warn};

fn main() -> Result<()> {
    pretty_env_logger::try_init()?;

    let repo = Repository::open(".").context("open repository")?;

    let mut total_deltas = 0usize;
    let mut total_commits_searched = 0usize;

    let mut last_commit = repo.head()?.peel_to_commit()?;
    let mut last_tree = last_commit.tree()?;

    let mut diff_opts = DiffOptions::new();

    last_tree.walk(TreeWalkMode::PreOrder, |path, entry| {
        if let (Some(ObjectType::Blob), Some(name)) = (entry.kind(), entry.name()) {
            let mut full_path = String::with_capacity(path.len() + name.len());
            full_path.push_str(path);
            full_path.push_str(name);

            trace!("watching path: ./{}", full_path);
            diff_opts.pathspec(full_path);
        }
        TreeWalkResult::Ok
    })?;

    while let Some(commit) = last_commit.parents().next() {
        let tree = commit.tree()?;
        total_commits_searched += 1;

        debug!("starting comparison {:?} to {:?}", last_commit.id(), commit.id());
        let diff = repo.diff_tree_to_tree(
            Some(&last_tree),
            Some(&tree),
            Some(&mut diff_opts)
        )?;
        debug!("finished diffing");
        debug!("calculating stats");
        let delta = diff.stats()?.files_changed();
        debug!("stat calculation finished");
        total_deltas += delta;
        
        if delta > 0 {
            println!("changes: {} / {}", total_deltas, total_commits_searched);
        }

        last_commit = commit;
        last_tree = tree;
    }

    Ok(())
}
