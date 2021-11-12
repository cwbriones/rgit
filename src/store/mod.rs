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
    BufWriter,
    Read,
    Write,
};
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
        use sha1::Digest;
        use sha1::Sha1;

        let contents: [u8; 20] = Sha1::digest(bytes).into();

        Self { contents }
    }

    pub fn from_array(bytes: &[u8; 20]) -> Self {
        Self { contents: *bytes }
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
        let mut idx = Index::new(idx);
        write_index(&self.dir, &mut idx)?;
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
            pack.find_by_sha(sha)
        })
    }

    pub fn log(&self, rev: &str) -> Result<()> {
        let mut sha = resolve_ref(&self.dir, rev)?;
        loop {
            let object = self.read_object(&sha)?;
            let commit = object
                .as_commit()
                .expect("Tried to log an object that wasn't a commit");
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Index {
    entries: Vec<IndexEntry>,
    extensions: Vec<IndexExtension>,
}

impl Index {
    fn new(entries: Vec<IndexEntry>) -> Self {
        Self::new_with_extensions(entries, Vec::new())
    }

    fn new_with_extensions(entries: Vec<IndexEntry>, extensions: Vec<IndexExtension>) -> Self {
        Self {
            entries,
            extensions,
        }
    }

    fn entries(&self) -> &[IndexEntry] {
        &self.entries[..]
    }

    fn entries_mut(&mut self) -> &mut [IndexEntry] {
        &mut self.entries[..]
    }

    fn extensions(&self) -> &[IndexExtension] {
        &self.extensions[..]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    ctime: GitTime,
    mtime: GitTime,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexExtension {
    sig: [u8; 4],
    contents: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GitTime {
    pub secs: u32,
    pub nanos: u32,
}

impl GitTime {
    pub fn new(secs: u32, nsecs: u32) -> Self {
        GitTime { secs, nanos: nsecs }
    }

    pub fn from_epoch_ns(ns: u64) -> Self {
        let secs = ns / 1_000_000_000;
        // I think I can just do a normal cast and it will truncate
        let nsecs = (ns - (secs * 1_000_000_000)) & (u32::MAX as u64);
        GitTime {
            secs: secs as u32,
            nanos: nsecs as u32,
        }
    }
}

fn get_index_entry(root: &str, path: &str, file_mode: EntryMode, sha: &Sha) -> Result<IndexEntry> {
    let meta = std::fs::metadata(path)?;

    // We need to remove the repo path from the path we save on the index entry
    // FIXME: This doesn't need to be a path since we just discard it again
    let relative_path = PathBuf::from(path.trim_start_matches(root).trim_start_matches('/'));

    let ctime = {
        let ctime_nsec = meta.ctime_nsec();
        if ctime_nsec < 0 {
            return Err(anyhow!("time before the epoch is unsupported"));
        }
        GitTime::from_epoch_ns(ctime_nsec as u64)
    };
    let mtime = {
        let mtime_nsec = meta.mtime_nsec();
        if mtime_nsec < 0 {
            return Err(anyhow!("time before the epoch is unsupported"));
        }
        GitTime::from_epoch_ns(mtime_nsec as u64)
    };

    Ok(IndexEntry {
        ctime,
        mtime,
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

fn write_index(repo: &str, index: &mut Index) -> Result<()> {
    let mut path = PathBuf::new();
    path.push(repo);
    path.push(".git");
    path.push("index");

    let idx_file = File::create(path)?;
    let mut idx_file = BufWriter::new(idx_file);
    encode_index(index, &mut idx_file)
}

fn encode_index<W: Write>(idx: &mut Index, w: &mut W) -> Result<()> {
    let sha = {
        let mut w = DigestWriter::new(w.by_ref());
        encode_header(idx.entries().len(), &mut w)?;
        idx.entries_mut().sort_by(|a, b| a.path.cmp(&b.path));
        for entry in idx.entries() {
            encode_entry(entry, &mut w)?;
        }
        for ext in idx.extensions() {
            w.write_all(&ext.sig[..])?;
            w.write_u32::<BigEndian>(ext.contents.len() as u32)?;
            w.write_all(&ext.contents[..])?;
        }
        w.finalize()
    };
    w.write_all(sha.as_bytes())?;
    Ok(())
}

fn encode_entry<W>(entry: &IndexEntry, w: &mut W) -> Result<()>
where
    W: Write,
{
    let mut w = CountWriter::new(w);
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
    w.write_u32::<BigEndian>(ctime.secs)?;
    w.write_u32::<BigEndian>(ctime.nanos)?;
    w.write_u32::<BigEndian>(mtime.secs)?;
    w.write_u32::<BigEndian>(mtime.nanos)?;
    w.write_u32::<BigEndian>(device as u32)?;
    w.write_u32::<BigEndian>(inode as u32)?;
    w.write_u32::<BigEndian>(encoded_mode)?;
    w.write_u32::<BigEndian>(uid as u32)?;
    w.write_u32::<BigEndian>(gid as u32)?;
    w.write_u32::<BigEndian>(size as u32)?;
    w.write_all(sha.as_bytes())?;
    w.write_u16::<BigEndian>(flags)?;
    w.write_all(path.as_bytes())?;
    w.write_u8(0u8)?;
    const ALIGN: usize = std::mem::size_of::<u64>();
    let padding_size = ALIGN - (w.total_written() % ALIGN);
    if padding_size != ALIGN {
        let padding = [0u8; ALIGN];
        w.write_all(&padding[..padding_size])?;
    }
    Ok(())
}

const GIT_INDEX_MAGIC: u32 = 1145655875; // "DIRC"
const GIT_INDEX_VERSION: u32 = 2;

fn encode_header<W>(num_entries: usize, w: &mut W) -> io::Result<()>
where
    W: Write,
{
    let version: u32 = 2;
    let magic = 1145655875; // "DIRC"
    w.write_u32::<BigEndian>(magic)?;
    w.write_u32::<BigEndian>(version)?;
    w.write_u32::<BigEndian>(num_entries as u32)?;
    Ok(())
}

struct CountWriter<W> {
    writer: W,
    total: usize,
}

impl<W: Write> CountWriter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer, total: 0 }
    }

    pub fn total_written(&self) -> usize {
        self.total
    }
}

impl<W: Write> Write for CountWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, std::io::Error> {
        let written = self.writer.write(bytes)?;
        self.total += written;
        Ok(written)
    }

    fn flush(&mut self) -> Result<(), std::io::Error> {
        self.writer.flush()
    }
}

use std::io::BufRead;
use std::io::Seek;

use byteorder::ReadBytesExt;

#[allow(unused)]
pub fn read_index<R: BufRead + Seek>(mut r: R) -> Result<Index> {
    let mut r = DigestReader::new(r);

    // Header
    let magic = r.read_u32::<BigEndian>()?;
    if magic != GIT_INDEX_MAGIC {
        return Err(anyhow!("index header magic number mismatch"));
    }
    let version = r.read_u32::<BigEndian>()?;
    if version != GIT_INDEX_VERSION {
        return Err(anyhow!("unsupported index version: {}", version));
    }
    let num_entries = r.read_u32::<BigEndian>()?;

    let mut entries = Vec::with_capacity(num_entries as usize);
    for _ in 0..num_entries {
        entries.push(read_entry(r.by_ref())?);
    }
    // Try to read extensions while we can
    let mut extensions = Vec::new();
    while let Some(ext) = read_extension(r.by_ref())? {
        extensions.push(ext);
    }
    let mut checksum = [0u8; 20];
    r.read_exact(&mut checksum[..])?;

    match r.read_u8() {
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {}
        _ => return Err(anyhow!("expected EOF")),
    }
    let sha = r.finalize();
    if sha.as_bytes() == &checksum[..] {
        Err(anyhow!("trailing checksum mismatch"))
    } else {
        Ok(Index::new_with_extensions(entries, extensions))
    }
}

fn read_extension<R: BufRead>(mut r: R) -> Result<Option<IndexExtension>> {
    let buf = r.fill_buf()?;
    if buf.len() < 4 {
        // Not enough room for signature
        return Ok(None);
    }
    let sig = &buf[..4];
    match sig {
        b"TREE" => {}
        b"REUC" => {}
        b"link" => {}
        b"UNTR" => {}
        b"FSMN" => {}
        b"EOIE" => {}
        b"IEOT" => {}
        b"sdir" => {}
        unknown if unknown.iter().all(|c| c.is_ascii()) => {}
        _ => {
            // Unknown signature or possibly not one at all
            return Ok(None);
        }
    };
    let sig_arr = [sig[0], sig[1], sig[2], sig[3]];
    r.consume(4);
    let ext_len = r.read_u32::<BigEndian>()?;
    let mut ext_contents = vec![0; ext_len as usize];
    r.read_exact(&mut ext_contents[..])?;

    Ok(Some(IndexExtension {
        sig: sig_arr,
        contents: ext_contents,
    }))
}

fn read_entry<R: BufRead>(mut r: R) -> Result<IndexEntry> {
    // FIXME: All of these casts make me nervous
    let ctime = read_time(&mut r)?;
    let mtime = read_time(&mut r)?;
    let device = r.read_u32::<BigEndian>()? as i32;
    let inode = r.read_u32::<BigEndian>()? as u64;
    let (file_mode, mode) = {
        let encoded_mode = r.read_u32::<BigEndian>()?;
        let encoded_type = encoded_mode >> 12;
        let perms = ((1 << 13) - 1) & encoded_mode;
        match (encoded_type, perms) {
            // TODO: these are a bunch of magic numbers
            // We can probably move them to a method of the type
            (8u32, 0o100644) => (EntryMode::Normal, perms),
            (8u32, 0o000644) => (EntryMode::Normal, perms),
            (8u32, 0o100755) => (EntryMode::Executable, perms),
            (10u32, 0u32) => (EntryMode::Symlink, 0),
            (14u32, 0u32) => (EntryMode::Gitlink, 0),
            _ => {
                return Err(anyhow!(
                    "unknown or unsupported file mode: type={}, perms={:0o}",
                    encoded_type,
                    perms
                ))
            }
        }
    };
    let uid = r.read_u32::<BigEndian>()?;
    let gid = r.read_u32::<BigEndian>()?;
    let size = r.read_u32::<BigEndian>()? as i64;

    let mut sha = [0u8; 20];
    r.read_exact(&mut sha[..])?;
    let sha = Sha::from_bytes(&sha[..])?;

    // FIXME: What is this?
    let _flags = r.read_u16::<BigEndian>()?;

    // Take path until nul u32
    let mut path = Vec::new();
    r.read_until(0, &mut path)?;
    path.pop();

    // Take the padding
    loop {
        let reader_buf = r.fill_buf()?;
        if reader_buf.is_empty() {
            break;
        }
        let skip = reader_buf.iter().take_while(|b| **b == 0).count();
        let reader_buf_len = reader_buf.len();
        r.consume(skip);
        if skip < reader_buf_len {
            break;
        }
    }
    let path = String::from_utf8(path)?;

    Ok(IndexEntry {
        ctime,
        mtime,
        device,
        inode,
        mode: mode as u16,
        uid,
        gid,
        size,
        sha,
        file_mode,
        path,
    })
}

fn read_time<R: Read>(mut r: R) -> Result<GitTime> {
    let sec = r.read_u32::<BigEndian>()?;
    let nsec = r.read_u32::<BigEndian>()?;
    Ok(GitTime::new(sec, nsec))
}

struct DigestReader<R> {
    inner: R,
    digest: sha1::Sha1,
}

impl<R> DigestReader<R> {
    fn new(r: R) -> Self {
        use sha1::Digest;
        use sha1::Sha1;

        Self {
            inner: r,
            digest: Sha1::new(),
        }
    }

    fn finalize(self) -> Sha {
        use sha1::Digest;

        let sha: [u8; 20] = self.digest.finalize().into();
        Sha::from_array(&sha)
    }
}

impl<R: Read> Read for DigestReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::result::Result<usize, std::io::Error> {
        use sha1::Digest;

        let count = self.inner.read(buf)?;
        self.digest.update(&buf[..count]);
        Ok(count)
    }
}

impl<R: BufRead> BufRead for DigestReader<R> {
    fn fill_buf(&mut self) -> std::result::Result<&[u8], std::io::Error> {
        self.inner.fill_buf()
    }

    fn consume(&mut self, count: usize) {
        self.inner.consume(count);
    }
}

struct DigestWriter<W> {
    writer: W,
    digest: sha1::Sha1,
}

impl<W: Write> DigestWriter<W> {
    pub fn new(writer: W) -> Self {
        use sha1::Digest;

        Self {
            writer,
            digest: Digest::new(),
        }
    }

    pub fn finalize(self) -> Sha {
        use sha1::Digest;

        let bytes: [u8; 20] = self.digest.finalize().into();
        Sha::from_bytes(&bytes[..]).unwrap()
    }
}

impl<W: Write> Write for DigestWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, std::io::Error> {
        use sha1::Digest;

        self.digest.update(bytes);
        self.writer.write(bytes)
    }

    fn flush(&mut self) -> Result<(), std::io::Error> {
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::fs::File;
    use std::io::BufReader;
    use std::io::Cursor;

    use super::*;

    #[test]
    fn test_read_write_index() -> Result<(), Box<dyn Error>> {
        let contents = read_file_contents("tests/data/indices/index")?;
        let mut index = read_index(Cursor::new(&contents[..]))?;
        // Assert that it's equal to what we expect
        let expected_entries = [
            IndexEntry {
                ctime: GitTime {
                    secs: 1636748714,
                    nanos: 79595821,
                },
                mtime: GitTime {
                    secs: 1636748714,
                    nanos: 79595821,
                },
                device: 16777220,
                inode: 94496211,
                mode: 420,
                uid: 501,
                gid: 20,
                size: 0,
                sha: Sha {
                    contents: [
                        230, 157, 226, 155, 178, 209, 214, 67, 75, 139, 41, 174, 119, 90, 216, 194,
                        228, 140, 83, 145,
                    ],
                },
                file_mode: EntryMode::Normal,
                path: "bar/baz".into(),
            },
            IndexEntry {
                ctime: GitTime {
                    secs: 1636748703,
                    nanos: 647308759,
                },
                mtime: GitTime {
                    secs: 1636748703,
                    nanos: 647308759,
                },
                device: 16777220,
                inode: 94496183,
                mode: 420,
                uid: 501,
                gid: 20,
                size: 0,
                sha: Sha {
                    contents: [
                        230, 157, 226, 155, 178, 209, 214, 67, 75, 139, 41, 174, 119, 90, 216, 194,
                        228, 140, 83, 145,
                    ],
                },
                file_mode: EntryMode::Normal,
                path: "foo".into(),
            },
        ];
        assert_eq!(index.entries_mut(), expected_entries);
        let mut encoded = Vec::new();
        encode_index(&mut index, &mut encoded)?;

        let mismatch = encoded
            .iter()
            .zip(contents.iter())
            .enumerate()
            .find(|(_, (a, b))| *a != *b)
            .map(|(i, _)| i);
        if let Some(i) = mismatch {
            println!("contents differ at position {}", i);
        }

        assert_eq!(encoded, contents, "decode/encode was not idempotent");

        Ok(())
    }

    fn read_file_contents(path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let file = File::open(path)?;
        let size = file.metadata()?.size();

        let mut contents = Vec::with_capacity(size as usize);
        BufReader::new(file).read_to_end(&mut contents)?;
        Ok(contents)
    }
}
