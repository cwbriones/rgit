use delta;

use flate2::read::ZlibDecoder;
use rustc_serialize::hex::ToHex;
use byteorder::{ReadBytesExt,BigEndian};

use std::fs::File;
use std::io::{Read,Seek,Cursor};
use std::io::Result as IoResult;
use std::collections::HashMap;

static MAGIC_HEADER: u32 = 1346454347; // "PACK"

pub mod refs;
pub mod object;

pub use self::object::Object;
pub use self::object::ObjectType;

// The fields version and num_objects are currently unused
#[allow(dead_code)]
pub struct PackFile {
    version: u32,
    num_objects: usize,
    objects: Vec<Object>,
    encoded_objects: Vec<u8>
}

impl PackFile {
    #[allow(unused)]
    pub fn from_file(mut file: File) -> Self {
        let mut contents = Vec::new();
        file.read_to_end(&mut contents).ok().expect("Error reading file contents");
        PackFile::parse(&contents)
    }

    pub fn parse(mut contents: &[u8]) -> Self {
        let magic = contents.read_u32::<BigEndian>().unwrap();
        let version = contents.read_u32::<BigEndian>().unwrap();
        let num_objects = contents.read_u32::<BigEndian>().unwrap() as usize;

        if magic == MAGIC_HEADER {
            let objects = Objects::new(contents, num_objects)
                .map(|(_, o)| o)
                .collect::<Vec<_>>();
            PackFile {
                version: version,
                num_objects: num_objects,
                objects: objects,
                encoded_objects: contents.to_vec()
            }
        } else {
          unreachable!("Packfile failed to parse");
        }
    }

    pub fn unpack_all(&self, repo: &str) -> IoResult<()> {
        // Initial Pass, write main objects
        // Accumulate unresolved deltas

        let mut base_objects = HashMap::new();
        let mut ref_deltas = Vec::new();

        for object in &self.objects {
            match object.obj_type {
                ObjectType::RefDelta(base) => {
                    let hex_base = base.to_hex();
                    ref_deltas.push((hex_base, object));
                },
                ObjectType::OfsDelta(_) => (),
                _ => {
                    let sha = object.sha();
                    base_objects.insert(sha, object);
                    try!(object.write(repo));
                },
            }
        }

        let total = ref_deltas.len();
        for (i, &(ref base_sha, delta)) in ref_deltas.iter().enumerate() {
            let base_object = try!(Object::read_from_disk(repo, base_sha));
            let source = &base_object.content[..];

            let patched = Object {
                obj_type: base_object.obj_type,
                content: delta::patch(source, &delta.content[..])
            };
            let percentage = (100.0 * ((i + 1) as f32) / (total as f32)) as usize;
            print!("\rResolving deltas: {}% ({}/{})", percentage, i + 1, total);
            patched.write(repo)
              .ok()
              .expect("Error writing decoded object to disk");
        }
        println!(", done.");

        // Resolve deltas
        Ok(())
    }

    ///
    /// Returns an iterator over the objects within this packfile, along
    /// with their offsets.
    ///
    pub fn objects(&self) -> Objects {
        Objects::new(&self.encoded_objects, self.num_objects)
    }
}

///
/// An iterator over the objects within a packfile, along
/// with their offsets.
///
struct Objects<'a> {
    cursor: Cursor<&'a [u8]>,
    remaining: usize
}

impl<'a> Objects<'a> {
    fn new(buf: &'a [u8], size: usize) -> Self {
        Objects { cursor: Cursor::new(buf), remaining: size }
    }
}

impl<'a> Iterator for Objects<'a> {
    type Item = (usize, Object);

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining > 0 {
            self.remaining -= 1; 
            let current_position = self.cursor.position() as usize;
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
            let object = Object {
                obj_type: obj_type,
                content: content
            };
            Some((current_position, object))
        } else {
            None
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
      z.read_to_end(&mut buf).ok().expect("Error reading object contents");
      if size != buf.len() {
          panic!("Size does not match for expected object contents")
      }
      (buf, z.total_in() + current)
    };
    in_data.seek(SeekFrom::Start(new_pos)).ok().expect("Error rewinding packfile data");
    content
}

fn read_object_type<R>(r: &mut R, id: u8) -> Option<ObjectType> where R: Read {
    match id {
        1 => Some(ObjectType::Commit),
        2 => Some(ObjectType::Tree),
        3 => Some(ObjectType::Blob),
        4 => Some(ObjectType::Tag),
        6 => {
            Some(ObjectType::OfsDelta(read_offset(r)))
        },
        7 => {
            let mut base: [u8; 20] = [0; 20];
            for i in 0..20 {
                base[i] = r.read_u8().unwrap();
            }
            Some(ObjectType::RefDelta(base))
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

