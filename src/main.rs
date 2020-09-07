#![warn(rust_2018_idioms)]

use git2::{
    Repository,
    Oid
};
use crossbeam_channel::bounded;
use std::{thread};

use anyhow::{Context, Result};

use log::{debug, info, trace, warn};

#[derive(Debug)]
struct DeltaJob {
    old_tree_oid: Oid,
    new_tree_oid: Oid
}

impl DeltaJob {
    fn run(&self) -> Result<DeltaJobOutput> {
        let repo = Repository::open(".").context("opening repository (from work thread)")?;
        let old_tree = repo.find_tree(self.old_tree_oid)?;
        let new_tree = repo.find_tree(self.new_tree_oid)?;
        let diff = repo.diff_tree_to_tree(Some(&old_tree), Some(&new_tree), None)?;

        let mut file_change_count = 0;
        diff.foreach(&mut |_file, _| {
            file_change_count += 1;
            true
        }, None, None, None)?;

        Ok(DeltaJobOutput { file_change_count })
    }
}

#[derive(Default, Debug)]
struct DeltaJobOutput {
    file_change_count: u32
}

impl DeltaJobOutput {
    fn reduce(&self, other: &Self) -> Self {
        DeltaJobOutput {
            file_change_count: self.file_change_count + other.file_change_count
        }
    }
}

fn main() -> Result<()> {
    pretty_env_logger::try_init()?;

    let repo = Repository::open(".").context("open repository")?;

    let (work_tx, work_rx) = bounded::<DeltaJob>(256);
    let (result_tx, result_rx) = bounded::<Result<DeltaJobOutput>>(256);

    let mut work_threads = Vec::new();

    // TODO: num CPUs
    info!("spawning 16 threads");
    for _ in 0..16 {
        let work_rx = work_rx.clone();
        let result_tx = result_tx.clone();
        work_threads.push(thread::spawn(move || {
            for job in work_rx {
                result_tx.send(job.run()).expect("sending result");
            }
        }));
    }

    let (final_tx, final_rx) = bounded::<DeltaJobOutput>(1);

    info!("spawning reduce thread");
    let reduce_thread = thread::spawn(move || {
        let mut final_result: DeltaJobOutput = Default::default();
        for result in result_rx {
            println!("current reduction: {:?}", final_result);
            match result {
                Ok(output) => {
                    final_result = final_result.reduce(&output)
                },
                Err(err) => warn!("got error result: {}", err)
            }
        }
        final_tx.send(final_result).expect("send final result");
    });

    let mut prev_commit = repo.find_commit(repo.head()?.target().expect("missing head!"))?;
    while let Some(commit) = prev_commit.parents().next() {
        trace!("sending job: diff {} to {}", prev_commit.tree_id(), commit.tree_id());
        work_tx.send(DeltaJob{
            old_tree_oid: prev_commit.tree_id(),
            new_tree_oid: commit.tree_id()
        }).context("sending job to workers")?;
        prev_commit = commit;
    }

    for handle in work_threads {
        handle.join().expect("join work thread");
    }
    reduce_thread.join().expect("join reduce thread");

    dbg!(final_rx.recv()?);

    Ok(())
}

// 3.85