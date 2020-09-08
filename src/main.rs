#![warn(rust_2018_idioms)]

use git2::{ObjectType, Oid, Repository};

use anyhow::{bail, Context, Result};

use chrono::{Duration, Local};
use clap::Clap;
use log::{info, warn};
use std::collections::BTreeMap;
use std::{
    path::{Path, PathBuf}
};

#[derive(Debug)]
struct DirectoryNode {
    last_revision: Oid,
    children: BTreeMap<Vec<u8>, WatchNode>,
}

#[derive(Debug)]
struct FileNode {
    last_revision: Oid,
    change_count: usize,
}

#[derive(Debug)]
enum WatchNode {
    Directory(DirectoryNode),
    File(FileNode),
}

impl WatchNode {
    fn new_file(last_revision: Oid) -> WatchNode {
        WatchNode::File(FileNode {
            last_revision,
            change_count: 0,
        })
    }

    fn from_git_tree_and_path(
        repo: &Repository,
        tree: git2::Tree<'_>,
        path: String,
    ) -> Result<WatchNode> {
        let components = path.split("/");
        Self::from_git_tree_and_components(repo, tree, components)
    }

    fn from_git_tree_and_components<'a>(
        repo: &Repository,
        tree: git2::Tree<'_>,
        mut components: impl Iterator<Item = &'a str>,
    ) -> Result<WatchNode> {
        match components.next() {
            Some(component) => {
                let try_tree = tree
                    .get_name(component)
                    .map(|e| e.to_object(repo).and_then(|o| o.peel_to_tree()));
                match try_tree {
                    Some(Ok(sub_tree)) => {
                        let revision = sub_tree.id();
                        let sub_node =
                            Self::from_git_tree_and_components(repo, sub_tree, components)?;
                        let mut children = BTreeMap::new();
                        children.insert(component.bytes().collect::<Vec<u8>>(), sub_node);
                        Ok(WatchNode::Directory(DirectoryNode {
                            last_revision: revision,
                            children: children,
                        }))
                    }
                    Some(Err(_)) => bail!("requested directory is not a tree"),
                    None => bail!("requested directory is not tracked"),
                }
            }
            None => Self::from_git_tree(repo, tree),
        }
    }

    fn from_git_tree(repo: &Repository, tree: git2::Tree<'_>) -> Result<WatchNode> {
        let mut children = BTreeMap::default();
        for entry in tree.iter() {
            match entry.kind() {
                Some(ObjectType::Tree) => {
                    let sub_tree = entry.to_object(repo)?.peel_to_tree().unwrap();
                    let sub_directory = WatchNode::from_git_tree(repo, sub_tree)?;
                    children.insert(entry.name_bytes().to_vec(), sub_directory);
                }
                Some(ObjectType::Blob) => {
                    children.insert(entry.name_bytes().to_vec(), WatchNode::new_file(entry.id()));
                }
                _ => (),
            };
        }
        Ok(WatchNode::Directory(DirectoryNode {
            last_revision: tree.id(),
            children: children,
        }))
    }

    fn update_for_revision(&mut self, repo: &Repository, revision: Oid) -> Result<()> {
        let mut result = Ok(());

        take_mut::take(self, |current| {
            match current {
                WatchNode::File(FileNode {
                    change_count,
                    last_revision,
                }) if revision != last_revision => WatchNode::File(FileNode {
                    change_count: change_count + 1,
                    last_revision: revision,
                }),
                WatchNode::Directory(DirectoryNode {
                    mut children,
                    last_revision,
                }) if revision != last_revision => {
                    match repo.find_tree(revision) {
                        Ok(tree) => {
                            for entry in tree.iter() {
                                if let Some(watch_node) = children.get_mut(entry.name_bytes()) {
                                    if let Err(child_err) =
                                        watch_node.update_for_revision(repo, entry.id())
                                    {
                                        result = Err(child_err);
                                    }
                                }
                            }
                            WatchNode::Directory(DirectoryNode {
                                children,
                                last_revision: revision,
                            })
                        }
                        Err(err) if err.code() == git2::ErrorCode::NotFound => {
                            // Directory was previously a file with the same name.
                            WatchNode::Directory(DirectoryNode {
                                children,
                                last_revision: revision,
                            })
                        }
                        Err(other_err) => {
                            result = Err(other_err.into());
                            WatchNode::Directory(DirectoryNode {
                                children,
                                last_revision,
                            })
                        }
                    }
                }
                _ => current,
            }
        });
        result
    }

    fn walk_files<F: FnMut(&Path, &FileNode) -> ()>(&self, walker: &mut F) {
        match self {
            &WatchNode::File { .. } => {
                panic!("walk_files can only be called on a directory");
            }
            &WatchNode::Directory { .. } => {
                self.walk_files_internal(walker, PathBuf::new());
            }
        }
    }

    fn walk_files_internal<F: FnMut(&Path, &FileNode) -> ()>(
        &self,
        walker: &mut F,
        path_buf: PathBuf,
    ) {
        match self {
            &WatchNode::File(ref file_node) => {
                walker(path_buf.as_path(), file_node);
            }
            &WatchNode::Directory(DirectoryNode { ref children, .. }) => {
                for (name_bytes, node) in children {
                    let mut node_path = path_buf.clone();
                    node_path.push(String::from_utf8_lossy(&name_bytes).into_owned());
                    node.walk_files_internal(walker, node_path);
                }
            }
        }
    }
}

#[derive(Clap)]
#[clap(version = "0.1", author = "Sam Doiron <samuel@hey.com>")]
struct CliOpts {
    #[clap(long, about = "number of weeks in the past to check")]
    weeks: Option<i64>,

    #[clap(long, about = "restrict checks to a directory (relative to repository root)")]
    directory: Option<String>,
}

fn main() -> Result<()> {
    pretty_env_logger::try_init()?;
    let cli_opts = CliOpts::parse();

    let repo = Repository::open(".").context("open repository")?;

    let mut last_commit = repo.head()?.peel_to_commit()?;

    let mut full_tree = match cli_opts.directory {
        Some(dir) => WatchNode::from_git_tree_and_path(&repo, last_commit.tree()?, dir)?,
        None => WatchNode::from_git_tree(&repo, last_commit.tree()?)?,
    };

    let commits = match cli_opts.weeks {
        Some(weeks) => {
            let start_time_epoch = (Local::now() - Duration::weeks(weeks)).timestamp();

            let mut commits = Vec::new();
            while let Some(commit) = last_commit.parents().next() {
                commits.push(commit.tree_id());
                if commit.time().seconds() < start_time_epoch {
                    break;
                }
                last_commit = commit;
            }
            commits
        }
        None => {
            let mut commits = Vec::new();
            while let Some(commit) = last_commit.parents().next() {
                commits.push(commit.tree_id());
                last_commit = commit;
            }
            commits
        }
    };

    let commit_count = commits.len();
    info!("counted {} commits total", commit_count);

    let mut commits_checked = 0usize;
    for tree_id in commits {
        full_tree
            .update_for_revision(&repo, tree_id)
            .with_context(|| format!("could not process tree {}", tree_id))?;
        commits_checked += 1;
        if commits_checked % 1000 == 0 {
            info!(
                "checked {}/{} commits ({}% complete)",
                commits_checked,
                commit_count,
                100 * commits_checked / commit_count
            );
        }
    }

    full_tree.walk_files(&mut |path, file_node| {
        println!("{}\t{}", file_node.change_count, path.to_string_lossy());
    });

    Ok(())
}
