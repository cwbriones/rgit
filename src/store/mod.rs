mod commit;
mod object;
mod tree;

use std::env;
use std::fs::{
    self,
    File,
};
use std::io::{
    self,
    Read,
    Write,
};
use std::iter::FromIterator;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{
    Path,
    PathBuf,
};

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use byteorder::{
    BigEndian,
    WriteBytesExt,
};

use self::commit::Commit;
use self::tree::{
    EntryMode,
    Tree,
    TreeEntry,
};
use crate::packfile::PackFile;
pub use crate::store::object::ObjectType;
pub use crate::store::object::PackedObject;

#[derive(Debug, Clone, Hash, PartialOrd, Ord, PartialEq, Eq)]
pub struct Sha {
    contents: [u8; 20],
}

#[derive(Debug, Clone)]
pub enum DecodeShaError {
    InvalidChar,
    InvalidLength(usize),
}

impl std::error::Error for DecodeShaError {}

impl std::fmt::Display for DecodeShaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeShaError::InvalidChar => write!(f, "invalid char"),
            DecodeShaError::InvalidLength(l) => write!(f, "invalid length: {}", l),
        }
    }
}

impl From<faster_hex::Error> for DecodeShaError {
    fn from(error: faster_hex::Error) -> Self {
        match error {
            faster_hex::Error::InvalidChar => DecodeShaError::InvalidChar,
            faster_hex::Error::InvalidLength(u) => DecodeShaError::InvalidLength(u),
        }
    }
}

impl Sha {
    pub fn from_hex(hex: &[u8]) -> Result<Self, DecodeShaError> {
        use faster_hex::hex_decode;

        let mut contents = [0u8; 20];
        if hex.len() != 40 {
            return Err(DecodeShaError::InvalidLength(hex.len()));
        }
        hex_decode(hex, &mut contents)?;
        Ok(Self { contents })
    }

    pub fn compute_from_bytes(bytes: &[u8]) -> Self {
        use sha1::{
            Digest,
            Sha1,
        };

        let contents: [u8; 20] = Sha1::digest(bytes).into();

        Self { contents }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeShaError> {
        if bytes.len() != 20 {
            return Err(DecodeShaError::InvalidLength(bytes.len()));
        }
        let mut contents = [0u8; 20];
        contents.copy_from_slice(bytes);
        Ok(Self { contents })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.contents[..]
    }

    pub fn hex(&self) -> String {
        faster_hex::hex_string(&self.contents[..])
    }
}

pub struct Repo {
    dir: String,
    pack: Option<PackFile>,
}

impl Repo {
    ///
    /// Recursively searches for and loads a repository from the enclosing
    /// directories.
    ///
    pub fn from_enclosing() -> Result<Self> {
        // Navigate upwards until we are in the repo
        let mut dir = env::current_dir()?;
        while !is_git_repo(&dir) {
            assert!(dir.pop(), "Not in a git repo");
        }

        let pack_path = Repo::find_packfile(dir.as_path())?;

        let pack = pack_path.map(|path| {
            let pctx = path.clone();
            PackFile::open(path)
                .with_context(|| format!("packfile {:?}", pctx))
                .unwrap()
        });

        Ok(Repo {
            dir: dir.to_str().unwrap().to_owned(),
            pack,
        })
    }

    fn find_packfile(dir: &Path) -> Result<Option<PathBuf>> {
        let mut pack_path = dir.to_owned();
        pack_path.push(".git");
        pack_path.push("objects");
        pack_path.push("pack");

        if pack_path.exists() {
            for dir_entry in fs::read_dir(&pack_path)? {
                let dir_entry = dir_entry.unwrap();
                let fname = dir_entry.file_name();
                let fname = fname.to_str().unwrap();
                if fname.starts_with("pack") && fname.ends_with(".pack") {
                    pack_path.push(fname);
                    return Ok(Some(pack_path));
                }
            }
        }
        Ok(None)
    }

