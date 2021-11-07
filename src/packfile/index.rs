// Reading from the index:
//
// To read from the index:
//   1. Check the leading 2 bytes of your sha, M
//   2. Length <- Fanout[M] , Number of objects with sha leading with M or less
//   3. Set start = 0 if M == 0 else Fanout[M - 1], number of objects with M or less as lead
//   4. Slice objects[start:start + length] to get the entries to search
//   5. Perform binary search
//   6. Return object SHA and offset, if any
//   7. Restore this object from the packfile.
//      - Is the object a delta?
//          -Yes => Read base recursively, parse and return object
//          -No  => Return object
//   8. Write this object to disk as a way of "caching"
//
//
// Read then becomes
//   1. Does the object exist in loose format?
//   No
//      2. Read from packfile via index,
//      3. Write in loose format, return
//   Yes
//      2. Read and return
//
use byteorder::{ReadBytesExt,WriteBytesExt,BigEndian};
use rustc_serialize::hex::{FromHex,ToHex};

use std::io;
use std::io::{Write,Read};
use std::io::Result as IoResult;
use std::fs::File;
use std::path::Path;

use crate::store;
use crate::store::GitObject;

type Sha = [u8; 20];

static MAGIC: [u8; 4] = [255, 116, 79, 99];
static VERSION: u32 = 2;

///
/// Version 2 of the Git Packfile Index containing separate
/// tables for the offsets, fanouts, and shas.
///
pub struct PackIndex {
    fanout: [u32; 256],
    offsets: Vec<u32>,
    shas: Vec<Sha>,
    checksums: Vec<u32>,
    pack_sha: String
}

