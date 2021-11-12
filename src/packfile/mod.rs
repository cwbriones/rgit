mod index;
pub mod refs;

use std::collections::HashMap;
use std::fs::{
    self,
    File,
};
use std::io::{
    self,
    BufRead,
    Read,
    Write,
};
use std::path::Path;

use anyhow::{
    anyhow,
    Context,
    Result,
};
use byteorder::{
    BigEndian,
    ReadBytesExt,
    WriteBytesExt,
};
use crc32fast::Hasher as CrcHasher;

pub use self::index::PackIndex;
use crate::store::{
    ObjectType,
    PackedObject,
    Sha,
};

static MAGIC_HEADER: u32 = 1346454347; // "PACK"
static HEADER_LENGTH: usize = 12; // Magic + Len + Version

// The fields version and num_objects are currently unused
#[allow(dead_code)]
pub struct PackFile {
    version: u32,
    num_objects: usize,
    encoded_objects: Vec<u8>,
    sha: Sha,
    // TODO: Fix this since this is only used in a verification test.
    pub index: PackIndex,
}

///
/// An object entry in the packfile, which may or may not be delta encoded.
///
pub enum PackEntry {
    Base(PackedObject),
    OfsDelta(OfsDelta),
    RefDelta(RefDelta),
}

pub struct OfsDelta {
    offset: usize,
    patch: Vec<u8>,
}

pub struct RefDelta {
    base: Sha,
    patch: Vec<u8>,
}

#[derive(Debug)]
pub struct PackEntryNotFound;

impl std::fmt::Display for PackEntryNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pack entry not found")
    }
}

impl std::error::Error for PackEntryNotFound {}

impl PackEntry {
    pub fn crc32(&self) -> u32 {
        let content = match self {
            PackEntry::Base(b) => &b.content[..][..],
            PackEntry::RefDelta(d) => &d.patch[..],
            PackEntry::OfsDelta(o) => &o.patch[..],
        };
        let mut h = CrcHasher::new();
        h.update(content);
        h.finalize()
    }
}

impl PackFile {
    pub fn open<P: AsRef<Path>>(p: P) -> Result<Self> {
        let path = p.as_ref();
        let mut contents = Vec::new();
        let mut file = File::open(path)?;
        file.read_to_end(&mut contents)?;

        let idx_path = path.with_extension("idx");
        let idx = PackIndex::open(idx_path)?;

        PackFile::parse_with_index(&contents, idx)
    }

    pub fn parse(contents: &[u8]) -> Result<Self> {
        PackFile::parse_with_index(contents, None)
    }

