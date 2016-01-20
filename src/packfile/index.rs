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
use crc::crc32;
use byteorder::{ReadBytesExt,WriteBytesExt,BigEndian};
use rustc_serialize::hex::{FromHex,ToHex};

use std::io;
use std::io::{Write,Read};
use std::io::Result as IoResult;
use std::fs::File;
use std::path::{Path,PathBuf};

use store;
use packfile::PackFile;
use packfile::object::Object;

type SHA = [u8; 20];

static MAGIC: [u8; 4] = [255, 116, 79, 99];
static VERSION: u32 = 2;

///
/// Version 2 of the Git Packfile Index containing separate
/// tables for the offsets, fanouts, and shas.
///
pub struct PackIndex {
    fanout: [u32; 256],
    offsets: Vec<u32>,
    shas: Vec<SHA>,
    checksums: Vec<u32>,
    pack_sha: String
}

impl PackIndex {
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let mut file = try!(File::open(path));
        let mut contents = Vec::new();
        try!(file.read_to_end(&mut contents));
        Self::parse(&contents)
    }

    pub fn parse(mut content: &[u8]) -> io::Result<Self> {
        let checksum = store::sha1_hash_hex(&content[..content.len() - 20]);

        // Parse header
        let mut magic = [0; 4];
        try!(content.read_exact(&mut magic));
        assert_eq!(magic, MAGIC);

        let version = try!(content.read_u32::<BigEndian>());
        assert_eq!(version, VERSION);

        // Parse Fanout table
        let mut fanout = [0; 256];
        for i in 0..256 {
            fanout[i] = try!(content.read_u32::<BigEndian>());
        }
        let size = fanout[255] as usize;

        // Parse N Shas
        let mut shas = Vec::with_capacity(size);
        for _ in 0..size {
            let mut sha = [0; 20];
            try!(content.read_exact(&mut sha));
            shas.push(sha);
        }

        // Parse N Checksums
        let mut checksums = Vec::with_capacity(size);
        for _ in 0..size {
            let crc = try!(content.read_u32::<BigEndian>());
            checksums.push(crc);
        }

        // Parse N Offsets
        let mut offsets = Vec::with_capacity(size);
        for _ in 0..size {
            let off = try!(content.read_u32::<BigEndian>());
            offsets.push(off);
        }

        // Parse trailer
        let mut pack_sha = [0; 20];
        try!(content.read_exact(&mut pack_sha));

        let mut idx_sha = [0; 20];
        try!(content.read_exact(&mut idx_sha));

        {
            println!("Checking shas");
            let shas_hex = shas.iter().map(|s| s.to_hex()).collect::<Vec<_>>().join("");
            //let check = store::sha1_hash_iter(shas_hex.iter().map(|s| s.as_bytes()));
            let from = shas_hex.from_hex().unwrap();
            let check = store::sha1_hash_hex(&from[..]);
            println!("got  {}", check);
            println!("pack {}", pack_sha.to_hex());
        }

        assert_eq!(idx_sha.to_hex(), checksum);

        Ok(PackIndex{
            fanout: fanout,
            offsets: offsets,
            shas: shas,
            checksums: checksums,
            pack_sha: pack_sha.to_hex()
        })
    }

    ///
    /// Encodes the index into binary format for writing.
    ///
    pub fn encode(&self) -> IoResult<Vec<u8>> {
        let size = self.shas.len();
        let total_size = (2 * 4) + 256 * 4 + size * 28;
        let mut buf: Vec<u8> = Vec::with_capacity(total_size);

        try!(buf.write_all(&MAGIC[..]));
        try!(buf.write_u32::<BigEndian>(VERSION));

        for f in &self.fanout[..] {
            try!(buf.write_u32::<BigEndian>(*f));
        }
        for sha in &self.shas {
            try!(buf.write_all(sha));
        }
        for f in &self.checksums {
            try!(buf.write_u32::<BigEndian>(*f));
        }
        for f in &self.offsets {
            try!(buf.write_u32::<BigEndian>(*f));
        }

        try!(buf.write_all(&self.pack_sha.from_hex().unwrap()));
        let checksum = store::sha1_hash(&buf[..]);
        try!(buf.write_all(&checksum));

        Ok(buf)
    }

    ///
    /// Creates an index from a list of objects and their offsets
    /// into the packfile.
    ///
    pub fn from_packfile(pack: &PackFile) -> Self {
        let mut objects = pack.objects().collect::<Vec<_>>();
        let size = objects.len();
        let mut fanout = [0u32; 256];
        let mut offsets = vec![0; size];
        let mut shas = vec![[0; 20]; size];
        let mut checksums: Vec<u32> = vec![0; size];

        // Sort the objects by SHA
        objects.sort_by(|&(_, ref oa), &(_, ref ob)| {
            oa.sha().cmp(&ob.sha())
        });

        for (i, &(offset, ref obj)) in objects.iter().enumerate() {
            let mut sha = [0; 20];
            let vsha = &obj.sha().from_hex().unwrap();
            // TODO: This should be a single copy instead of a loop but
            // there is currently no stable function for this.
            for (i, b) in vsha.iter().enumerate() {
                sha[i] = *b;
            }

            let checksum = crc32::checksum_ieee(&obj.content);
            let fanout_start = sha[0] as usize;
            // By definition of the fanout table we need to increment every entry >= this sha
            for j in fanout_start..256 {
                fanout[j] += 1;
            }
            shas[i] = sha;
            offsets[i] = offset as u32;
            checksums[i] = checksum;
        }
        assert_eq!(size as u32, fanout[255]);
        PackIndex {
            fanout: fanout,
            offsets: offsets,
            shas: shas,
            checksums: checksums,
            pack_sha: pack.sha().to_string()
        }
    }

    ///
    /// Returns the offset in the packfile for the given SHA, if any.
    ///
    pub fn find(&self, sha: &[u8]) -> Option<usize> {
        let fan = sha[0] as usize;
        let start = if fan > 0 {
            self.fanout[fan - 1] as usize
        } else {
            0
        };
        let end = self.fanout[fan] as usize;

        self.shas[start..end].binary_search_by(|ref s| {
            s[..].cmp(sha)
        }).and_then(|i| Ok(self.offsets[i + start] as usize)).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_serialize::hex::FromHex;

    static IDX_FILE: &'static [u8] =
        include_bytes!(
            "../../tests/data/packs/pack-73e0a23f5ebfc74c7ea1940e2843a408ce1789d0.idx"
        );

    static COMMIT: &'static str = "fb6fb3d9b81142566f4b2466857b0302617768de";

    #[test]
    fn reading_an_index() {
        let bytes = IDX_FILE;
        PackIndex::parse(&bytes[..]).unwrap();
    }

    #[test]
    fn read_and_write_should_be_inverses() {
        let bytes = IDX_FILE;

        let idx = PackIndex::parse(&bytes[..]).unwrap();
        let encoded = idx.encode().unwrap();
        assert_eq!(&bytes[..], &encoded[..]);
    }

    #[test]
    fn finding_an_offset() {
        let index = PackIndex::parse(IDX_FILE).unwrap();
        let SHA = COMMIT.from_hex().unwrap();
        let bad_sha = "abcdefabcdefabcdefabcdefabcdefabcd".from_hex().unwrap();

        assert_eq!(index.find(&SHA[..]), Some(458));
        assert_eq!(index.find(&bad_sha), None);
    }
}

