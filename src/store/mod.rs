mod commit;
mod tree;
mod object;

pub use crate::store::object::Object as GitObject;
pub use crate::store::object::ObjectType as GitObjectType;

use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::io::{Read,Write};
use std::io::Result as IoResult;
use std::path::{Path,PathBuf};
use std::os::unix::fs::MetadataExt;
use std::iter::FromIterator;

use byteorder::{BigEndian, WriteBytesExt};

use faster_hex::hex_decode;

use crate::packfile::PackFile;

use self::tree::{Tree,TreeEntry,EntryMode};
use self::commit::Commit;

use std::env;

pub struct Repo {
    dir: String,
    pack: Option<PackFile>,
}

impl Repo {
    ///
    /// Recursively searches for and loads a repository from the enclosing
    /// directories.
    ///
    pub fn from_enclosing() -> IoResult<Self> {
        // Navigate upwards until we are in the repo
        let mut dir = env::current_dir()?;
        while !is_git_repo(&dir) {
            assert!(dir.pop(), "Not in a git repo");
        }

        let mut p = dir.clone();
        p.push(".git");
        p.push("objects");
        p.push("pack");

        let pack_path = if p.exists() {
            let mut the_path = String::new();
            for dir_entry in fs::read_dir(p)? {
                let d = dir_entry.unwrap();
                let fname = d.file_name();
                let s = fname.to_str().unwrap();
                if s.starts_with("pack") && s.ends_with(".pack") {
                    the_path = s.to_owned();
                    break;
                }
            }
            Some(the_path)
        } else {
            None
        };

        let pack = pack_path.map(|p| {
            PackFile::open(p).unwrap()
        });

        Ok(Repo {
            dir: dir.to_str().unwrap().to_owned(),
            pack,
        })
    }

    pub fn from_packfile(dir: &str, packfile_data: &[u8]) -> IoResult<Self> {
        let packfile = PackFile::parse(packfile_data)?;
        let mut root = PathBuf::new();
        root.push(dir);
        root.push(".git");
        packfile.write(&root)?;

        Ok(Repo {
            dir: dir.to_owned(),
            pack: Some(packfile),
        })
    }

    ///
    /// Resolves the head SHA and attempts to create the file structure
    /// of the repository.
    ///
    pub fn checkout_head(&self) -> IoResult<()> {
        let tip = resolve_ref(&self.dir, "HEAD")?;
        let mut idx = Vec::new();
        // FIXME: This should also "bubble up" errors, walk needs to return a result.
        self.walk(&tip).and_then(|t| self.walk_tree(&self.dir, &t, &mut idx).ok());
        write_index(&self.dir, &mut idx[..])?;
        Ok(())
    }

    pub fn walk(&self, sha: &[u8]) -> Option<Tree> {
        self.read_object(sha).ok().and_then(|object| {
            match object.obj_type {
                GitObjectType::Commit => {
                    object.as_commit().and_then(|c| self.extract_tree(&c))
                },
                GitObjectType::Tree => {
                    object.as_tree()
                },
                _ => None
            }
        })
    }

    fn walk_tree(&self, parent: &str, tree: &Tree, idx: &mut Vec<IndexEntry>) -> IoResult<()> {
        for entry in &tree.entries {
            let &TreeEntry {
                ref path,
                ref mode,
                ref sha
            } = entry;
            let mut full_path = PathBuf::new();
            full_path.push(parent);
            full_path.push(path);
            match *mode {
                EntryMode::SubDirectory => {
                    fs::create_dir_all(&full_path)?;
                    let path_str = full_path.to_str().unwrap();
                    self.walk(sha).and_then(|t| {
                        self.walk_tree(path_str, &t, idx).ok()
                    });
                },
                EntryMode::Normal | EntryMode::Executable => {
                    let object = self.read_object(sha)?;
                    let mut file = File::create(&full_path)?;
                    file.write_all(&object.content[..])?;
                    let meta = file.metadata()?;
                    let mut perms = meta.permissions();

                    let raw_mode = match *mode {
                        EntryMode::Normal => 33188,
                        _ => 33261
                    };
                    perms.set_mode(raw_mode);
                    fs::set_permissions(&full_path, perms)?;

                    let idx_entry = get_index_entry(
                        &self.dir,
                        full_path.to_str().unwrap(),
                        mode.clone(),
                        &sha[..])?;
                    idx.push(idx_entry);
                },
                ref e => panic!("Unsupported Entry Mode {:?}", e)
            }
        }
        Ok(())
    }

