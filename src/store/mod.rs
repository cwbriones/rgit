mod commit;
mod tree;

use std::fs::File;
use std::io::{Read,Write};
use std::io::Result as IoResult;
use std::path;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::env;

use std::iter::FromIterator;

use byteorder::{BigEndian, WriteBytesExt};
use byteorder::Result as BoResult;

use rustc_serialize::hex::FromHex;

use packfile::object::{Object,ObjectType};
use self::tree::{Tree,TreeEntry,EntryMode};
use self::commit::Commit;


///
/// Resolves the head SHA and attempts to create the file structure
/// of the repository.
///
pub fn checkout_head(repo: &str) -> IoResult<()> {
    let tip = try!(read_sym_ref(repo, "HEAD"));
    let mut idx = Vec::new();
    // FIXME: This should also "bubble up" errors
    walk(repo, &tip).and_then(|t| walk_tree(repo, repo, &t, &mut idx).ok());
    try!(write_index(repo, &mut idx[..]));
    Ok(())
}

fn walk(repo: &str, sha: &str) -> Option<Tree> {
    Object::read_from_disk(repo, sha).ok().and_then(|object| {
        match object.obj_type {
            ObjectType::Commit => {
                Commit::from_packfile_object(object).and_then(|c| extract_tree(repo, &c))
            },
            ObjectType::Tree => {
                Tree::from_packfile_object(object)
            },
            _ => None
        }
    })
}

fn walk_tree(repo: &str, parent: &str, tree: &Tree, idx: &mut Vec<IndexEntry>) -> IoResult<()> {
    for entry in &tree.entries {
        let &TreeEntry {
            ref path, 
            ref mode, 
            ref sha
        } = entry;
        let mut full_path = path::PathBuf::new();
        full_path.push(parent);
        full_path.push(path);
        match *mode {
            EntryMode::SubDirectory => {
                try!(fs::create_dir_all(&full_path));
                let path_str = full_path.to_str().unwrap();
                walk(repo, sha).and_then(|t| {
                    walk_tree(repo, path_str, &t, idx).ok()
                });
            },
            EntryMode::Normal => {
                let object = try!(Object::read_from_disk(repo, &sha));
                // FIXME: Need to properly set the file mode here.
                let mut file = try!(File::create(&full_path));
                try!(file.write_all(&object.content[..]));
                let idx_entry = try!(get_index_entry(
                    full_path.to_str().unwrap(), 
                    mode.clone(), 
                    sha.clone()));
                idx.push(idx_entry);
            },
            _ => panic!("Unsupported Entry Mode")
        }
    }
    Ok(())
}

fn extract_tree(repo: &str, commit: &Commit) -> Option<Tree> {
    let sha = &commit.tree;
    read_tree(repo, sha)
}

fn read_tree(repo: &str, sha: &str) -> Option<Tree> {
    use std::str;
    Object::read_from_disk(repo, sha).ok().and_then(|object| {
        Tree::from_packfile_object(object)
    })
}

///
/// Reads the symbolic ref and resolve it to the actual SHA it points to, if any.
///
pub fn read_sym_ref(repo: &str, name: &str) -> IoResult<String> {
    // Read the symbolic ref directly and parse the actual ref out
    let mut root = path::PathBuf::new();
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
    Ok(sha.trim().to_string())
}

fn resolve_tree(sha: &str) -> IoResult<()> {
    Ok(())
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
    let mut file = try!(File::open(path));
    let mut meta = try!(file.metadata());

    // We need to remove the repo path from the path we save on the index entry
    let iter = path::Path::new(path)
        .components()
        .skip(1)
        .map(|c| c.as_os_str());
    let relative_path =  path::PathBuf::from_iter(iter);
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
        path: relative_path.to_str().unwrap().to_string()
    })
}

fn write_index(repo: &str, entries: &mut [IndexEntry]) -> IoResult<()> {
    let mut path = path::PathBuf::new();
    path.push(repo);
    path.push(".git");
    path.push("index");
    let mut idx_file = try!(File::create(path));
    let mut encoded = try!(encode_index(entries));
    try!(idx_file.write_all(&mut encoded[..]));
    Ok(())
}

fn encode_index(idx: &mut [IndexEntry]) -> IoResult<Vec<u8>> {
    let mut encoded = try!(index_header(idx.len()));
    idx.sort_by(|a, b| a.path.cmp(&b.path));
    let mut entries: Vec<u8> = idx.iter()
        .map(|e| encode_entry(e))
        .flat_map(|e| e.into_iter())
        .collect();
    encoded.append(&mut entries);
    // FIXME: This needs to be hex decoded
    let mut hash = sha1_hash(&encoded);
    encoded.append(&mut hash);
    Ok(encoded)
}

fn encode_entry(entry: &IndexEntry) -> Vec<u8> {
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
        let mut v: Vec<u8> = Vec::from_iter(path.as_bytes().iter().map(|u| *u));
        v.push(0u8);
        let padding_size = 8 - ((v.len() - 2) % 8);
        let padding = vec![0u8; padding_size];
        if padding_size != 8 {
            v.extend(padding);
        }
        v
    };

    buf.write_u32::<BigEndian>(ctime as u32);
    buf.write_u32::<BigEndian>(0u32);
    buf.write_u32::<BigEndian>(mtime as u32);
    buf.write_u32::<BigEndian>(0u32);
    buf.write_u32::<BigEndian>(device as u32);
    buf.write_u32::<BigEndian>(inode as u32);
    buf.write_u32::<BigEndian>(encoded_mode);
    buf.write_u32::<BigEndian>(uid as u32);
    buf.write_u32::<BigEndian>(gid as u32);
    buf.write_u32::<BigEndian>(size as u32);
    buf.extend(sha.iter());
    buf.write_u16::<BigEndian>(flags);
    buf.extend(path_and_padding);
    buf
}

fn index_header(num_entries: usize) -> BoResult<Vec<u8>> {
    let mut header = Vec::with_capacity(12);
    let magic = 1145655875; // "DIRC"
    let version: u32 = 2;
    header.write_u32::<BigEndian>(magic);
    header.write_u32::<BigEndian>(version);
    header.write_u32::<BigEndian>(num_entries as u32);
    Ok(header)
}

// TODO:
// Remove as this is duplicated in `packfile::object`
fn sha1_hash(input: &[u8]) -> Vec<u8> {
    use crypto::digest::Digest;
    use crypto::sha1::Sha1;

    let mut hasher = Sha1::new();
    hasher.input(input);
    let result = hasher.result_str();
    result.from_hex().unwrap()
}

