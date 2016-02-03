// TODO: Move this module into store and simply by refactoring
// store::commit and store::tree

use flate2::Compression;
use flate2::write::ZlibEncoder;
use flate2::read::ZlibDecoder;

use std::fs;
use std::fs::File;
use std::io::{Read,Write};
use std::io::Result as IoResult;
use std::path::PathBuf;
use std::str;

use store;
use self::ObjectType::*;

// TODO: We can simplify this by moving the content into the object type
// as a single field.
#[derive(Clone,PartialEq,Debug)]
pub struct Object {
    pub obj_type: ObjectType,
    pub content: Vec<u8>
}

#[derive(Copy,Clone,PartialEq,Debug)]
pub enum ObjectType {
    Commit,
    Tree,
    Blob,
    Tag,
    // TODO: Remove these from the object type since they are
    // really only relevant when stored and not when in memory.
    OfsDelta(u8),
    RefDelta([u8; 20]),
}

impl Object {
    #[allow(dead_code)]
    pub fn read_from_disk(repo: &str, sha1: &str) -> IoResult<Self> {
        let path = object_path(repo, sha1);

        let mut inflated = Vec::new();
        let file = try!(File::open(path));
        let mut z = ZlibDecoder::new(file);
        z.read_to_end(&mut inflated).expect("Error inflating object");

        let sha1_checksum = store::sha1_hash_hex(&inflated);
        assert_eq!(sha1_checksum, sha1);

        let split_idx = inflated.iter().position(|x| *x == 0).unwrap();
        let (obj_type, size) = {
            let header = str::from_utf8(&inflated[..split_idx]).unwrap();
            Object::parse_header(header)
        };

        let mut footer = Vec::new();
        footer.extend(inflated.into_iter().skip(split_idx+1));

        assert_eq!(footer.len(), size);

        Ok(Object {
            obj_type: obj_type,
            content: footer
        })
    }

    pub fn encode(&self) -> (String, Vec<u8>) {
        // encoding:
        // header ++ content
        if let Some(mut blob) = self.header() {
            // TODO: Update since this was because I couldn't use 
            // Vec::extend or push_all
            for c in &self.content {
                blob.push(*c)
            }
            (store::sha1_hash_hex(&blob[..]), blob)
        } else {
            unreachable!("Tried to write an object type that was not Tree, Commit, Blob, or Tag")
        }
    }

    pub fn sha(&self) -> String {
        let (hash, _) = self.encode();
        hash
    }

    pub fn write(&self, repo: &str) -> IoResult<()> {
        let (sha1, blob) = self.encode();
        let path = object_path(repo, &sha1);

        try!(fs::create_dir_all(path.parent().unwrap()));

        let file = try!(File::create(&path));
        let mut z = ZlibEncoder::new(file, Compression::Default);
        try!(z.write_all(&blob[..]));
        Ok(())
    }

    fn parse_header(header: &str) -> (ObjectType, usize) {
        let split: Vec<&str> = header.split(' ').collect();
        if split.len() == 2 {
            let (t, s) = (split[0], split[1]);
            let obj_type = match t {
                "commit" => Commit,
                "tree" => Tree,
                "blob" => Blob,
                "tag" => Tag,
                _ => panic!("Unknown object type")
            };
            let size = s.parse::<usize>().unwrap();

            (obj_type, size)
        } else {
            panic!("Unknown object type")
        }
    }

    fn header(&self) -> Option<Vec<u8>> {
        // header:
        // "type size \0"
        let str_type = match self.obj_type {
            Commit => "commit",
            Tree => "tree",
            Blob => "blob",
            Tag => "tag",
            _ => ""
        };
        match str_type {
            ""  => None,
            _ => {
                let str_size = self.content.len().to_string();
                let res: String = [str_type, " ", &str_size[..], "\0"].concat();
                Some(res.into_bytes())
            }
        }
    }
}

fn object_path(repo: &str, sha: &str) -> PathBuf {
    let mut path = PathBuf::new();
    path.push(repo);
    path.push(".git");
    path.push("objects");
    path.push(&sha[..2]);
    path.push(&sha[2..40]);
    path
}

