pub mod refs;
mod index;

pub use self::index::PackIndex;

use flate2::read::ZlibDecoder;
use rustc_serialize::hex::{FromHex,ToHex};
use byteorder::{ReadBytesExt,WriteBytesExt,BigEndian};
use crc::crc32;

use std::fs::{self, File};
use std::path::{Path,PathBuf};
use std::io::{Read,Write,Seek,Cursor,SeekFrom};
use std::io::Result as IoResult;
use std::collections::HashMap;

use store;
use store::{GitObject, GitObjectType};

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
        if let PackObject::Base(_) = *self {
            true
        } else {
            false
        }
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
}

impl PackFile {
    pub fn open<P: AsRef<Path>>(p: P) -> IoResult<Self> {
        let path = p.as_ref();
        let mut contents = Vec::new();
        let mut file = try!(File::open(path));
        try!(file.read_to_end(&mut contents));

        let idx_path = path.with_extension("idx");
        let idx = try!(PackIndex::open(idx_path));

        PackFile::parse_with_index(&contents, idx)
    }

    pub fn parse(contents: &[u8]) -> IoResult<Self> {
        PackFile::parse_with_index(contents, None)
    }

    fn parse_with_index(mut contents: &[u8], idx: Option<PackIndex>) -> IoResult<Self> {
        let sha_computed = store::sha1_hash_hex(&contents[..contents.len() - 20]);

        let magic = try!(contents.read_u32::<BigEndian>());
        let version = try!(contents.read_u32::<BigEndian>());
        let num_objects = try!(contents.read_u32::<BigEndian>()) as usize;

        if magic == MAGIC_HEADER {
            let contents_len = contents.len();
            let checksum = &contents[(contents_len - 20)..contents_len].to_hex();
            assert_eq!(checksum, &sha_computed);

            // Use slice::split_at
            contents = &contents[..contents_len - 20];
            let objects = Objects::new(&contents, num_objects);
            let index = idx.unwrap_or_else(|| PackIndex::from_objects(objects, &sha_computed));

            Ok(PackFile {
                version: version,
                num_objects: num_objects,
                encoded_objects: contents.to_vec(),
                sha: sha_computed,
                index: index
            })
        } else {
          unreachable!("Packfile failed to parse");
        }
    }

    pub fn write(&self, root: &PathBuf) -> IoResult<()> {
        let mut path = root.clone();
        path.push(format!("objects/pack"));
        try!(fs::create_dir_all(&path));
        path.push(format!("pack-{}", self.sha()));
        path.set_extension("pack");

        let mut idx_path = path.clone();
        idx_path.set_extension("idx");

        let mut pack_file = try!(File::create(&path));
        let mut idx_file = try!(File::create(&idx_path));

        let pack = try!(self.encode());
        try!(pack_file.write_all(&pack));

        let idx = try!(self.index.encode());
        try!(idx_file.write_all(&idx));
        Ok(())
    }

    ///
    /// Returns an iterator over the objects within this packfile, along
    /// with their offsets.
    ///
    #[allow(unused)]
    pub fn objects(&self) -> Objects {
        Objects::new(&self.encoded_objects, self.num_objects)
    }

    pub fn encode(&self) -> IoResult<Vec<u8>> {
        let mut encoded = Vec::with_capacity(HEADER_LENGTH + self.encoded_objects.len());
        try!(encoded.write_u32::<BigEndian>(MAGIC_HEADER));
        try!(encoded.write_u32::<BigEndian>(self.version));
        try!(encoded.write_u32::<BigEndian>(self.num_objects as u32));
        try!(encoded.write_all(&self.encoded_objects));
        let checksum = store::sha1_hash(&encoded);
        try!(encoded.write_all(&checksum));
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
            Some(offset) => Ok(Some(try!(self.read_at_offset(offset)))),
            None => Ok(None)
        }
    }

    fn find_by_offset(&self, mut offset: usize) -> IoResult<Option<GitObject>> {
        // Read the initial offset.
        //
        // If it is a base object, return the enclosing object.
        let mut object = try!(self.read_at_offset(offset));
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
                    next = Some(try!(self.read_at_offset(offset)));
                },
                PackObject::RefDelta(sha, patch) => {
                    patches.push(patch);
                    next = try!(self.find_by_sha_unresolved(&sha.to_hex()));
                },
                _ => unreachable!()
            }
            if next.is_some() {
                object = next.unwrap()
            } else {
                // This should be an error that the object is incomplete
                return Ok(None)
            }
        }
        // The patches then look like: vec![patch3, patch2, patch1]
        //
        // These patches are then popped off the end, applied in turn to create the desired object.
        // We could cache these results along the way in some offset cache to avoid repeatedly
        // recreating the chain for any object along it, but this shouldn't be necessary
        // for most operations since we will only be concerned with the tip of the chain.
        for patch in patches.pop() {
            object = object.patch(&patch).unwrap();
            // TODO: Cache here
        }
        Ok(Some(object.unwrap()))
    }

    fn read_at_offset(&self, offset: usize) -> IoResult<PackObject> {
        let total_offset = offset - HEADER_LENGTH;
        let mut cursor = Cursor::new(&self.encoded_objects[..]);
        cursor.seek(SeekFrom::Start(total_offset as u64)).ok().unwrap();
        read_object(&mut cursor)
    }
}

