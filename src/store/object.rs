use std::cell::RefCell;
use std::fs::{
    self,
    File,
};
use std::io::{
    Read,
    Write,
};
use std::path::PathBuf;
use std::str;

use anyhow::anyhow;
use anyhow::Result;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;

use crate::delta;
use crate::store::commit::Commit;
use crate::store::tree::Tree;
use crate::store::Sha;

///
/// A type of loose object found in the database.
///
#[derive(Debug, Copy, Clone)]
pub enum ObjectType {
    Tree,
    Commit,
    Tag,
    Blob,
}

///
/// A parsed Git object found in the database.
///
#[derive(Clone)]
pub struct PackedObject {
    pub obj_type: ObjectType,
    pub content: Vec<u8>,
    sha: RefCell<Option<Sha>>,
}

impl PackedObject {
    pub fn new(obj_type: ObjectType, content: Vec<u8>) -> Self {
        PackedObject {
            obj_type,
            content,
            sha: RefCell::new(None),
        }
    }

    pub fn patch(&self, patch: &[u8]) -> Self {
        PackedObject {
            obj_type: self.obj_type,
            content: delta::patch(&self.content, patch),
            sha: RefCell::new(None),
        }
    }

    ///
    /// Opens the given object from loose form in the repo.
    ///
    pub fn open(repo: &str, sha: &Sha) -> Result<Self> {
        let path = object_path(repo, sha);

        let mut inflated = Vec::new();
        let file = File::open(path)?;
        let mut z = ZlibDecoder::new(file);
        z.read_to_end(&mut inflated)
            .expect("Error inflating object");

        let sha1_checksum = Sha::compute_from_bytes(&inflated);
        assert_eq!(&sha1_checksum, sha);

        let split_idx = inflated.iter().position(|x| *x == 0).unwrap();
        let (obj_type, size) = {
            let header = str::from_utf8(&inflated[..split_idx]).unwrap();
            PackedObject::parse_header(header)?
        };

        let mut footer = Vec::new();
        footer.extend_from_slice(&inflated[split_idx + 1..]);

        assert_eq!(footer.len(), size);

        Ok(PackedObject {
            obj_type,
            content: footer,
            sha: RefCell::new(Some(sha.clone())),
        })
    }

    ///
    /// Encodes the object into packed format, returning the
    /// SHA and encoded representation.
    ///
    pub fn encode(&self) -> (Sha, Vec<u8>) {
        // encoding:
        // header ++ content
        let mut encoded = self.header();
        encoded.extend_from_slice(&self.content);
        (Sha::compute_from_bytes(&encoded[..]), encoded)
    }

    ///
    /// Returns the SHA-1 hash of this object's encoded representation.
    ///
    pub fn sha(&self) -> Sha {
        {
            let mut cache = self.sha.borrow_mut();
            if cache.is_some() {
                return cache.as_ref().unwrap().clone();
            }
            let (hash, _) = self.encode();
            *cache = Some(hash);
        }
        self.sha()
    }

    ///
    /// Encodes this object and writes it to the repo's database.
    ///
    #[allow(unused)]
    pub fn write(&self, repo: &str) -> Result<()> {
        let (sha, blob) = self.encode();
        let path = object_path(repo, &sha);

        fs::create_dir_all(path.parent().unwrap())?;

        let file = File::create(&path)?;
        let mut z = ZlibEncoder::new(file, Compression::Default);
        z.write_all(&blob[..])?;
        Ok(())
    }

    fn parse_header(header: &str) -> Result<(ObjectType, usize)> {
        let split: Vec<&str> = header.split(' ').collect();
        if split.len() != 2 {
            return Err(anyhow!("Bad object header"));
        }
        let (t, s) = (split[0], split[1]);
        let obj_type = match t {
            "commit" => ObjectType::Commit,
            "tree" => ObjectType::Tree,
            "blob" => ObjectType::Blob,
            "tag" => ObjectType::Tag,
            _ => Err(anyhow!("unknown object type: {}", t))?,
        };
        let size = s.parse::<usize>().unwrap();
        Ok((obj_type, size))
    }

    fn header(&self) -> Vec<u8> {
        // header:
        // "type size \0"
        let str_type = match self.obj_type {
            ObjectType::Commit => "commit",
            ObjectType::Tree => "tree",
            ObjectType::Blob => "blob",
            ObjectType::Tag => "tag",
        };
        let str_size = self.content.len().to_string();
        let res: String = [str_type, " ", &str_size[..], "\0"].concat();
        res.into_bytes()
    }

    ///
    /// Parses the internal representation of this object into a Tree.
    /// Returns `None` if the object is not a Tree.
    ///
    pub fn as_tree(&self) -> Option<Tree> {
        if let ObjectType::Tree = self.obj_type {
            Tree::parse(&self.content)
        } else {
            None
        }
    }

    ///
    /// Parses the internal representation of this object into a Commit.
    /// Returns `None` if the object is not a Commit.
    ///
    pub fn as_commit(&self) -> Option<Commit> {
        if let ObjectType::Commit = self.obj_type {
            Commit::from_raw(self)
        } else {
            None
        }
    }
}

fn object_path(repo: &str, sha: &Sha) -> PathBuf {
    let hex_sha = sha.hex();

    let mut path = PathBuf::new();
    path.push(repo);
    path.push(".git");
    path.push("objects");
    path.push(&hex_sha[..2]);
    path.push(&hex_sha[2..40]);
    path
}