    fn extract_tree(&self, commit: &Commit) -> Option<Tree> {
        let sha = &commit.tree;
        self.read_tree(sha)
    }

    fn read_tree(&self, sha: &[u8]) -> Option<Tree> {
        self.read_object(sha).ok().and_then(|obj| {
            obj.as_tree()
        })
    }

    pub fn read_object(&self, sha: &[u8]) -> IoResult<GitObject> {
        // Attempt to read from disk first
        GitObject::open(&self.dir, sha).or_else(|_| {
            // If this isn't there, read from the packfile
            let pack = self.pack.as_ref().unwrap();
            pack.find_by_sha(sha).map(|o| o.unwrap())
        })
    }

    pub fn log(&self, rev: &str) -> IoResult<()> {
        let mut sha = resolve_ref(&self.dir, rev)?;
        loop {
            let object = self.read_object(&sha)?;
            let commit = object.as_commit().expect("Tried to log an object that wasn't a commit");
            println!("{}", commit);
            if commit.parents.is_empty() {
                break;
            }
            sha = commit.parents[0].to_owned();
        }
        Ok(())
    }
}

fn is_git_repo<P: AsRef<Path>>(p: &P) -> bool {
    let path = p.as_ref().join(".git");
    path.exists()
}

///
/// Reads the given ref to a valid SHA.
///
fn resolve_ref(repo: &str, name: &str) -> IoResult<Vec<u8>> {
    // Check if the name is already a sha.
    let trimmed = name.trim();
    if is_hex_sha(trimmed) {
        let mut sha = vec![0; trimmed.len() / 2];
        hex_decode(trimmed.as_bytes(), &mut sha).unwrap();
        Ok(sha)
    } else {
        read_sym_ref(repo, trimmed)
    }
}

///
/// Returns true if id is a valid SHA-1 hash.
///
fn is_hex_sha(id: &str) -> bool {
    id.len() == 40 && id.chars().all(|c| c.is_digit(16))
}


///
/// Reads the symbolic ref and resolve it to the actual ref it represents.
///
fn read_sym_ref(repo: &str, name: &str) -> IoResult<Vec<u8>> {
    // Read the symbolic ref directly and parse the actual ref out
    let mut path = PathBuf::new();
    path.push(repo);
    path.push(".git");

    if name != "HEAD" {
        if !name.contains('/') {
            path.push("refs/heads");
        } else if !name.starts_with("refs/") {
            path.push("refs/remotes");
        }
    }
    path.push(name);

    // Read the actual ref out
    let mut contents = String::new();
    let mut file = File::open(path)?;
    file.read_to_string(&mut contents)?;

    if contents.starts_with("ref: ") {
        let the_ref = contents.split("ref: ")
            .nth(1)
            .unwrap()
            .trim();
        resolve_ref(repo, the_ref)
    } else {
        let trimmed = contents.trim();
        let mut sha = vec![0; trimmed.len() / 2];
        hex_decode(trimmed.as_bytes(), &mut sha).unwrap();
        Ok(sha)
    }
}

#[derive(Debug)]
struct IndexEntry {
    ctime: i64,
    mtime: i64,
    device: i32,
    inode: u64,
    mode: u16,
    uid: u32,
    gid: u32,
    size: i64,
    sha: Vec<u8>,
    file_mode: EntryMode,
    path: String
}

// FIXME:
// This doesn't need to read the file a second time.
fn get_index_entry(root: &str, path: &str, file_mode: EntryMode, sha: &[u8]) -> IoResult<IndexEntry> {
    let file = File::open(path)?;
    let meta = file.metadata()?;

    // We need to remove the repo path from the path we save on the index entry
    // FIXME: This doesn't need to be a path since we just discard it again
    let relative_path = PathBuf::from(
            path.trim_start_matches(root)
                .trim_start_matches('/')
        );

    Ok(IndexEntry {
        ctime: meta.ctime(),
        mtime: meta.mtime(),
        device: meta.dev() as i32,
        inode: meta.ino(),
        mode: meta.mode() as u16,
        uid: meta.uid(),
        gid: meta.gid(),
        size: meta.size() as i64,
        sha: sha.to_owned(),
        path: relative_path.to_str().unwrap().to_owned(),
        file_mode,
    })
}