impl PackIndex {
    #[allow(unused)]
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Option<Self>> {
        use std::io::Error as IoError;
        use std::io::ErrorKind;

        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(ref io_error @ IoError{..}) if ErrorKind::NotFound == io_error.kind() => return Ok(None),
            Err(io) => return Err(io),
        };
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        Self::parse(&contents).map(|parsed| {
            Some(parsed)
        })
    }

    #[allow(unused)]
    fn parse(mut content: &[u8]) -> io::Result<Self> {
        let checksum = store::sha1_hash_hex(&content[..content.len() - 20]);

        // Parse header
        let mut magic = [0; 4];
        content.read_exact(&mut magic)?;
        assert_eq!(magic, MAGIC);

        let version = content.read_u32::<BigEndian>()?;
        assert_eq!(version, VERSION);

        // Parse Fanout table
        let mut fanout = [0; 256];
        for f in fanout.iter_mut() {
            *f = content.read_u32::<BigEndian>()?;
        }
        let size = fanout[255] as usize;

        // Parse N Shas
        let mut shas = Vec::with_capacity(size);
        for _ in 0..size {
            let mut sha = [0; 20];
            content.read_exact(&mut sha)?;
            shas.push(sha);
        }

        // Parse N Checksums
        let mut checksums = Vec::with_capacity(size);
        for _ in 0..size {
            let crc = content.read_u32::<BigEndian>()?;
            checksums.push(crc);
        }

        // Parse N Offsets
        let mut offsets = Vec::with_capacity(size);
        for _ in 0..size {
            let off = content.read_u32::<BigEndian>()?;
            offsets.push(off);
        }

        // Parse trailer
        let mut pack_sha = [0; 20];
        content.read_exact(&mut pack_sha)?;

        let mut idx_sha = [0; 20];
        content.read_exact(&mut idx_sha)?;

        assert_eq!(idx_sha.to_hex(), checksum);

        Ok(PackIndex{
            fanout,
            offsets,
            shas,
            checksums,
            pack_sha: pack_sha.to_hex()
        })
    }

    ///
    /// Encodes the index into binary format for writing.
    ///
    #[allow(dead_code)]
    pub fn encode(&self) -> IoResult<Vec<u8>> {
        let size = self.shas.len();
        let total_size = (2 * 4) + 256 * 4 + size * 28;
        let mut buf: Vec<u8> = Vec::with_capacity(total_size);

        buf.write_all(&MAGIC[..])?;
        buf.write_u32::<BigEndian>(VERSION)?;

        for f in &self.fanout[..] {
            buf.write_u32::<BigEndian>(*f)?;
        }
        for sha in &self.shas {
            buf.write_all(sha)?;
        }
        for f in &self.checksums {
            buf.write_u32::<BigEndian>(*f)?;
        }
        for f in &self.offsets {
            buf.write_u32::<BigEndian>(*f)?;
        }

        buf.write_all(&self.pack_sha.from_hex().unwrap())?;
        let checksum = store::sha1_hash(&buf[..]);
        buf.write_all(&checksum)?;

        Ok(buf)
    }

    ///
    /// Returns the offset in the packfile for the given SHA, if any.
    ///
    #[allow(dead_code)]
    pub fn find(&self, sha: &[u8]) -> Option<usize> {
        let fan = sha[0] as usize;
        let start = if fan > 0 {
            self.fanout[fan - 1] as usize
        } else {
            0
        };
        let end = self.fanout[fan] as usize;

        self.shas[start..end]
            .binary_search_by(|s| s[..].cmp(sha))
            .map(|i| self.offsets[i + start] as usize)
            .ok()
    }

    ///
    /// Creates an index from a list of objects and their offsets
    /// into the packfile.
    ///
    pub fn from_objects(mut objects: Vec<(usize, u32, GitObject)>, pack_sha: &str) -> Self {
        let size = objects.len();
        let mut fanout = [0u32; 256];
        let mut offsets = vec![0; size];
        let mut shas = vec![[0; 20]; size];
        let mut checksums: Vec<u32> = vec![0; size];

        // Sort the objects by SHA
        objects.sort_by(|&(_, _, ref oa), &(_, _, ref ob)| {
            oa.sha().cmp(&ob.sha())
        });

        for (i, &(offset, crc, ref obj)) in objects.iter().enumerate() {
            let mut sha = [0; 20];
            let vsha = obj.sha().from_hex().unwrap();
            sha.clone_from_slice(&vsha);

            // Checksum should be of packed content in the packfile.
            let fanout_start = sha[0] as usize;
            // By definition of the fanout table we need to increment every entry >= this sha
            for f in fanout.iter_mut().skip(fanout_start) {
                *f += 1;
            }
            shas[i] = sha;
            offsets[i] = offset as u32;
            checksums[i] = crc;
        }
        assert_eq!(size as u32, fanout[255]);
        PackIndex {
            fanout,
            offsets,
            shas,
            checksums,
            pack_sha: pack_sha.to_owned()
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use rustc_serialize::hex::{FromHex,ToHex};
    use std::fs::File;
    use std::io::Read;

    use packfile::PackFile;

    static PACK_FILE: &'static str =
        "tests/data/packs/pack-73e0a23f5ebfc74c7ea1940e2843a408ce1789d0.pack";
    static IDX_FILE: &'static str =
        "tests/data/packs/pack-73e0a23f5ebfc74c7ea1940e2843a408ce1789d0.idx";

    static COMMIT: &'static str = "fb6fb3d9b81142566f4b2466857b0302617768de";

    #[test]
    fn reading_an_index() {
        let mut bytes = Vec::new();
        let mut file = File::open(IDX_FILE).unwrap();
        file.read_to_end(&mut bytes).unwrap();
        PackIndex::parse(&bytes[..]).unwrap();
    }

    #[test]
    fn creating_an_index() {
        // Create an index from the associated packfile
        //
        // The packfile index when encoded should exactly
        // match the one which was read when the Packfile::open
        // call was made.
        let pack = PackFile::open(PACK_FILE).unwrap();
        let index = {
            let mut bytes = Vec::new();
            let mut file = File::open(IDX_FILE).unwrap();
            file.read_to_end(&mut bytes).unwrap();
            PackIndex::parse(&bytes[..]).unwrap()
        };

        let test_shas = pack.index.shas
            .iter()
            .map(|s| s.to_hex())
            .collect::<Vec<_>>();
        let idx_shas = index.shas
            .iter()
            .map(|s| s.to_hex())
            .collect::<Vec<_>>();
        assert_eq!(idx_shas.len(), test_shas.len());
        assert_eq!(idx_shas, test_shas);
        let test_encoded = pack.index.encode().unwrap();
        let idx_encoded = index.encode().unwrap();
        assert_eq!(idx_encoded, test_encoded);
    }

    #[test]
    fn read_and_write_should_be_inverses() {
        let mut bytes = Vec::new();
        let mut file = File::open(IDX_FILE).unwrap();
        file.read_to_end(&mut bytes).unwrap();
        PackIndex::parse(&bytes[..]).unwrap();

        let idx = PackIndex::parse(&bytes[..]).unwrap();
        let encoded = idx.encode().unwrap();
        assert_eq!(&bytes[..], &encoded[..]);
    }

    #[test]
    fn finding_an_offset() {
        let mut bytes = Vec::new();
        let mut file = File::open(IDX_FILE).unwrap();
        file.read_to_end(&mut bytes).unwrap();
        let index = PackIndex::parse(&bytes[..]).unwrap();
        let sha = COMMIT.from_hex().unwrap();
        let bad_sha = "abcdefabcdefabcdefabcdefabcdefabcd".from_hex().unwrap();

        assert_eq!(index.find(&sha[..]), Some(458));
        assert_eq!(index.find(&bad_sha), None);
    }
}