///
/// An iterator over the objects within a packfile, along
/// with their offsets.
///
pub struct Objects<'a> {
    cursor: Cursor<&'a [u8]>,
    remaining: usize,
    base_objects: HashMap<String, GitObject>,
    base_offsets: HashMap<usize, String>,
    ref_deltas: Vec<(usize, u32, PackObject)>,
    ofs_deltas: Vec<(usize, u32, PackObject)>,
    resolve: bool,
}

impl<'a> Objects<'a> {
    fn new(buf: &'a [u8], size: usize) -> Self {
        Objects {
            cursor: Cursor::new(buf),
            remaining: size,
            ref_deltas: Vec::new(),
            base_objects: HashMap::new(),
            base_offsets: HashMap::new(),
            ofs_deltas: Vec::new(),
            resolve: false
        }
    }

    fn read_object(&mut self) -> PackObject {
        read_object(&mut self.cursor).unwrap()
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
                self.base_objects.insert(sha, patched.clone());
                Some((offset, checksum, patched))
            },
            Some(_) => unreachable!(),
            None => None
        }
    }
}

impl<'a> Iterator for Objects<'a> {
    // (offset, crc32, Object)
    type Item = (usize, u32, GitObject);

    fn next(&mut self) -> Option<Self::Item> {
        // First yield all the base objects
        while self.remaining > 0 {
            self.remaining -= 1;
            let offset = self.cursor.position() as usize + HEADER_LENGTH;
            let object = self.read_object();
            let checksum = {
                let contents = self.cursor.get_ref();
                let end = self.cursor.position() as usize;

                let compressed_object = &contents[offset - HEADER_LENGTH..end];
                crc32::checksum_ieee(&compressed_object)
            };

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

fn read_object(mut cursor: &mut Cursor<&[u8]>) -> IoResult<PackObject> {
    let mut c = try!(cursor.read_u8());
    let type_id = (c >> 4) & 7;

    let mut size: usize = (c & 15) as usize;
    let mut shift: usize = 4;

    // Parse the variable length size header for the object.
    // Read the MSB and check if we need to continue
    // consuming bytes to get the object size
    while c & 0x80 > 0 {
        c = try!(cursor.read_u8());
        size += ((c & 0x7f) as usize) << shift;
        shift += 7;
    }

    match type_id {
        1 | 2 | 3 | 4 => {
            let content = try!(read_object_content(&mut cursor, size));
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
            let offset = try!(read_offset(&mut cursor));
            let content = try!(read_object_content(&mut cursor, size));
            Ok(PackObject::OfsDelta(offset, content))
        },
        7 => {
            let mut base: [u8; 20] = [0; 20];
            try!(cursor.read_exact(&mut base));
            let content = try!(read_object_content(&mut cursor, size));
            Ok(PackObject::RefDelta(base, content))
        }
        _ => panic!("Unexpected id for git object")
    }
}

// Reads exactly size bytes of zlib inflated data from the filestream.
fn read_object_content(in_data: &mut Cursor<&[u8]>, size: usize) -> IoResult<Vec<u8>> {
    use std::io::Seek;
    use std::io::SeekFrom;

    let current = in_data.position();

    let (content, new_pos) = {
      let mut z = ZlibDecoder::new(in_data.by_ref());
      let mut buf = Vec::with_capacity(size);
      try!(z.read_to_end(&mut buf));
      if size != buf.len() {
          panic!("Size does not match for expected object contents")
      }
      (buf, z.total_in() + current)
    };
    try!(in_data.seek(SeekFrom::Start(new_pos)));
    Ok(content)
}

// Offset encoding.
// n bytes with MSB set in all but the last one.
// The offset is then the number constructed
// by concatenating the lower 7 bits of each byte, and
// for n >= 2 adding 2^7 + 2^14 + ... + 2^(7*(n-1))
// to the result.
fn read_offset<R>(r: &mut R) -> IoResult<usize> where R: Read {
    let bytes = try!(read_msb_bytes(r));
    let mut offset = (bytes[0] & 0x7f) as usize;
    for b in &bytes[1..] {
        offset += 1;
        offset <<= 7;
        offset += (b & 0x7f) as usize;
    }
    Ok(offset)
}

fn read_msb_bytes<R: Read>(r: &mut R) -> IoResult<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut c = try!(r.read_u8());
    while c & 0x80 != 0 {
        bytes.push(c);
        c = try!(r.read_u8());
    }
    bytes.push(c);
    Ok(bytes)
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
}

