use nom::IResult;

use std::str;
use std::vec::Vec;

use packfile;

struct Tree {
    sha: String,
    entries: Vec<TreeEntry>
}

struct TreeEntry {
    mode: EntryMode,
    path: String,
    sha: String
}

enum EntryMode {
    Normal,
    Executable,
    Symlink,
    Gitlink,
    SubDirectory
}

impl Tree {
    fn from_packfile_object(raw: packfile::Object) -> Option<Self> {
        if let IResult::Done(_, entries) = parse_tree_entries(&raw.content[..]) {
            Some(Tree {
                entries: entries,
                sha: raw.sha()
            })
        } else {
            None
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
            "040000" => EntryMode::SubDirectory,
            _ => panic!("Unsupported file mode: {}", mode)
        }
    }
}

named!(parse_tree_entry <TreeEntry>,
    chain!(
        mode: map_res!(take_until_and_consume!(" "), str::from_utf8) ~
        path: map_res!(take_until_and_consume!("\0"), str::from_utf8) ~
        sha: map_res!(take!(20), str::from_utf8) ,
        || {
            TreeEntry {
                mode: EntryMode::from_str(mode),
                path: path.to_string(),
                sha: sha.to_string()
            }
        }
    )
);

named!(parse_tree_entries <Vec<TreeEntry> >, many1!(parse_tree_entry));

#[test]
fn test_parse_tree() {
    let input = "100644 .ghci\01234567891234567890a100644 RunMain.hs\01234567891234567890b";
    if let IResult::Done(_, entries) = parse_tree_entries(input.as_bytes()) {
        ()
    } else {
        panic!("Failed to parse tree");
    }
}
