use flate2::Compression;
use flate2::write::ZlibEncoder;

use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use self::GitObjectType::*;

pub struct GitObject {
    pub obj_type: GitObjectType,
    pub content: Vec<u8>
}

#[derive(Debug)]
pub enum GitObjectType {
    Commit,
    Tree,
    Blob,
    Tag,
    OfsDelta(u8),
    RefDelta([u8; 20]),
}

impl GitObject {
    pub fn encode(&self) -> (String, Vec<u8>) {
        // encoding:
        // header ++ content
        if let Some(mut blob) = self.header() {
            blob.push_all(&self.content[..]);
            (sha1_hash(&blob[..]), blob)
        } else {
            unreachable!("Tried to write an object type that was not Tree, Commit, Blob, or Tag")
        }
    }

    pub fn write_object(&self) {
        let (sha1, blob) = self.encode();
        let path = object_path(&sha1);

        fs::create_dir(path.parent().unwrap());

        let mut file = File::create(&path).unwrap();
        let mut z = ZlibEncoder::new(file, Compression::Default);
        z.write_all(&blob[..]).unwrap();
    }

    fn header(&self) -> Option<Vec<u8>> {
        // header: 
        // "type size \0"
        let num_type = match self.obj_type {
            Commit => 1,
            Tree => 2,
            Blob => 3,
            Tag => 4,
            _ => 0
        };
        match num_type {
            0  => None,
            _ => {
                let s1 = num_type.to_string();
                let s2 = self.content.len().to_string();
                let res: String = [&s1[..], " ", &s2[..], "\0"].concat();
                Some(res.into_bytes())
            }
        }
    }
}

fn object_path(sha: &String) -> PathBuf {
    let mut path = PathBuf::new(&sha[..1]);
    path.push(&sha[1..40]);
    path
}

fn sha1_hash(input: &[u8]) -> String {
    use crypto::digest::Digest;
    use crypto::sha1::Sha1;

    let mut hasher = Sha1::new();
    hasher.input(input);

    hasher.result_str()
}

