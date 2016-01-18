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
pub fn checkout_head(repo: &str) -> IoResult<()> {
    println!("Checking out head");
    let tip = try!(read_sym_ref(repo, "HEAD"));
    println!("Head is {}", tip);
    walk(repo, &tip).and_then(|t| walk_tree(repo, repo, &t).ok());
    // Need to write the index
    Ok(())
}

fn walk(repo: &str, sha: &str) -> Option<Tree> {
    println!("walking object {}", sha);
    Object::read_from_disk(repo, sha).ok().and_then(|object| {
        match object.obj_type {
            ObjectType::Commit => {
                println!("object is commit, extracting tree");
                Commit::from_packfile_object(object).and_then(|c| extract_tree(repo, &c))
            },
            ObjectType::Tree => {
                println!("object is tree, parsing");
                Tree::from_packfile_object(object)
            },
            _ => None
        }
    })
}

fn walk_tree(repo: &str, parent: &str, tree: &Tree) -> IoResult<()> {
    println!("walking tree at {}", parent);
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
                walk(repo, sha).and_then(|t| {
                    walk_tree(repo, path_str, &t).ok()
                });
            },
            EntryMode::Normal => {
                let object = try!(Object::read_from_disk(repo, &sha));
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

fn extract_tree(repo: &str, commit: &Commit) -> Option<Tree> {
    let sha = &commit.tree;
    println!("Followed commit to tree {}", sha);
    read_tree(repo, sha)
}

fn read_tree(repo: &str, sha: &str) -> Option<Tree> {
    use std::str;
    Object::read_from_disk(repo, sha).ok().and_then(|object| {
        Tree::from_packfile_object(object)
    })
}

///
/// Reads the symbolic ref and resolve it to the actual SHA it points to, if any.
///
pub fn read_sym_ref(repo: &str, name: &str) -> IoResult<String> {
    // Read the symbolic ref directly and parse the actual ref out
    let mut root = path::PathBuf::new();
    root.push(repo);
    root.push(".git");

    let mut sym_path = root.clone();
    sym_path.push(name);

    let mut contents = String::new();
    let mut file = try!(File::open(sym_path));
    try!(file.read_to_string(&mut contents));
    let the_ref = contents.split("ref: ").skip(1).next().unwrap().trim();

    // Read the SHA out of the actual ref
    let mut ref_path = root.clone();
    ref_path.push(the_ref);

    let mut ref_file = try!(File::open(ref_path));
    let mut sha = String::new();
    try!(ref_file.read_to_string(&mut sha));
    Ok(sha.trim().to_string())
}

fn resolve_tree(sha: &str) -> IoResult<()> {
    Ok(())
}
