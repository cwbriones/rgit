pub mod refs;
mod index;

pub use self::index::PackIndex;

use rustc_serialize::hex::{FromHex,ToHex};
use byteorder::{ReadBytesExt,WriteBytesExt,BigEndian};
use crc::Crc;

use std::fs::{self, File};
use std::path::Path;
use std::io::{Read,Write};
use std::io::Result as IoResult;
use std::collections::HashMap;

use crate::store;
use crate::store::{GitObject, GitObjectType};

static MAGIC_HEADER: u32 = 1346454347; // "PACK"
static HEADER_LENGTH: usize = 12; // Magic + Len + Version

// The fields version and num_objects are currently unused
#[allow(dead_code)]
pub struct PackFile {
    version: u32,
    num_objects: usize,
    encoded_objects: Vec<u8>,
    sha: String,
    // TODO: Fix this since this is only used in a verification test.
    pub index: PackIndex,
}

///
/// An object in the packfile, which may or may not be delta encoded.
///
pub enum PackObject {
    Base(GitObject),
    OfsDelta(usize, Vec<u8>),
    RefDelta([u8; 20], Vec<u8>),
}

impl PackObject {
    pub fn is_base(&self) -> bool {
        matches!(*self, PackObject::Base(_))
    }

    pub fn unwrap(self) -> GitObject {
        match self {
            PackObject::Base(b) => b,
            _ => panic!("Called `GitObject::unwrap` on a deltified object")
        }
    }

    pub fn patch(&self, delta: &[u8]) -> Option<Self> {
        if let PackObject::Base(ref b) = *self {
            Some(PackObject::Base(b.patch(delta)))
        } else {
            None
        }
    }

    pub fn crc32(&self) -> u32 {
        let content = match *self {
            PackObject::Base(ref b) => &b.content[..][..],
            PackObject::RefDelta(_, ref c) => &c[..],
            PackObject::OfsDelta(_, ref c) => &c[..],
        };
        let crc = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
        crc.checksum(content)
    }
}

impl PackFile {
    pub fn open<P: AsRef<Path>>(p: P) -> IoResult<Self> {
        let path = p.as_ref();
        let mut contents = Vec::new();
        let mut file = File::open(path)?;
        file.read_to_end(&mut contents)?;

        let idx_path = path.with_extension("idx");
        let idx = PackIndex::open(idx_path)?;

        PackFile::parse_with_index(&contents, idx)
    }

    pub fn parse(contents: &[u8]) -> IoResult<Self> {
        PackFile::parse_with_index(contents, None)
    }

    fn parse_with_index(mut contents: &[u8], idx: Option<PackIndex>) -> IoResult<Self> {
        let sha_computed = store::sha1_hash_hex(&contents[..contents.len() - 20]);

        let magic = contents.read_u32::<BigEndian>()?;
        let version = contents.read_u32::<BigEndian>()?;
        let num_objects = contents.read_u32::<BigEndian>()? as usize;

        if magic == MAGIC_HEADER {
            let contents_len = contents.len();
            let checksum = &contents[(contents_len - 20)..contents_len].to_hex();
            assert_eq!(checksum, &sha_computed);

            // Use slice::split_at
            contents = &contents[..contents_len - 20];
            let objects = Objects::new(contents, num_objects).collect();
            let index = idx.unwrap_or_else(|| PackIndex::from_objects(objects, &sha_computed));

            Ok(PackFile {
                version,
                num_objects,
                encoded_objects: contents.to_vec(),
                sha: sha_computed,
                index,
            })
        } else {
          unreachable!("Packfile failed to parse");
        }
    }

    pub fn write(&self, root: &Path) -> IoResult<()> {
        let mut path = root.to_path_buf();
        path.push("objects/pack");
        fs::create_dir_all(&path)?;
        path.push(format!("pack-{}", self.sha()));
        path.set_extension("pack");

        let mut idx_path = path.clone();
        idx_path.set_extension("idx");

        let mut pack_file = File::create(&path)?;
        let mut idx_file = File::create(&idx_path)?;

        let pack = self.encode()?;
        pack_file.write_all(&pack)?;

        let idx = self.index.encode()?;
        idx_file.write_all(&idx)?;
        Ok(())
    }

