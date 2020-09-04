#![warn(rust_2018_idioms)]

mod executor;

use std::env::set_current_dir;

use std::io::{
    self,
    ErrorKind::*,
};
use std::path::PathBuf;
use std::str;
use smol::{
    self,
    unblock,
    fs as afs,
    future::zip
};
use std::collections::HashMap;
use std::time::{Duration, Instant};

use executor::spawn;


#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash)]
struct Sha(String);

impl std::fmt::Debug for Sha {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "sha {}", self.0)
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Hash)]
struct RefStr(Vec<u8>);

impl std::fmt::Debug for RefStr {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(fmt, "ref: {}", String::from_utf8_lossy(&self.0[..]))
    }
}

struct PackedRefs {
    refdb: HashMap<RefStr, Sha>
}

impl PackedRefs {
    async fn load() -> io::Result<Self> {
        let mut refdb = HashMap::new();
        let file_bytes = afs::read(".git/packed-refs")
            .await?
            .split(|b| b == &b'\n')
             // lines starting with # are metadata, ^ seems to be something called a "boundary commit"
             // resulting from a shallow clone (?)
            .filter(|line| !line.is_empty() && line[0] != b'#' && line[0] != b'^')
            .for_each(|line| {
                let mut parts = line.split(|c| c == &b' ');
                let sha = Sha(str::from_utf8(parts.next().unwrap()).unwrap().into());
                let ref_str = RefStr(parts.next().expect("ref-db malformed: lone entry").to_vec());
                refdb.insert(ref_str, sha);
            });
        Ok(PackedRefs { refdb })
    }

    fn lookup_ref(&self, ref_str: &RefStr) -> Option<Sha> {
        self.refdb.get(ref_str).map(Sha::clone)
    }
}

async fn fetch(sha: Sha) -> io::Result<Vec<u8>> {
    let mut path = PathBuf::new();
    path.push(".git");
    path.push("objects");
    path.push(&sha.0[..2]);
    path.push(&sha.0[3..]);
    dbg!(&path);
    afs::read(path).await
}

async fn lookup_in_refs_directory(ref_str: &str) -> io::Result<Option<Sha>> {
    let mut ref_path = PathBuf::new();
    ref_path.push(".git");
    ref_path.push(ref_str);

    match afs::read(ref_path).await {
        Ok(ref_bytes) => Ok(Some(Sha(String::from_utf8(ref_bytes).expect("sha is utf8")))),
        Err(err) if err.kind() == NotFound => Ok(None),
        Err(other) => Err(other)
    }
}

fn main() {
    smol::block_on(async {
        let read_head = spawn(async {
            let head_bytes = afs::read(".git/HEAD").await.expect("read head");
            if head_bytes.starts_with(b"ref: ") {
                RefStr(head_bytes[5..head_bytes.len()-1].to_vec())
            } else {
                // Detatched head state where it is just a SHA
                todo!();
            }
        });

        let read_packed_refs = spawn(async {
            PackedRefs::load().await.expect("load packed refs")
        });

        let (head_ref, packed_refs) = zip(read_head, read_packed_refs).await;
        let head_sha = packed_refs.lookup_ref(&head_ref);
        match head_sha {
            Some(sha) => dbg!(fetch(sha).await),
            None => panic!("head SHA could not be found")
        }
    });
}