    fn parse_with_index(mut contents: &[u8], idx: Option<PackIndex>) -> Result<Self> {
        let sha_computed = Sha::compute_from_bytes(&contents[..contents.len() - 20]);

        let magic = contents.read_u32::<BigEndian>().context("magic number")?;
        let version = contents.read_u32::<BigEndian>().context("version")?;
        let num_objects = contents.read_u32::<BigEndian>().context("num_objects")? as usize;

        if magic == MAGIC_HEADER {
            let contents_len = contents.len();
            let checksum = &contents[(contents_len - 20)..contents_len];
            assert_eq!(checksum, sha_computed.as_bytes());

            // Use slice::split_at
            contents = &contents[..contents_len - 20];
            let index = idx.unwrap_or_else(|| {
                PackIndex::from_objects(
                    Objects::new(contents, num_objects).collect(),
                    &sha_computed,
                )
            });

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

    pub fn write(&self, root: &Path) -> Result<()> {
        let mut path = root.to_path_buf();
        path.push("objects/pack");
        fs::create_dir_all(&path)?;
        path.push(format!("pack-{}", self.sha().hex()));
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

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut encoded = Vec::with_capacity(HEADER_LENGTH + self.encoded_objects.len());
        encoded.write_u32::<BigEndian>(MAGIC_HEADER)?;
        encoded.write_u32::<BigEndian>(self.version)?;
        encoded.write_u32::<BigEndian>(self.num_objects as u32)?;
        encoded.write_all(&self.encoded_objects)?;
        let checksum = Sha::compute_from_bytes(&encoded);
        encoded.write_all(checksum.as_bytes())?;
        Ok(encoded)
    }

    pub fn sha(&self) -> &Sha {
        &self.sha
    }

    pub fn find_by_sha(&self, sha: &Sha) -> Result<PackedObject> {
        self.index
            .find(sha)
            .ok_or_else(|| anyhow::Error::from(PackEntryNotFound))
            .and_then(|offset| self.find_by_offset(offset))
    }

    fn find_by_sha_unresolved(&self, sha: &Sha) -> Result<PackEntry> {
        self.index
            .find(sha)
            .ok_or_else(|| anyhow::Error::from(PackEntryNotFound))
            .and_then(|offset| self.read_at_offset(offset))
    }

    fn find_by_offset(&self, mut offset: usize) -> Result<PackedObject> {
        // Read the initial offset.
        //
        // If it is a base object, return the enclosing object.
        let mut tip = self.read_at_offset(offset)?;
        if let PackEntry::Base(base) = tip {
            return Ok(base);
        };
        // Otherwise we will have to recreate the delta object.
        //
        // To do this, we accumulate the entire delta chain into a vector by repeatedly
        // following the references to the next base object.
        //
        // We need to keep track of all the offsets so they are correct.
        let mut patches = Vec::new();

        let mut accum = loop {
            match tip {
                PackEntry::Base(b) => break b,
                PackEntry::OfsDelta(delta) => {
                    // This offset is *relative* to its own position
                    // We don't need to store multiple chains because a delta chain
                    // will either be offsets or shas but not both.
                    offset -= delta.offset;
                    patches.push(delta.patch);
                    tip = self.read_at_offset(offset)?
                }
                PackEntry::RefDelta(delta) => {
                    patches.push(delta.patch);
                    tip = self.find_by_sha_unresolved(&delta.base)?
                }
            };
        };
        // The patches then look like: vec![patch3, patch2, patch1]
        //
        // These patches are then popped off the end, applied in turn to create the desired object.
        // We could cache these results along the way in some offset cache to avoid repeatedly
        // recreating the chain for any object along it, but this shouldn't be necessary
        // for most operations since we will only be concerned with the tip of the chain.
        while let Some(patch) = patches.pop() {
            accum = accum.patch(&patch);
            // TODO: Cache here
        }
        Ok(accum)
    }

    fn read_at_offset(&self, offset: usize) -> Result<PackEntry> {
        let total_offset = offset - HEADER_LENGTH;
        let contents = &self.encoded_objects[total_offset..];
        let mut reader = EntryReader::new(contents);
        reader.read_object()
    }
}

///
/// An iterator over the objects within a packfile, along
/// with their offsets.
///
pub struct Objects<R> {
    reader: EntryReader<R>,
    remaining: usize,
    base_objects: HashMap<Sha, PackedObject>,
    base_offsets: HashMap<usize, Sha>,
    ref_deltas: Vec<(usize, u32, RefDelta)>,
    ofs_deltas: Vec<(usize, u32, OfsDelta)>,
    resolve: bool,
}

impl<R> Objects<R>
where
    R: Read + BufRead,
{
    fn new(reader: R, size: usize) -> Self {
        Objects {
            reader: EntryReader::new(reader),
            remaining: size,
            ref_deltas: Vec::new(),
            base_objects: HashMap::new(),
            base_offsets: HashMap::new(),
            ofs_deltas: Vec::new(),
            resolve: false,
        }
    }

    fn resolve_ref_delta(&mut self) -> Option<(usize, u32, PackedObject)> {
        match self.ref_deltas.pop() {
            Some((offset, checksum, delta)) => {
                let patched = {
                    let base_object = self.base_objects.get(&delta.base).unwrap();
                    base_object.patch(&delta.patch)
                };
                {
                    let sha = patched.sha();
                    self.base_offsets.insert(offset, sha.clone());
                    self.base_objects.insert(sha, patched.clone());
                }
                Some((offset, checksum, patched))
            }
            None => None,
        }
    }

    fn resolve_ofs_delta(&mut self) -> Option<(usize, u32, PackedObject)> {
        match self.ofs_deltas.pop() {
            Some((offset, checksum, delta)) => {
                let patched = {
                    let base = offset - delta.offset;
                    let base_sha = self.base_offsets.get(&base).unwrap();
                    let base_object = self.base_objects.get(base_sha).unwrap();
                    base_object.patch(&delta.patch)
                };
                let sha = patched.sha();
                self.base_offsets.insert(offset, sha.clone());
                self.base_objects.insert(sha, patched.clone());
                Some((offset, checksum, patched))
            }
            None => None,
        }
    }
}

impl<R> Iterator for Objects<R>
where
    R: Read + BufRead,
{
    // (offset, crc32, Object)
    type Item = (usize, u32, PackedObject);

    fn next(&mut self) -> Option<Self::Item> {
        // First yield all the base objects
        while self.remaining > 0 {
            self.remaining -= 1;
            let offset = self.reader.consumed_bytes() + HEADER_LENGTH;

            let object = self.reader.read_object().unwrap();
            let checksum = object.crc32();

            match object {
                PackEntry::OfsDelta(delta) => self.ofs_deltas.push((offset, checksum, delta)),
                PackEntry::RefDelta(delta) => self.ref_deltas.push((offset, checksum, delta)),
                PackEntry::Base(base) => {
                    {
                        let sha = base.sha();
                        self.base_offsets.insert(offset, sha.clone());
                        self.base_objects.insert(sha, base.clone());
                    }
                    return Some((offset, checksum, base));
                }
            }
        }
        if !self.resolve {
            self.resolve = true;
            self.ref_deltas.reverse();
            self.ofs_deltas.reverse();
        }
        // Then resolve and yield all the delta objects
        self.resolve_ref_delta()
            .or_else(|| self.resolve_ofs_delta())
    }
}

pub struct EntryReader<R> {
    inner: R,
    consumed_bytes: usize,
}

impl<R> EntryReader<R>
where
    R: Read + BufRead,
{
    pub fn new(inner: R) -> Self {
        EntryReader {
            inner,
            consumed_bytes: 0,
        }
    }

    pub fn read_object(&mut self) -> Result<PackEntry> {
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
                let content = self.decompress_content(size)?;
                let base_type = match type_id {
                    1 => ObjectType::Commit,
                    2 => ObjectType::Tree,
                    3 => ObjectType::Blob,
                    4 => ObjectType::Tag,
                    _ => unreachable!(),
                };
                Ok(PackEntry::Base(PackedObject::new(base_type, content)))
            }
            6 => {
                let offset = self.read_offset()?;
                let content = self.decompress_content(size)?;
                Ok(PackEntry::OfsDelta(OfsDelta {
                    offset,
                    patch: content,
                }))
            }
            7 => {
                let mut base_content: [u8; 20] = [0; 20];
                self.read_exact(&mut base_content)?;
                let base = Sha::from_bytes(&base_content[..]).unwrap();

                let content = self.decompress_content(size)?;
                Ok(PackEntry::RefDelta(RefDelta {
                    base,
                    patch: content,
                }))
            }
            _ => return Err(anyhow!("unexpected id for git object: {}", type_id)),
        }
    }