    pub fn encode(&self) -> IoResult<Vec<u8>> {
        let mut encoded = Vec::with_capacity(HEADER_LENGTH + self.encoded_objects.len());
        encoded.write_u32::<BigEndian>(MAGIC_HEADER)?;
        encoded.write_u32::<BigEndian>(self.version)?;
        encoded.write_u32::<BigEndian>(self.num_objects as u32)?;
        encoded.write_all(&self.encoded_objects)?;
        let checksum = store::sha1_hash(&encoded);
        encoded.write_all(&checksum)?;
        Ok(encoded)
    }

    pub fn sha(&self) -> &str {
        &self.sha
    }

    pub fn find_by_sha(&self, sha: &str) -> IoResult<Option<GitObject>> {
        let off = sha.from_hex().ok().and_then(|s| self.index.find(&s));
        match off {
            Some(offset) => self.find_by_offset(offset),
            None => Ok(None)
        }
    }

    fn find_by_sha_unresolved(&self, sha: &str) -> IoResult<Option<PackObject>> {
        let off = sha.from_hex().ok().and_then(|s| self.index.find(&s));
        match off {
            Some(offset) => Ok(Some(self.read_at_offset(offset)?)),
            None => Ok(None)
        }
    }

    fn find_by_offset(&self, mut offset: usize) -> IoResult<Option<GitObject>> {
        // Read the initial offset.
        //
        // If it is a base object, return the enclosing object.
        let mut object = self.read_at_offset(offset)?;
        if let PackObject::Base(base) = object {
            return Ok(Some(base));
        };
        // Otherwise we will have to recreate the delta object.
        //
        // To do this, we accumulate the entire delta chain into a vector by repeatedly
        // following the references to the next base object.
        //
        // We need to keep track of all the offsets so they are correct.
        let mut patches = Vec::new();

        while !object.is_base() {
            let next;
            match object {
                PackObject::OfsDelta(delta_offset, patch) => {
                    patches.push(patch);
                    // This offset is *relative* to its own position
                    // We don't need to store multiple chains because a delta chain
                    // will either be offsets or shas but not both.
                    offset -= delta_offset;
                    next = Some(self.read_at_offset(offset)?);
                },
                PackObject::RefDelta(sha, patch) => {
                    patches.push(patch);
                    next = self.find_by_sha_unresolved(&sha.to_hex())?;
                },
                _ => unreachable!()
            }
            match next {
                Some(o) => { object = o; }
                // This should be an error that the object is incomplete
                None => return Ok(None)
            }
        }
        // The patches then look like: vec![patch3, patch2, patch1]
        //
        // These patches are then popped off the end, applied in turn to create the desired object.
        // We could cache these results along the way in some offset cache to avoid repeatedly
        // recreating the chain for any object along it, but this shouldn't be necessary
        // for most operations since we will only be concerned with the tip of the chain.
        while let Some(patch) = patches.pop() {
            object = object.patch(&patch).unwrap();
            // TODO: Cache here
        }
        Ok(Some(object.unwrap()))
    }

    fn read_at_offset(&self, offset: usize) -> IoResult<PackObject> {
        let total_offset = offset - HEADER_LENGTH;
        let contents = &self.encoded_objects[total_offset..];
        let mut reader = ObjectReader::new(contents);
        reader.read_object()
    }
}

///
/// An iterator over the objects within a packfile, along
/// with their offsets.
///
pub struct Objects<R> {
    reader: ObjectReader<R>,
    remaining: usize,
    base_objects: HashMap<String, GitObject>,
    base_offsets: HashMap<usize, String>,
    ref_deltas: Vec<(usize, u32, PackObject)>,
    ofs_deltas: Vec<(usize, u32, PackObject)>,
    resolve: bool,
}

impl<R> Objects<R> where R: Read {
    fn new(reader: R, size: usize) -> Self {
        Objects {
            reader: ObjectReader::new(reader),
            remaining: size,
            ref_deltas: Vec::new(),
            base_objects: HashMap::new(),
            base_offsets: HashMap::new(),
            ofs_deltas: Vec::new(),
            resolve: false
        }
    }

    fn resolve_ref_delta(&mut self) -> Option<(usize, u32, GitObject)> {
        match self.ref_deltas.pop() {
            Some((offset, checksum, PackObject::RefDelta(base, patch))) => {
                let patched = {
                    let sha = base.to_hex();
                    let base_object = self.base_objects.get(&sha).unwrap();
                    base_object.patch(&patch)
                };
                {
                    let sha = patched.sha();
                    self.base_offsets.insert(offset, sha.clone());
                    self.base_objects.insert(sha, patched.clone());
                }
                Some((offset, checksum, patched))
            },
            Some(_) => unreachable!(),
            None => None
        }
    }

