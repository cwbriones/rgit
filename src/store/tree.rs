use nom::{IResult,space};

use std::str;
use std::vec::Vec;
use rustc_serialize::hex::ToHex;

use packfile;

pub struct Tree {
    pub sha: String,
    pub entries: Vec<TreeEntry>
}

#[derive(Debug)]
pub struct TreeEntry {
    pub mode: EntryMode,
    pub path: String,
    pub sha: String
}

#[derive(Debug)]
pub enum EntryMode {
    Normal,
    Executable,
    Symlink,
    Gitlink,
    SubDirectory
}

impl Tree {
    pub fn from_packfile_object(raw: packfile::Object) -> Option<Self> {
        match parse_tree_entries(&raw.content[..]) {
            IResult::Done(_, entries) => {
                Some(Tree {
                    entries: entries,
                    sha: raw.sha()
                })
            },
            e @ _ => {
                println!("Error parsing tree: {:?}", e);
                None
            }
        }
    }
}

impl EntryMode {
    fn from_str(mode: &str) -> Self {
        match mode {
            "100644" | "644" => EntryMode::Normal,
            "100755" | "755" => EntryMode::Executable,
            "120000" => EntryMode::Symlink,
            "160000" => EntryMode::Gitlink,
            "40000" => EntryMode::SubDirectory,
            _ => panic!("Unsupported file mode: {}", mode)
        }
    }
}

named!(parse_tree_entry <TreeEntry>,
    chain!(
        mode: map_res!(take_until!(" "), str::from_utf8) ~
        space ~
        path: map_res!(take_until_and_consume!("\0"), str::from_utf8) ~
        sha: take!(20),
        || {
            println!("mode: {:?}", mode);
            TreeEntry {
                mode: EntryMode::from_str(mode),
                path: path.to_string(),
                sha: sha.to_hex()
            }
        }
    )
);

named!(parse_tree_entries <Vec<TreeEntry> >, many1!(parse_tree_entry));

#[test]
fn test_parse_tree() {
    // The raw contents of a tree object.
    let input = [49, 48, 48, 54, 52, 52, 32, 46, 103, 105, 116, 105, 103, 110, 111, 114, 101, 0, 79,
        255, 178, 248, 156, 189, 143, 33, 105, 206, 153, 20, 189, 22, 189, 67, 120, 91, 179, 104, 49,
        48, 48, 54, 52, 52, 32, 67, 97, 114, 103, 111, 46, 116, 111, 109, 108, 0, 226, 11, 220, 57, 
        33, 62, 223, 169, 46, 80, 98, 15, 155, 24, 209, 88, 234, 228, 138, 99, 49, 48, 48, 54, 52, 52,
        32, 82, 69, 65, 68, 77, 69, 46, 109, 100, 0, 189, 6, 31, 50, 207, 237, 81, 181, 168, 222, 145,
        109, 134, 186, 137, 235, 159, 208, 104, 242, 52, 48, 48, 48, 48, 32, 115, 114, 99, 0, 44, 153,
        32, 248, 175, 44, 114, 130, 179, 183, 191, 144, 34, 196, 7, 92, 15, 177, 105, 86];
    if let IResult::Done(_, entries) = parse_tree_entries(&input) {
        ()
    } else {
        panic!("Failed to parse tree");
    }
}
