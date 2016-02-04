pub mod refs;
mod index;

pub use self::index::PackIndex;

use flate2::read::ZlibDecoder;
use rustc_serialize::hex::ToHex;
use byteorder::{ReadBytesExt,BigEndian};

use std::fs::File;
use std::io::{Read,Seek,Cursor,SeekFrom};
use std::io::Result as IoResult;
use std::collections::HashMap;

use store;
use store::{GitObject, GitObjectType};
use delta;

static MAGIC_HEADER: u32 = 1346454347; // "PACK"

// The fields version and num_objects are currently unused
#[allow(dead_code)]
pub struct PackFile {
    version: u32,
    num_objects: usize,
    objects: HashMap<String, GitObject>,
    encoded_objects: Vec<u8>,
    sha: String,
}

///
/// An object in the packfile, which may or may not be delta encoded.
///
pub enum PackObject {
    Base(GitObject),
    OfsDelta(usize, Vec<u8>),
    RefDelta([u8; 20], Vec<u8>),
}

impl PackFile {
    #[allow(unused)]
    pub fn from_file(mut file: File) -> IoResult<Self> {
        let mut contents = Vec::new();
        try!(file.read_to_end(&mut contents));
        PackFile::parse(&contents)
    }

    pub fn parse(mut contents: &[u8]) -> IoResult<Self> {
        let sha_computed = store::sha1_hash_hex(&contents[..contents.len() - 20]);

        let magic = try!(contents.read_u32::<BigEndian>());
        let version = try!(contents.read_u32::<BigEndian>());
        let num_objects = try!(contents.read_u32::<BigEndian>()) as usize;

        if magic == MAGIC_HEADER {
            let objects = Objects::new(contents, num_objects)
                .map(|(_, o)| (o.sha(), o))
                .collect::<HashMap<_, _>>();
            // Get the last 20 bytes to read the sha
            let contents_len = contents.len();

            let sha = &contents[(contents_len - 20)..contents_len].to_hex();
            assert_eq!(sha, &sha_computed);

            Ok(PackFile {
                version: version,
                num_objects: num_objects,
                objects: objects,
                encoded_objects: contents.to_vec(),
                sha: sha_computed
            })
        } else {
          unreachable!("Packfile failed to parse");
        }
    }

    #[allow(dead_code)]
    pub fn unpack_all(&self, repo: &str) -> IoResult<()> {
        for (_, object) in self.objects() {
            try!(object.write(repo))
        }
        Ok(())
    }

    ///
    /// Returns an iterator over the objects within this packfile, along
    /// with their offsets.
    ///
    pub fn objects(&self) -> Objects {
        Objects::new(&self.encoded_objects, self.num_objects)
    }

    pub fn find_by_sha(&self, sha: &str) -> Option<GitObject> {
        self.objects.get(sha).cloned()
    }

    pub fn sha(&self) -> &str {
        &self.sha
    }

    pub fn find_by_offset(&self, offset: usize) -> Option<GitObject> {
        //let mut cursor = Cursor::new(&self.encoded_objects[..]);
        //cursor.seek(SeekFrom::Start(offset as u64)).ok().unwrap();
        //read_object(&mut cursor).ok().and_then(|pack_obj| {
        //    match pack_obj {
        //        PackObject::Base(base) => base,
        //        PackObject::OfsDelta(offset, patch) => {
        //        }
        //        PackObject::RefDelta(sha, patch) => {
        //            self.find_by_sha(&sha).and_then(|base| {
        //                GitObject::new(
        //            })
        //        }
        //    }
        //})
        None
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
    ref_deltas: Vec<(usize, PackObject)>,
    ofs_deltas: Vec<(usize, PackObject)>,
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

    fn resolve_ref_delta(&mut self) -> Option<(usize, GitObject)> {
        match self.ref_deltas.pop() {
            Some((offset, PackObject::RefDelta(base, patch))) => {
                let patched = {
                    let sha = base.to_hex();
                    let base_object = self.base_objects.get(&sha).unwrap();
                    let source = &base_object.content[..];
                    GitObject::new(
                        base_object.obj_type,
                        delta::patch(source, &patch)
                    )
                };
                {
                    let sha = patched.sha();
                    self.base_offsets.insert(offset, sha.clone());
                    self.base_objects.insert(sha, patched.clone());
                }
                Some((offset, patched))
            },
            Some((_, _)) => unreachable!(),
            None => None
        }
    }

    fn resolve_ofs_delta(&mut self) -> Option<(usize, GitObject)> {
        match self.ofs_deltas.pop() {
            Some((offset, PackObject::OfsDelta(base, patch))) => {
                let patched = {
                    let base = offset - base;
                    let base_sha = self.base_offsets.get(&base).unwrap();
                    let base_object = self.base_objects.get(base_sha).unwrap();
                    let source = &base_object.content[..];
                    GitObject::new(base_object.obj_type, delta::patch(source, &patch))
                };
                let sha = patched.sha();
                self.base_objects.insert(sha, patched.clone());
                Some((offset, patched))
            },
            Some((_, _)) => unreachable!(),
            None => None
        }
    }
}

impl<'a> Iterator for Objects<'a> {
    type Item = (usize, GitObject);

    fn next(&mut self) -> Option<Self::Item> {
        // First yield all the base objects
        while self.remaining > 0 {
            self.remaining -= 1;
            let offset = self.cursor.position() as usize + 12;
            let object = self.read_object();

            match object {
                PackObject::OfsDelta(_, _) => self.ofs_deltas.push((offset, object)),
                PackObject::RefDelta(_, _) => self.ref_deltas.push((offset, object)),
                PackObject::Base(base) => {
                    {
                        let sha = base.sha();
                        self.base_offsets.insert(offset, sha.clone());
                        self.base_objects.insert(sha, base.clone());
                    }
                    return Some((offset, base))
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

    static IDX_FILE: &'static [u8] =
        include_bytes!(
            "../../tests/data/packs/pack-73e0a23f5ebfc74c7ea1940e2843a408ce1789d0.pack"
        );

    #[test]
    fn reading_a_packfile() {
        PackFile::parse(IDX_FILE).unwrap();
    }
}