    fn resolve_ofs_delta(&mut self) -> Option<(usize, u32, GitObject)> {
        match self.ofs_deltas.pop() {
            Some((offset, checksum, PackObject::OfsDelta(base, patch))) => {
                let patched = {
                    let base = offset - base;
                    let base_sha = self.base_offsets.get(&base).unwrap();
                    let base_object = self.base_objects.get(base_sha).unwrap();
                    base_object.patch(&patch)
                };
                let sha = patched.sha();
                self.base_offsets.insert(offset, sha.clone());
                self.base_objects.insert(sha, patched.clone());
                Some((offset, checksum, patched))
            },
            Some(_) => unreachable!(),
            None => None
        }
    }
}

impl<R> Iterator for Objects<R> where R: Read {
    // (offset, crc32, Object)
    type Item = (usize, u32, GitObject);

    fn next(&mut self) -> Option<Self::Item> {
        // First yield all the base objects
        while self.remaining > 0 {
            self.remaining -= 1;
            let offset = self.reader.consumed_bytes() + HEADER_LENGTH;
            let object = self.reader.read_object().unwrap();
            let checksum = object.crc32();

            match object {
                PackObject::OfsDelta(_, _) => self.ofs_deltas.push((offset, checksum, object)),
                PackObject::RefDelta(_, _) => self.ref_deltas.push((offset, checksum, object)),
                PackObject::Base(base) => {
                    {
                        let sha = base.sha();
                        self.base_offsets.insert(offset, sha.clone());
                        self.base_objects.insert(sha, base.clone());
                    }
                    return Some((offset, checksum, base))
                },
            }
        }
        if !self.resolve {
            self.resolve = true;
            self.ref_deltas.reverse();
            self.ofs_deltas.reverse();
        }
        // Then resolve and yield all the delta objects
        self.resolve_ref_delta().or_else(|| self.resolve_ofs_delta())
    }
}

use flate2::{Decompress, Flush, Status};
use std::cmp;

const BUFFER_SIZE: usize = 4 * 1024;

pub struct ObjectReader<R> {
    inner: R,
    pos: usize,
    cap: usize,
    consumed_bytes: usize,
    buf: [u8; BUFFER_SIZE],
}


impl<R> ObjectReader<R> where R: Read {
    pub fn new(inner: R) -> Self {
        ObjectReader {
            inner,
            pos: 0,
            cap: 0,
            consumed_bytes: 0,
            buf: [0; BUFFER_SIZE],
        }
    }

    pub fn read_object(&mut self) -> IoResult<PackObject> {
        let mut c = self.read_u8()?;
        let type_id = (c >> 4) & 7;

        let mut size: usize = (c & 15) as usize;
        let mut shift: usize = 4;

        // Parse the variable length size header for the object.
        // Read the MSB and check if we need to continue
        // consuming bytes to get the object size
        while c & 0x80 > 0 {
            c = self.read_u8()?;
            size += ((c & 0x7f) as usize) << shift;
            shift += 7;
        }

        match type_id {
            1 | 2 | 3 | 4 => {
                let content = self.read_object_content(size)?;
                let base_type = match type_id {
                    1 => GitObjectType::Commit,
                    2 => GitObjectType::Tree,
                    3 => GitObjectType::Blob,
                    4 => GitObjectType::Tag,
                    _ => unreachable!()
                };
                Ok(PackObject::Base(GitObject::new(base_type, content)))
            },
            6 => {
                let offset = self.read_offset()?;
                let content = self.read_object_content(size)?;
                Ok(PackObject::OfsDelta(offset, content))
            },
            7 => {
                let mut base: [u8; 20] = [0; 20];
                self.read_exact(&mut base)?;
                let content = self.read_object_content(size)?;
                Ok(PackObject::RefDelta(base, content))
            }
            _ => panic!("Unexpected id for git object")
        }
    }

    // Offset encoding.
    // n bytes with MSB set in all but the last one.
    // The offset is then the number constructed
    // by concatenating the lower 7 bits of each byte, and
    // for n >= 2 adding 2^7 + 2^14 + ... + 2^(7*(n-1))
    // to the result.
    fn read_offset(&mut self) -> IoResult<usize> {
        let mut c = self.read_u8()?;
        let mut offset = (c & 0x7f) as usize;
        while c & 0x80 != 0 {
            c = self.read_u8()?;
            offset += 1;
            offset <<= 7;
            offset += (c & 0x7f) as usize;
        }
        Ok(offset)
    }

