mod commit;
mod tree;

use std::fs::File;
use std::io::{Read,Write};
use std::io::Result as IoResult;
use std::path;
use std::fs;
use std::env;

use packfile::object::{Object,ObjectType};
use self::tree::{Tree,TreeEntry,EntryMode};
use self::commit::Commit;

///
/// Resolves the head SHA and attempts to create the file structure
/// of the repository.
///
pub fn checkout_head() -> IoResult<()> {
    println!("checking out head");
    let tip = try!(read_head());
    println!("head is {}", tip);
    walk(&tip).and_then(|t| walk_tree("", &t).ok());
    // Need to write the index
    Ok(())
}

fn walk(sha: &str) -> Option<Tree> {
    println!("walking object {}", sha);
    Object::read_from_disk(sha).ok().and_then(|object| {
        match object.obj_type {
            ObjectType::Commit => {
                println!("object is commit, extracting tree");
                Commit::from_packfile_object(object).and_then(|c| extract_tree(&c))
            },
            ObjectType::Tree => {
                println!("object is tree, parsing");
                Tree::from_packfile_object(object)
            },
            _ => None
        }
    })
}

fn walk_tree(parent: &str, tree: &Tree) -> IoResult<()> {
    for entry in &tree.entries {
        let &TreeEntry {
            ref path, 
            ref mode, 
            ref sha
        } = entry;
        let mut full_path = path::PathBuf::new();
        full_path.push(parent);
        full_path.push(path);
        match *mode {
            EntryMode::SubDirectory => {
                try!(fs::create_dir_all(&full_path));
                let path_str = full_path.to_str().unwrap();
                println!("Tree entry: {}", path_str);
                walk(path_str).and_then(|t| {
                    walk_tree(path_str, &t).ok()
                });
            },
            EntryMode::Normal => {
                let object = try!(Object::read_from_disk(&sha));
                println!("Tree entry: {:?}", full_path);
                let mut file = try!(File::create(full_path));
                try!(file.write_all(&object.content[..]));
                // FIXME: Need to properly set the file mode here.
            },
            _ => panic!("Unsupported Entry Mode")
        }
    }
    Ok(())
}

fn extract_tree(commit: &Commit) -> Option<Tree> {
    let sha = &commit.tree;
    println!("Followed commit to tree {}", sha);
    read_tree(sha)
}

fn read_tree(sha: &str) -> Option<Tree> {
    use std::str;
    Object::read_from_disk(sha).ok().and_then(|object| {
        Tree::from_packfile_object(object)
    })
}

///
/// Reads the symbolic ref "HEAD" and resolves it.
///
pub fn read_head() -> IoResult<String> {
    read_sym_ref("HEAD")
}

///
/// Reads the symbolic ref and resolve it to the actual SHA it points to, if any.
///
pub fn read_sym_ref(name: &str) -> IoResult<String> {
    // Read the symbolic ref directly and parse the actual ref out
    let mut contents = String::new();
    let mut file = try!(File::open(name));
    try!(file.read_to_string(&mut contents));
    let the_ref = contents.split("ref: ").skip(1).next().unwrap().trim();
    println!("sym ref {} contains {}", name, the_ref);

    // Read the SHA out of the actual ref
    let mut ref_file = try!(File::open(the_ref));
    let mut sha = String::new();
    try!(ref_file.read_to_string(&mut sha));
    println!("sym ref {} resolved to {}", name, sha);
    Ok(sha.trim().to_string())
}

fn resolve_tree(sha: &str) -> IoResult<()> {
    Ok(())
}
