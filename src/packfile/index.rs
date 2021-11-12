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
use std::fs::File;
use std::io::{
    Read,
    Write,
};
use std::path::Path;

use anyhow::Result;
use byteorder::{
    BigEndian,
    ReadBytesExt,
    WriteBytesExt,
};

use crate::store::PackedObject;
use crate::store::Sha;

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
    pack_sha: Sha,
}

impl PackIndex {
    #[allow(unused)]
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Option<Self>> {
        use std::io::Error as IoError;
        use std::io::ErrorKind;

        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(io_error @ IoError { .. }) if ErrorKind::NotFound == io_error.kind() => {
                return Ok(None)
            }
            Err(io) => return Err(io.into()),
        };
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        Self::parse(&contents).map(Some)
    }

    #[allow(unused)]
    fn parse(mut content: &[u8]) -> Result<Self> {
        let checksum = Sha::compute_from_bytes(&content[..content.len() - 20]);

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
            shas.push(Sha::from_bytes(&sha[..])?);
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
        let mut pack_sha_content = [0; 20];
        content.read_exact(&mut pack_sha_content)?;
        let pack_sha = Sha::from_bytes(&pack_sha_content[..])?;

        let mut idx_sha_content = [0; 20];
        content.read_exact(&mut idx_sha_content)?;
        let idx_sha = Sha::from_bytes(&idx_sha_content[..])?;

        assert_eq!(idx_sha, checksum);

        Ok(PackIndex {
            fanout,
            offsets,
            shas,
            checksums,
            pack_sha,
        })
    }

    ///
    /// Encodes the index into binary format for writing.
    ///
    #[allow(dead_code)]
    pub fn encode(&self) -> Result<Vec<u8>> {
        let size = self.shas.len();
        let total_size = (2 * 4) + 256 * 4 + size * 28;
        let mut buf: Vec<u8> = Vec::with_capacity(total_size);

        buf.write_all(&MAGIC[..])?;
        buf.write_u32::<BigEndian>(VERSION)?;

        for f in &self.fanout[..] {
            buf.write_u32::<BigEndian>(*f)?;
        }
        for sha in &self.shas {
            buf.write_all(sha.as_bytes())?;
        }
        for f in &self.checksums {
            buf.write_u32::<BigEndian>(*f)?;
        }
        for f in &self.offsets {
            buf.write_u32::<BigEndian>(*f)?;
        }

        buf.write_all(self.pack_sha.as_bytes())?;
        let checksum = Sha::compute_from_bytes(&buf[..]);
        buf.write_all(checksum.as_bytes())?;

        Ok(buf)
    }

    ///
    /// Returns the offset in the packfile for the given SHA, if any.
    ///
    #[allow(dead_code)]
    pub fn find(&self, sha: &Sha) -> Option<usize> {
        let fan = sha.as_bytes()[0] as usize;
        let start = if fan > 0 {
            self.fanout[fan - 1] as usize
        } else {
            0
        };
        let end = self.fanout[fan] as usize;

        self.shas[start..end]
            .binary_search_by(|s| s.cmp(sha))
            .map(|i| self.offsets[i + start] as usize)
            .ok()
    }

    ///
    /// Creates an index from a list of objects and their offsets
    /// into the packfile.
    ///
    pub fn from_objects(mut objects: Vec<(usize, u32, PackedObject)>, pack_sha: &Sha) -> Self {
        let size = objects.len();
        let mut fanout = [0u32; 256];
        let mut offsets = Vec::with_capacity(size);
        let mut shas = Vec::with_capacity(size);
        let mut checksums: Vec<u32> = Vec::with_capacity(size);

        // Sort the objects by SHA
        objects.sort_by(|&(_, _, ref oa), &(_, _, ref ob)| oa.sha().cmp(&ob.sha()));

        for &(offset, crc, ref obj) in objects.iter() {
            let sha = obj.sha();

            // Checksum should be of packed content in the packfile.
            let fanout_start = sha.as_bytes()[0] as usize;
            // By definition of the fanout table we need to increment every entry >= this sha
            for f in fanout.iter_mut().skip(fanout_start) {
                *f += 1;
            }
            shas.push(sha);
            offsets.push(offset as u32);
            checksums.push(crc);
        }
        assert_eq!(size as u32, fanout[255]);
        PackIndex {
            fanout,
            offsets,
            shas,
            checksums,
            pack_sha: pack_sha.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Read;

    use super::*;
    use crate::packfile::PackFile;

    static PACK_FILE: &'static str =
        "tests/data/packs/pack-73e0a23f5ebfc74c7ea1940e2843a408ce1789d0.pack";
    static IDX_FILE: &'static str =
        "tests/data/packs/pack-73e0a23f5ebfc74c7ea1940e2843a408ce1789d0.idx";

    static COMMIT: &'static [u8] = b"fb6fb3d9b81142566f4b2466857b0302617768de";

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

        // FIXME: Weird, unnecessary iter/collect
        let test_shas = pack.index.shas.iter().collect::<Vec<_>>();
        let idx_shas = index.shas.iter().collect::<Vec<_>>();
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

        let sha = Sha::from_hex(COMMIT).unwrap();
        let bad_sha = Sha::from_hex(b"abcdefabcdefabcdefabcdefabcdefabcdabcdef").unwrap();

        assert_eq!(index.find(&sha), Some(458));
        assert_eq!(index.find(&bad_sha), None);
    }
}
