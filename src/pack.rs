use reader::MyReaderExt;

use flate2::read::ZlibDecoder;

use std::fs::File;
use std::io::{Read,Seek,Cursor};

static MAGIC_HEADER: u32 = 1346454347; // "PACK"

pub struct PackFile {
    version: u32,
    num_objects: u32,
    objects: Vec<PackfileObject>
}

pub struct PackfileObject {
    obj_type: PackObjectType,
    size: usize,
    content: Vec<u8>
}

#[derive(Debug)]
pub enum PackObjectType {
    Commit,
    Tree,
    Blob,
    Tag,
    OfsDelta(u8),
    RefDelta([u8; 20]),
}

impl PackFile {
    pub fn from_file(mut file: File) -> Self {
        // Read header bytes in big-endian format
        let magic = file.read_be_u32().unwrap();
        let version = file.read_be_u32().unwrap();
        let num_objects = file.read_be_u32().unwrap();

        println!("magic: {}, vsn: {}, num_objects: {}", magic, version, num_objects);

        if magic == MAGIC_HEADER {
            let objects = read_packfile_objects(&mut file, num_objects);
            PackFile {
                version: version,
                num_objects: num_objects,
                objects: objects
            }
        } else {
          unreachable!("Packfile failed to parse");
        }
    }
}

fn read_packfile_objects(file: &mut File, num_objects: u32) -> Vec<PackfileObject> {
    let mut objects = Vec::with_capacity(num_objects as usize);

    let mut contents = Vec::new();
    file.read_to_end(&mut contents).ok().expect("Error reading file contents");
    let mut cursor = Cursor::new(contents);

    for _ in 0..num_objects {
      let mut c = cursor.read_byte().unwrap();
      let type_id = (c >> 4) & 7;

      let mut size: usize = (c & 15) as usize;
      let mut shift: usize = 4;

      // Parse the variable length size header for the object.
      // Read the MSB and check if we need to continue
      // consuming bytes to get the object size
      while c & 0x80 > 0 {
          c = cursor.read_byte().unwrap();
          size += ((c & 0x7f) as usize) << shift;
          shift += 7;
      }

      let obj_type = read_object_type(&mut cursor, type_id).expect(
          "Error parsing object type in packfile"
          );

      println!("object type {:?}", obj_type);
      let content = read_object_content(&mut cursor, size);
      let obj = PackfileObject {
          obj_type: obj_type,
          size: size,
          content: content
      };
      objects.push(obj);
    }
    objects
}

// Reads exactly size bytes of zlib inflated data from the filestream.
fn read_object_content(in_data: &mut Cursor<Vec<u8>>, size: usize) -> Vec<u8> {
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

fn read_object_type<R>(r: &mut R, id: u8) -> Option<PackObjectType> where R: Read {
    match id {
        1 => Some(PackObjectType::Commit),
        2 => Some(PackObjectType::Tree),
        3 => Some(PackObjectType::Blob),
        4 => Some(PackObjectType::Tag),
        6 => {
            Some(PackObjectType::OfsDelta(read_offset(r)))
        },
        7 => {
            let mut base: [u8; 20] = [0; 20];
            for i in 0..20 {
                base[i] = r.read_byte().unwrap();
            }
            Some(PackObjectType::RefDelta(base))
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
        c = r.read_byte().unwrap();
        offset += (c & 0x7f) << shift;
        shift += 7;
    }
    offset
}