fn write_index(repo: &str, entries: &mut [IndexEntry]) -> IoResult<()> {
    let mut path = PathBuf::new();
    path.push(repo);
    path.push(".git");
    path.push("index");
    let mut idx_file = File::create(path)?;
    let encoded = encode_index(entries)?;
    idx_file.write_all(&encoded[..])?;
    Ok(())
}

fn encode_index(idx: &mut [IndexEntry]) -> IoResult<Vec<u8>> {
    let mut encoded = index_header(idx.len())?;
    idx.sort_by(|a, b| a.path.cmp(&b.path));
    let entries =
        idx.iter()
        .map(|e| encode_entry(e))
        .collect::<Result<Vec<_>, _>>()?;
    let mut encoded_entries = entries.concat();
    encoded.append(&mut encoded_entries);
    let mut hash = sha1_hash(&encoded);
    encoded.append(&mut hash);
    Ok(encoded)
}

fn encode_entry(entry: &IndexEntry) -> IoResult<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::with_capacity(62);
    let &IndexEntry {
        ctime,
        mtime,
        device,
        inode,
        mode,
        uid,
        gid,
        size
    , ..} = entry;
    let &IndexEntry {
        ref sha,
        ref file_mode,
        ref path
    , ..} = entry;
    let flags = (path.len() & 0xFFF) as u16;
    let (encoded_type, perms) = match *file_mode {
        EntryMode::Normal | EntryMode::Executable => (8u32, mode as u32),
        EntryMode::Symlink => (10u32, 0u32),
        EntryMode::Gitlink => (14u32, 0u32),
        _ => unreachable!("Tried to create an index entry for a non-indexable object")
    };
    let encoded_mode = (encoded_type << 12) | perms;

    let path_and_padding = {
        // This is the total length of the index entry file
        // NUL-terminated and padded with enough NUL bytes to pad
        // the entry to a multiple of 8 bytes.
        //
        // The -2 is because of the amount needed to compensate for the flags
        // only being 2 bytes.
        let mut v: Vec<u8> = Vec::from_iter(path.as_bytes().iter().cloned());
        v.push(0u8);
        let padding_size = 8 - ((v.len() - 2) % 8);
        let padding = vec![0u8; padding_size];
        if padding_size != 8 {
            v.extend(padding);
        }
        v
    };

    buf.write_u32::<BigEndian>(ctime as u32)?;
    buf.write_u32::<BigEndian>(0u32)?;
    buf.write_u32::<BigEndian>(mtime as u32)?;
    buf.write_u32::<BigEndian>(0u32)?;
    buf.write_u32::<BigEndian>(device as u32)?;
    buf.write_u32::<BigEndian>(inode as u32)?;
    buf.write_u32::<BigEndian>(encoded_mode)?;
    buf.write_u32::<BigEndian>(uid as u32)?;
    buf.write_u32::<BigEndian>(gid as u32)?;
    buf.write_u32::<BigEndian>(size as u32)?;
    buf.extend_from_slice(sha);
    buf.write_u16::<BigEndian>(flags)?;
    buf.extend(path_and_padding);
    Ok(buf)
}

fn index_header(num_entries: usize) -> IoResult<Vec<u8>> {
    let mut header = Vec::with_capacity(12);
    let magic = 1145655875; // "DIRC"
    let version: u32 = 2;
    header.write_u32::<BigEndian>(magic)?;
    header.write_u32::<BigEndian>(version)?;
    header.write_u32::<BigEndian>(num_entries as u32)?;
    Ok(header)
}

pub fn sha1_hash(input: &[u8]) -> Vec<u8> {
    use crypto::digest::Digest;
    use crypto::sha1::Sha1;

    let mut hasher = Sha1::new();
    hasher.input(input);
    let mut buf = vec![0; hasher.output_bytes()];
    hasher.result(&mut buf);
    buf
}

#[allow(dead_code)]
pub fn sha1_hash_iter<'a, I: Iterator<Item=&'a [u8]>>(inputs: I) -> Vec<u8> {
    use crypto::digest::Digest;
    use crypto::sha1::Sha1;

    let mut hasher = Sha1::new();
    for input in inputs {
        hasher.input(input);
    }
    let mut buf = vec![0; hasher.output_bytes()];
    hasher.result(&mut buf);
    buf
}
