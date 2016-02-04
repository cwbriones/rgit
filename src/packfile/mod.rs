pub mod refs;
mod object;
mod index;

pub use self::object::Object as PackObject;
pub use self::object::ObjectType as PackObjectType;
pub use self::index::PackIndex;

use flate2::read::ZlibDecoder;
use rustc_serialize::hex::ToHex;
use byteorder::{ReadBytesExt,BigEndian};

use std::fs::File;
use std::io::{Read,Seek,Cursor};
use std::io::Result as IoResult;
use std::collections::HashMap;

use store;
use delta;
use self::object::ObjectType::*;

static MAGIC_HEADER: u32 = 1346454347; // "PACK"

// The fields version and num_objects are currently unused
#[allow(dead_code)]
pub struct PackFile {
    version: u32,
    num_objects: usize,
    objects: HashMap<String, PackObject>,
    encoded_objects: Vec<u8>,
    sha: String,
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

    pub fn find_by_sha(&self, sha: &str) -> Option<PackObject> {
        self.objects.get(sha).cloned()
    }

    pub fn sha(&self) -> &str {
        &self.sha
    }
//
//    pub fn find_by_offset(&self, offset: usize) -> Option<Object> {
//        let mut cursor = Cursor::new(self.encoded_objects);
//        cursor.seek(offset as u64);
//    }
}

///
/// An iterator over the objects within a packfile, along
/// with their offsets.
///
pub struct Objects<'a> {
    cursor: Cursor<&'a [u8]>,
    remaining: usize,
    base_objects: HashMap<String, PackObject>,
    ref_deltas: Vec<(usize, String, PackObject)>,
    resolve: bool,
}

impl<'a> Objects<'a> {
    fn new(buf: &'a [u8], size: usize) -> Self {
        Objects {
            cursor: Cursor::new(buf),
            remaining: size,
            ref_deltas: Vec::new(),
            base_objects: HashMap::new(),
            resolve: false
        }
    }

    fn read_object(&mut self) -> PackObject {
        let mut c = self.cursor.read_u8().unwrap();
        let type_id = (c >> 4) & 7;

        let mut size: usize = (c & 15) as usize;
        let mut shift: usize = 4;

        // Parse the variable length size header for the object.
        // Read the MSB and check if we need to continue
        // consuming bytes to get the object size
        while c & 0x80 > 0 {
            c = self.cursor.read_u8().unwrap();
            size += ((c & 0x7f) as usize) << shift;
            shift += 7;
        }

        let obj_type = read_object_type(&mut self.cursor, type_id).expect(
            "Error parsing object type in packfile"
            );
        let content = read_object_content(&mut self.cursor, size);

        PackObject {
            obj_type: obj_type,
            content: content
        }
    }
}

impl<'a> Iterator for Objects<'a> {
    type Item = (usize, PackObject);

    fn next(&mut self) -> Option<Self::Item> {
        // First yield all the base objects
        while self.remaining > 0 {
            self.remaining -= 1;
            let offset = self.cursor.position() as usize;
            let object = self.read_object();

            match object.obj_type {
                RefDelta(base) => {
                    let hex_base = base.to_hex();
                    self.ref_deltas.push((offset, hex_base, object));
                },
                OfsDelta(_) => (),
                _ => {
                    let sha = object.sha();
                    self.base_objects.insert(sha, object.clone());
                    return Some((offset, object))
                },
            }
        }
        if !self.resolve {
            self.resolve = true;
            self.ref_deltas.reverse();
        }
        // Then resolve and yield all the delta objects
        match self.ref_deltas.pop() {
            Some((offset, base, delta)) => {
                let patched = {
                    let base_object = self.base_objects.get(&base).unwrap();
                    let source = &base_object.content[..];
                    PackObject {
                        obj_type: base_object.obj_type,
                        content: delta::patch(source, &delta.content[..])
                    }
                };
                let sha = patched.sha();
                self.base_objects.insert(sha, patched.clone());
                Some((offset, patched))
            },
            None => None
        }
    }
}

// Reads exactly size bytes of zlib inflated data from the filestream.
fn read_object_content(in_data: &mut Cursor<&[u8]>, size: usize) -> Vec<u8> {
    use std::io::Seek;
    use std::io::SeekFrom;

    let current = in_data.position();

    let (content, new_pos) = {
      let mut z = ZlibDecoder::new(in_data.by_ref());
      let mut buf = Vec::with_capacity(size);
      z.read_to_end(&mut buf).expect("Error reading object contents");
      if size != buf.len() {
          panic!("Size does not match for expected object contents")
      }
      (buf, z.total_in() + current)
    };
    in_data.seek(SeekFrom::Start(new_pos)).expect("Error rewinding packfile data");
    content
}

// FIXME: This should return IoResult with the only error being UnexpectedEof
fn read_object_type<R>(r: &mut R, id: u8) -> Option<PackObjectType> where R: Read {
    match id {
        1 => Some(Commit),
        2 => Some(Tree),
        3 => Some(Blob),
        4 => Some(Tag),
        6 => {
            Some(OfsDelta(read_offset(r)))
        },
        7 => {
            let mut base: [u8; 20] = [0; 20];
            for item in &mut base {
                *item = r.read_u8().unwrap();
            }
            Some(RefDelta(base))
        }
        _ => None
    }
}

// Offset encoding.
// n bytes with MSB set in all but the last one.
// The offset is then the number constructed
// by concatenating the lower 7 bits of each byte, and
// for n >= 2 adding 2^7 + 2^14 + ... + 2^(7*(n-1))
// to the result.
fn read_offset<R>(r: &mut R) -> u8 where R: Read {
    let mut shift = 0;
    let mut c = 0x80;
    let mut offset = 0;
    while c & 0x80 > 0 {
        c = r.read_u8().unwrap();
        offset += (c & 0x7f) << shift;
        shift += 7;
    }
    offset
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