    pub fn consumed_bytes(&self) -> usize {
        self.consumed_bytes
    }

    fn read_object_content(&mut self, size: usize) -> IoResult<Vec<u8>> {
        let mut decompressor = Decompress::new(true);
        let mut object_buffer = Vec::with_capacity(size);

        loop {
            let last_total_in = decompressor.total_in();
            let res = {
                let zlib_buffer = self.fill_buffer()?;
                decompressor.decompress_vec(zlib_buffer, &mut object_buffer, Flush::None)
            };
            let nread = decompressor.total_in() - last_total_in;
            self.consume(nread as usize);

            match res {
                Ok(Status::StreamEnd) => {
                    if decompressor.total_out() as usize != size {
                        panic!("Size does not match for expected object contents");
                    }

                    return Ok(object_buffer);
                },
                Ok(Status::BufError) => panic!("Encountered zlib buffer error"),
                Ok(Status::Ok) => (),
                Err(e) => panic!("Encountered zlib decompression error: {}", e),
            }
        }
    }

    fn fill_buffer(&mut self) -> IoResult<&[u8]> {
        // If we've reached the end of our internal buffer then we need to fetch
        // some more data from the underlying reader.
        if self.pos == self.cap {
            self.cap = self.inner.read(&mut self.buf)?;
            self.pos = 0;
        }
        Ok(&self.buf[self.pos..self.cap])
    }

    fn consume(&mut self, amt: usize) {
        self.consumed_bytes += amt;
        self.pos = cmp::min(self.pos + amt, self.cap);
    }
}

impl<R: Read> Read for ObjectReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        // If we don't have any buffered data and we're doing a massive read
        // (larger than our internal buffer), bypass our internal buffer
        // entirely.
        if self.pos == self.cap && buf.len() >= self.buf.len() {
            let nread = self.inner.read(buf)?;
            // We still want to keep track of the correct offset so
            // we consider this consumed.
            self.consumed_bytes += nread;
            return Ok(nread);
        }
        let nread = {
            let mut rem = self.fill_buffer()?;
            rem.read(buf)?
        };
        self.consume(nread);

        Ok(nread)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::fs::File;

    static PACK_FILE: &'static str =
        "tests/data/packs/pack-79f006bb5e8d079fdbe07e7ce41f97f4db7d341c.pack";

    static BASE_OFFSET: usize = 2154;
    static BASE_SHA: &'static str = "7e690abcc93718dbf26ddea5c6ede644a63a5b34";
    // We need to test reading an object with a non-trivial delta
    // chain (4).
    static DELTA_SHA: &'static str = "9b104dc31028e46f2f7d0b8a29989ab9a5155d41";
    static DELTA_OFFSET: usize = 2461;
    static DELTA_CONTENT: &'static str =
        "This is a test repo, used for testing the capabilities of the rgit tool. \
        rgit is a implementation of\n\
        the Git version control tool written in Rust.\n\n\
        This line was added on test branch.\n";

    fn read_pack() -> PackFile {
        PackFile::open(PACK_FILE).unwrap()
    }

    #[test]
    fn reading_a_packfile() {
        read_pack();
    }

    #[test]
    fn read_and_encode_should_be_inverses() {
        let pack = read_pack();
        let encoded = pack.encode().unwrap();
        let mut on_disk = Vec::with_capacity(encoded.len());
        let mut file = File::open(PACK_FILE).unwrap();
        file.read_to_end(&mut on_disk).unwrap();

        assert_eq!(on_disk, encoded);
    }

    #[test]
    fn reading_a_packed_object_by_offset() {
        let pack = read_pack();
        // Read a base object
        pack.find_by_offset(BASE_OFFSET)
            .unwrap()
            .unwrap();
        // Read a deltified object
        pack.find_by_offset(DELTA_OFFSET)
            .unwrap()
            .unwrap();
    }

    #[test]
    fn reading_a_packed_object_by_sha() {
        let pack = read_pack();
        // Read a base object
        pack.find_by_sha(BASE_SHA)
            .unwrap()
            .unwrap();
        // Read a deltified object
        pack.find_by_sha(DELTA_SHA)
            .unwrap()
            .unwrap();
    }

    #[test]
    fn reading_delta_objects_should_resolve_them_correctly() {
        use std::str;
        let pack = read_pack();
        let delta = pack.find_by_sha(DELTA_SHA)
            .unwrap()
            .unwrap();
        let content = str::from_utf8(&delta.content[..]).unwrap();
        assert_eq!(content, DELTA_CONTENT);
    }
}

