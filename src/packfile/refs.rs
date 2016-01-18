use std::io::Result as IoResult;
use std::io::Write;
use std::fs;
use std::fs::File;
use std::path::{Path,PathBuf};

#[derive(Debug)]
pub struct GitRef {
    pub id: String,
    pub name: String,
}

pub fn create_refs(repo: &str, refs: &Vec<GitRef>) -> IoResult<()> {
    let (tags, branches): (Vec<_>, Vec<_>) = refs.iter()
        .filter(|r| !r.name.ends_with("^{}"))
        .partition(|r| {
            r.name.starts_with("refs/tags")
        });

    try!(write_refs(repo, "refs/remotes/origin", &branches));
    try!(write_refs(repo, "refs/tags", &tags));
    Ok(())
}

fn write_refs(repo: &str, parent_path: &str, refs: &Vec<&GitRef>) -> IoResult<()> {
    let mut path = PathBuf::new();
    path.push(parent_path);

    for r in refs {
        let mut full_path = path.clone();
        let simple_name = Path::new(&r.name).file_name().unwrap();
        full_path.push(&simple_name);
        try!(create_ref(repo, full_path.to_str().unwrap(), &r.id));
    }
    Ok(())
}

pub fn update_head(repo: &str, refs: &Vec<GitRef>) -> IoResult<()> {
    if let Some(head) = refs.iter().find(|r| r.name == "HEAD") {
        let sha1 = &head.id;
        let true_ref = refs.iter().find(|r| r.name != "HEAD" && r.id == *sha1);
        let dir = true_ref
            .map(|r| &r.name[..])
            .unwrap_or("refs/heads/master");
        try!(create_ref(repo, dir, &sha1));
        try!(create_sym_ref(repo, "HEAD", dir));
    }
    Ok(())
}

///
/// Creates a ref in the given repository.
///
fn create_ref(repo: &str, path: &str, id: &str) -> IoResult<()> {
    let mut full_path = PathBuf::new();
    full_path.push(repo);
    full_path.push(".git");
    full_path.push(path);
    try!(fs::create_dir_all(full_path.parent().unwrap()));
    let mut file = try!(File::create(full_path));
    try!(file.write_fmt(format_args!("{}\n", id)));
    Ok(())
}

///
/// Creates a symbolic ref in the given repository.
///
fn create_sym_ref(repo: &str, name: &str, the_ref: &str) -> IoResult<()> {
    let mut path = PathBuf::new();
    path.push(repo);
    path.push(".git");
    path.push(name);
    let mut file = try!(File::create(path));
    try!(file.write_fmt(format_args!("ref: {}\n", the_ref)));
    Ok(())
}
