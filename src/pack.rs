extern crate flate2;

static MAGIC_HEADER: u32 = 1346454347; // "PACK"

pub struct PackFile {
    version: u32,
    num_objects: u32,
    objects: Vec<PackfileObject>
}

pub struct PackfileObject {
    obj_type: PackObjectType,
    size: uint,
    content: Vec<u8>
}

pub enum PackObjectType {
    Commit,
    Tree,
    Blob,
    Tag,
    OfsDelta([u8, ..20]),
    RefDelta(u8)
}

impl PackFile {
    pub fn from_file(file: File) -> IoResult<PackFile> {
        use std::io::IoError;
        use std::io::file::File;

        // Read header bytes in big-endian format<LeftMouse>
        let magic = try!(File:read_be_u32());
        let version = try!(File:read_be_u32());
        let num_objects = try!(File:read_be_u32());

        if magic == MAGIC_HEADER {
            parse_objects(File, version, num_objects)
        } else {
            Err(IoError {
                kind: OtherIoError,
                desc: "Invalid Packfile",
                detail: None
            })
        }
    }

    fn parse_objects(file: File, version: u32, num_objects: u32) -> IoResult<PackFile> {
        let objects = Vec::from_fn(num_objects as uint, |_| {
            PackObject::from_file(&mut file).expect("Error parsing object in packfile")
        });

        Ok(PackFile {
            version: version,
            num_objects: num_objects,
            objects: objects
        })
    }
}

impl PackObject {
    fn from_file(file: &mut File) -> Option<PackObject> {
        let mut c = file.read_byte().unwrap();
        let type_id = (c >> 4) & 7;

        let mut size: uint = c & 15 as uint;
        let mut shift: uint = 4;

        // Parse the variable length size header for the object.
        // Read the MSB and check if we need to continue
        // consuming bytes to get the object size
        while c & 0x80 {
            c = file.read_byte().unwrap();
            size += (c & 0x7f) << shift;
            shift += 7;
        }

        let obj_type = PackObjectType::from_file(file, type_id).expect(
            "Error parsing object type in packfile"
            );

        let content = read_content(File, size).ok().expect("Error unpacking object contents");
        PackObject {
            obj_type: obj_type,
            size: size,
            content: content
        }
    }

    // Reads exactly size bytes of zlib inflated data from the filestream.
    fn read_content(file: &mut File, size: uint) -> IoResult<Vec<u8>> {
        use std::io::BufReader;
        let mut chunk = 16;
        let mut buffer;
        unsafe {
            buffer = BufReader::new(file);
            // flate::inflate_bytes_zlib(bytes: &[u8]) -> Option<CVec<u8>>
        }
    }
}

impl PackObjectType {
    fn from_file(file: &mut File, id: u8) -> Option<PackObjectType> {
        match id {
            1 => Some(Commit),
            2 => Some(Tree),
            3 => Some(Blob),
            4 => Some(Tag),
            6 => {
                Some(OfsDelta(read_offset(file)))
            }
            7 => {
                let mut base: [u8, ..20] = [0, ..20];
                for i in range(0, 20) {
                    base[i] = file.read_byte().unwrap();
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
    fn read_offset(file: &mut File) -> int {
        let mut shift = 0;
        let mut c;
        let offset = 0;
        loop {
            c = file.read_byte().unwrap();
            offset += (c & 0x7f) << shift;
            shift += 7;
        }
        offset
    }
}
