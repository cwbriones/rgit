use std::io::Result as IoResult;
use std::io::{Read,Write};
use std::fs;
use std::fs::File;
use std::path::PathBuf;

pub struct GitRef {
    id: String,
    name: String
}

fn create_refs(refs: Vec<GitRef>) {
    let filtered = refs.iter().filter(|r| r.name.ends_with("^{}"));
    let (tags, branches): (Vec<_>, Vec<_>) = refs.iter().partition(|r| {
        r.name.starts_with("refs/tags")
    });

    write_refs("refs/remotes/origin", &branches);
    write_refs("refs/tags", &tags);
}

fn write_refs(path: &str, refs: &Vec<&GitRef>) -> IoResult<()> {
    let mut p = PathBuf::new();
    p.push(".git");
    p.push(path);

    try!(fs::create_dir_all(&p));

    for r in refs {
        let mut qualified_path = p.clone();
        qualified_path.push(&r.name);
        let mut file = File::create(qualified_path);
    }
    Ok(())
}

fn update_head(refs: &Vec<&GitRef>) {
    if let Some(head) = refs.iter().find(|r| r.name == "HEAD") {
        let sha1 = &head.id;
        let true_ref = refs.iter().find(|r| r.name != "HEAD" && r.id == *sha1);
        let dir = true_ref
            .map(|r| &r.name[..])
            .unwrap_or("refs/heads/master");
        create_ref(dir, &sha1);
        create_sym_ref("HEAD", dir); 
    }
}

fn create_ref(name: &str, the_ref: &str) -> IoResult<()> {
    Ok(())
}

/// Creates a symbolic ref in the given repository.
fn create_sym_ref(name: &str, the_ref: &str) -> IoResult<()> {
    Ok(())
}