    pub fn from_packfile(dir: &str, packfile_data: &[u8]) -> Result<Self> {
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
    pub fn checkout_head(&self) -> Result<()> {
        let tip = resolve_ref(&self.dir, "HEAD")?;
        let mut idx = Vec::new();
        // FIXME: This should also "bubble up" errors, walk needs to return a result.
        self.walk(&tip)
            .and_then(|t| self.walk_tree(&self.dir, &t, &mut idx).ok());
        write_index(&self.dir, &mut idx[..])?;
        Ok(())
    }

    pub fn walk(&self, sha: &Sha) -> Option<Tree> {
        self.read_object(sha)
            .ok()
            .and_then(|object| match object.obj_type {
                ObjectType::Commit => object.as_commit().and_then(|c| self.extract_tree(&c)),
                ObjectType::Tree => object.as_tree(),
                _ => None,
            })
    }

    fn walk_tree(&self, parent: &str, tree: &Tree, idx: &mut Vec<IndexEntry>) -> Result<()> {
        for entry in &tree.entries {
            let &TreeEntry {
                ref path,
                ref mode,
                ref sha,
            } = entry;
            let mut full_path = PathBuf::new();
            full_path.push(parent);
            full_path.push(path);
            match mode {
                EntryMode::SubDirectory => {
                    fs::create_dir_all(&full_path)?;
                    let path_str = full_path.to_str().unwrap();
                    self.walk(sha)
                        .and_then(|t| self.walk_tree(path_str, &t, idx).ok());
                }
                EntryMode::Normal | EntryMode::Executable => {
                    let object = self.read_object(sha)?;
                    let mut file = File::create(&full_path)?;
                    file.write_all(&object.content[..])?;
                    let meta = file.metadata()?;
                    let mut perms = meta.permissions();

                    let raw_mode = match *mode {
                        EntryMode::Normal => 33188,
                        _ => 33261,
                    };
                    perms.set_mode(raw_mode);
                    fs::set_permissions(&full_path, perms)?;

                    let idx_entry =
                        get_index_entry(&self.dir, full_path.to_str().unwrap(), mode.clone(), sha)?;
                    idx.push(idx_entry);
                }
                e => return Err(anyhow!("Unsupported Entry Mode {:?}", e)),
            }
        }
        Ok(())
    }

    fn extract_tree(&self, commit: &Commit) -> Option<Tree> {
        let sha = &commit.tree;
        self.read_tree(sha)
    }

    fn read_tree(&self, sha: &Sha) -> Option<Tree> {
        self.read_object(sha).ok().and_then(|obj| obj.as_tree())
    }

    pub fn read_object(&self, sha: &Sha) -> Result<PackedObject> {
        // Attempt to read from disk first
        PackedObject::open(&self.dir, sha).or_else(|_| {
            // If this isn't there, read from the packfile
            let pack = self.pack.as_ref().unwrap();
            pack.find_by_sha(sha).map(|o| o.unwrap())
        })
    }

    pub fn log(&self, rev: &str) -> Result<()> {
        let mut sha = resolve_ref(&self.dir, rev)?;
        loop {
            let object = self.read_object(&sha)?;
            let commit = object
                .as_commit()
                .expect("Tried to log an object that wasn't a commit");
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
fn resolve_ref(repo: &str, name: &str) -> Result<Sha> {
    // Check if the name is already a sha.
    let trimmed = name.trim();
    if is_hex_sha(trimmed) {
        Ok(Sha::from_hex(trimmed.as_bytes()).unwrap())
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
fn read_sym_ref(repo: &str, name: &str) -> Result<Sha> {
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
        let the_ref = contents.split("ref: ").nth(1).unwrap().trim();
        resolve_ref(repo, the_ref)
    } else {
        let trimmed = contents.trim();
        let sha = Sha::from_hex(trimmed.as_bytes()).unwrap();
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
    sha: Sha,
    file_mode: EntryMode,
    path: String,
}

// FIXME:
// This doesn't need to read the file a second time.
fn get_index_entry(root: &str, path: &str, file_mode: EntryMode, sha: &Sha) -> Result<IndexEntry> {
    let file = File::open(path)?;
    let meta = file.metadata()?;

    // We need to remove the repo path from the path we save on the index entry
    // FIXME: This doesn't need to be a path since we just discard it again
    let relative_path = PathBuf::from(path.trim_start_matches(root).trim_start_matches('/'));

    Ok(IndexEntry {
        ctime: meta.ctime(),
        mtime: meta.mtime(),
        device: meta.dev() as i32,
        inode: meta.ino(),
        mode: meta.mode() as u16,
        uid: meta.uid(),
        gid: meta.gid(),
        size: meta.size() as i64,
        sha: sha.clone(),
        path: relative_path.to_str().unwrap().to_owned(),
        file_mode,
    })
}

fn write_index(repo: &str, entries: &mut [IndexEntry]) -> Result<()> {
    let mut path = PathBuf::new();
    path.push(repo);
    path.push(".git");
    path.push("index");
    let mut idx_file = File::create(path)?;
    let encoded = encode_index(entries)?;
    idx_file.write_all(&encoded[..])?;
    Ok(())
}

fn encode_index(idx: &mut [IndexEntry]) -> Result<Vec<u8>> {
    let mut encoded = index_header(idx.len())?;
    idx.sort_by(|a, b| a.path.cmp(&b.path));
    let entries = idx
        .iter()
        .map(|e| encode_entry(e))
        .collect::<Result<Vec<_>, _>>()?;
    let mut encoded_entries = entries.concat();
    encoded.append(&mut encoded_entries);
    let sha = Sha::compute_from_bytes(&encoded);
    encoded.extend_from_slice(sha.as_bytes());
    Ok(encoded)
}

fn encode_entry(entry: &IndexEntry) -> Result<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::with_capacity(62);
    let &IndexEntry {
        ctime,
        mtime,
        device,
        inode,
        mode,
        uid,
        gid,
        size,
        ..
    } = entry;
    let &IndexEntry {
        ref sha,
        ref file_mode,
        ref path,
        ..
    } = entry;
    let flags = (path.len() & 0xFFF) as u16;
    let (encoded_type, perms) = match *file_mode {
        EntryMode::Normal | EntryMode::Executable => (8u32, mode as u32),
        EntryMode::Symlink => (10u32, 0u32),
        EntryMode::Gitlink => (14u32, 0u32),
        _ => unreachable!("Tried to create an index entry for a non-indexable object"),
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
            v.extend_from_slice(&padding[..]);
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
    buf.extend_from_slice(sha.as_bytes());
    buf.write_u16::<BigEndian>(flags)?;
    buf.extend(path_and_padding);
    Ok(buf)
}

fn index_header(num_entries: usize) -> io::Result<Vec<u8>> {
    let mut header = Vec::with_capacity(12);
    let magic = 1145655875; // "DIRC"
    let version: u32 = 2;
    header.write_u32::<BigEndian>(magic)?;
    header.write_u32::<BigEndian>(version)?;
    header.write_u32::<BigEndian>(num_entries as u32)?;
    Ok(header)
}