    // Offset encoding.
    // n bytes with MSB set in all but the last one.
    // The offset is then the number constructed
    // by concatenating the lower 7 bits of each byte, and
    // for n >= 2 adding 2^7 + 2^14 + ... + 2^(7*(n-1))
    // to the result.
    fn read_offset(&mut self) -> io::Result<usize> {
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

    #[inline]
    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.consumed_bytes += buf.len();
        self.inner.read_exact(buf)
    }

    #[inline]
    fn read_u8(&mut self) -> io::Result<u8> {
        self.consumed_bytes += 1;
        self.inner.read_u8()
    }

    fn decompress_content(&mut self, size: usize) -> Result<Vec<u8>> {
        let mut object_buffer = Vec::with_capacity(size);

        use flate2::Decompress;
        use flate2::Flush;
        use flate2::Status;

        let mut decompressor = Decompress::new(true);
        loop {
            let last_total_in = decompressor.total_in();
            let res = {
                let zlib_buffer = self.inner.fill_buf()?;
                decompressor.decompress_vec(zlib_buffer, &mut object_buffer, Flush::None)
            };
            let nread = decompressor.total_in() - last_total_in;
            self.inner.consume(nread as usize);
            self.consumed_bytes += nread as usize;

            match res {
                Ok(Status::StreamEnd) => {
                    if decompressor.total_out() as usize != size {
                        return Err(anyhow!(
                            "decompressed size does not match header: {} != {}",
                            decompressor.total_out(),
                            size,
                        ));
                    }
                    return Ok(object_buffer);
                }
                // The assumption is that reaching BufError is truly an error.
                // - Input case: The input buffer is empty. We read the buffer before every call to
                // `decompress_vec`, so if the returned buf is empty then we have truly exhausted
                // the reader before completing the stream.
                // - Output case: The object buffer was filled before end of stream. This means
                // that the header we initially read was incorrect and the vec was not
                // sized.
                Ok(Status::BufError) => return Err(anyhow!("zlib buffer error")),
                Ok(Status::Ok) => (),
                Err(e) => return Err(anyhow!("zlib decompression error: {}", e)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Read;

    use super::*;

    static PACK_FILE: &'static str =
        "tests/data/packs/pack-79f006bb5e8d079fdbe07e7ce41f97f4db7d341c.pack";

    static BASE_OFFSET: usize = 2154;
    static BASE_SHA: &'static [u8] = b"7e690abcc93718dbf26ddea5c6ede644a63a5b34";
    // We need to test reading an object with a non-trivial delta
    // chain (4).
    static DELTA_SHA: &'static [u8] = b"9b104dc31028e46f2f7d0b8a29989ab9a5155d41";
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
        pack.find_by_offset(BASE_OFFSET).unwrap();
        // Read a deltified object
        pack.find_by_offset(DELTA_OFFSET).unwrap();
    }

    #[test]
    fn reading_a_packed_object_by_sha() {
        let pack = read_pack();
        // Read a base object
        pack.find_by_sha(&Sha::from_hex(BASE_SHA).unwrap()).unwrap();
        // Read a deltified object
        pack.find_by_sha(&Sha::from_hex(DELTA_SHA).unwrap())
            .unwrap();
    }

    #[test]
    fn reading_delta_objects_should_resolve_them_correctly() {
        use std::str;
        let pack = read_pack();
        let delta = pack
            .find_by_sha(&Sha::from_hex(DELTA_SHA).unwrap())
            .unwrap();
        let content = str::from_utf8(&delta.content[..]).unwrap();
        assert_eq!(content, DELTA_CONTENT);
    }
}
