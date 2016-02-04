mod commit;
mod tree;
mod object;

pub use store::object::Object as GitObject;
pub use store::object::ObjectType as GitObjectType;

use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::io::{Read,Write};
use std::io::Result as IoResult;
use std::path::{Path,PathBuf};
use std::os::unix::fs::MetadataExt;
use std::iter::FromIterator;

use byteorder::{BigEndian, WriteBytesExt};

use rustc_serialize::hex::FromHex;

use packfile::{PackFile, PackIndex};

use self::tree::{Tree,TreeEntry,EntryMode};
use self::commit::Commit;

pub struct Repo<'a> {
    dir: &'a str,
    pack: PackFile,
    _index: Option<PackIndex>,
}

impl<'a> Repo<'a> {
    pub fn from_packfile(dir: &'a str, packfile_data: &[u8]) -> IoResult<Self> {
        let packfile = try!(PackFile::parse(&packfile_data[..]));

        let mut p = PathBuf::new();
        p.push(dir);
        p.push(".git");
        p.push("objects");
        p.push("pack");
        try!(fs::create_dir_all(&p));
        p.push(format!("pack-{}", packfile.sha()));
        p.set_extension("pack");

        try!(packfile.unpack_all(dir));

        let mut file = try!(File::create(&p));
        try!(file.write_all(&packfile_data[..]));

        let index = PackIndex::from_packfile(&packfile);

        Ok(Repo {
            dir: dir,
            pack: packfile,
            _index: Some(index)
        })
    }

    ///
    /// Resolves the head SHA and attempts to create the file structure
    /// of the repository.
    ///
    pub fn checkout_head(&self) -> IoResult<()> {
        let tip = try!(read_sym_ref(self.dir, "HEAD"));
        let mut idx = Vec::new();
        // FIXME: This should also "bubble up" errors, walk needs to return a result.
        self.walk(&tip).and_then(|t| self.walk_tree(self.dir, &t, &mut idx).ok());
        try!(write_index(self.dir, &mut idx[..]));
        Ok(())
    }

    pub fn walk(&self, sha: &str) -> Option<Tree> {
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
                    try!(fs::create_dir_all(&full_path));
                    let path_str = full_path.to_str().unwrap();
                    self.walk(sha).and_then(|t| {
                        self.walk_tree(path_str, &t, idx).ok()
                    });
                },
                EntryMode::Normal | EntryMode::Executable => {
                    let object = try!(self.read_object(sha));
                    let mut file = try!(File::create(&full_path));
                    try!(file.write_all(&object.content[..]));
                    let meta = try!(file.metadata());
                    let mut perms = meta.permissions();

                    let raw_mode = match *mode {
                        EntryMode::Normal => 33188,
                        _ => 33261
                    };
                    perms.set_mode(raw_mode);
                    try!(fs::set_permissions(&full_path, perms));

                    let idx_entry = try!(get_index_entry(
                        full_path.to_str().unwrap(),
                        mode.clone(),
                        sha.clone()));
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

    fn read_tree(&self, sha: &str) -> Option<Tree> {
        self.read_object(sha).ok().and_then(|obj| {
            obj.as_tree()
        })
    }

    pub fn read_object(&self, sha: &str) -> IoResult<GitObject> {
        let object = 
            self.pack.find_by_sha(sha).and_then(GitObject::from_raw);
        Ok(object.unwrap())
    }
}

///
/// Reads the symbolic ref and resolve it to the actual SHA it points to, if any.
///
pub fn read_sym_ref(repo: &str, name: &str) -> IoResult<String> {
    // Read the symbolic ref directly and parse the actual ref out
    let mut root = PathBuf::new();
    root.push(repo);
    root.push(".git");

    let mut sym_path = root.clone();
    sym_path.push(name);

    let mut contents = String::new();
    let mut file = try!(File::open(sym_path));
    try!(file.read_to_string(&mut contents));
    let the_ref = contents.split("ref: ").skip(1).next().unwrap().trim();

    // Read the SHA out of the actual ref
    let mut ref_path = root.clone();
    ref_path.push(the_ref);

    let mut ref_file = try!(File::open(ref_path));
    let mut sha = String::new();
    try!(ref_file.read_to_string(&mut sha));
    Ok(sha.trim().to_owned())
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

fn get_index_entry(path: &str, file_mode: EntryMode, sha: String) -> IoResult<IndexEntry> {
    let file = try!(File::open(path));
    let meta = try!(file.metadata());

    // We need to remove the repo path from the path we save on the index entry
    let iter = Path::new(path)
        .components()
        .skip(1)
        .map(|c| c.as_os_str());
    let relative_path =  PathBuf::from_iter(iter);
    // FIXME: This error is not handled.
    let decoded_sha = sha.from_hex().unwrap();

    Ok(IndexEntry {
        ctime: meta.ctime(),
        mtime: meta.mtime(),
        device: meta.dev(),
        inode: meta.ino(),
        mode: meta.mode(),
        uid: meta.uid(),
        gid: meta.gid(),
        size: meta.size(),
        sha: decoded_sha,
        file_mode: file_mode,
        path: relative_path.to_str().unwrap().to_owned()
    })
}

fn write_index(repo: &str, entries: &mut [IndexEntry]) -> IoResult<()> {
    let mut path = PathBuf::new();
    path.push(repo);
    path.push(".git");
    path.push("index");
    let mut idx_file = try!(File::create(path));
    let encoded = try!(encode_index(entries));
    try!(idx_file.write_all(&encoded[..]));
    Ok(())
}

fn encode_index(idx: &mut [IndexEntry]) -> IoResult<Vec<u8>> {
    let mut encoded = try!(index_header(idx.len()));
    idx.sort_by(|a, b| a.path.cmp(&b.path));
    let entries: Result<Vec<_>, _> =
        idx.iter()
        .map(|e| encode_entry(e))
        .collect();
    let mut encoded_entries = try!(entries).concat();
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
        EntryMode::Normal => (8u32, mode as u32),
        EntryMode::Symlink => (10u32, 0u32),
        EntryMode::Gitlink => (14u32, 0u32),
        EntryMode::Executable => (8u32, mode as u32),
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

    try!(buf.write_u32::<BigEndian>(ctime as u32));
    try!(buf.write_u32::<BigEndian>(0u32));
    try!(buf.write_u32::<BigEndian>(mtime as u32));
    try!(buf.write_u32::<BigEndian>(0u32));
    try!(buf.write_u32::<BigEndian>(device as u32));
    try!(buf.write_u32::<BigEndian>(inode as u32));
    try!(buf.write_u32::<BigEndian>(encoded_mode));
    try!(buf.write_u32::<BigEndian>(uid as u32));
    try!(buf.write_u32::<BigEndian>(gid as u32));
    try!(buf.write_u32::<BigEndian>(size as u32));
    buf.extend(sha.iter());
    try!(buf.write_u16::<BigEndian>(flags));
    buf.extend(path_and_padding);
    Ok(buf)
}

fn index_header(num_entries: usize) -> IoResult<Vec<u8>> {
    let mut header = Vec::with_capacity(12);
    let magic = 1145655875; // "DIRC"
    let version: u32 = 2;
    try!(header.write_u32::<BigEndian>(magic));
    try!(header.write_u32::<BigEndian>(version));
    try!(header.write_u32::<BigEndian>(num_entries as u32));
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

pub fn sha1_hash_hex(input: &[u8]) -> String {
    use crypto::digest::Digest;
    use crypto::sha1::Sha1;

    let mut hasher = Sha1::new();
    hasher.input(input);

    hasher.result_str()
}

