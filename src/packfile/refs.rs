use std::io::Result as IoResult;
use std::io::{Read,Write};
use std::fs;
use std::fs::File;
use std::path::{Path,PathBuf};

pub struct GitRef {
    pub id: String,
    pub name: String,
}

pub fn create_refs(refs: &Vec<GitRef>) -> IoResult<()> {
    let (tags, branches): (Vec<_>, Vec<_>) = refs.iter()
        .filter(|r| !r.name.ends_with("^{}"))
        .partition(|r| {
            println!("ref {}", r.name);
            r.name.starts_with("refs/tags")
        });

    try!(write_refs("refs/remotes/origin", &branches));
    try!(write_refs("refs/tags", &tags));
    Ok(())
}

fn write_refs(parent_path: &str, refs: &Vec<&GitRef>) -> IoResult<()> {
    let mut path = PathBuf::new();
    path.push(parent_path);
    println!("writing refs to {}", parent_path);

    for r in refs {
        let mut qualified_path = path.clone();
        let simple_name = Path::new(&r.name).file_name().unwrap();
        try!(fs::create_dir_all(&qualified_path));
        qualified_path.push(&simple_name);
        let mut file = try!(File::create(qualified_path));
        try!(file.write_fmt(format_args!("{}\n", r.id)));
    }
    Ok(())
}

pub fn update_head(refs: &Vec<GitRef>) -> IoResult<()> {
    if let Some(head) = refs.iter().find(|r| r.name == "HEAD") {
        let sha1 = &head.id;
        let true_ref = refs.iter().find(|r| r.name != "HEAD" && r.id == *sha1);
        let dir = true_ref
            .map(|r| &r.name[..])
            .unwrap_or("refs/heads/master");
        try!(create_ref(dir, &sha1));
        try!(create_sym_ref("HEAD", dir));
    }
    Ok(())
}

///
/// Creates a ref in the given repository.
///
fn create_ref(dir: &str, r: &str) -> IoResult<()> {
    Ok(())
}

///
/// Creates a symbolic ref in the given repository.
///
fn create_sym_ref(name: &str, the_ref: &str) -> IoResult<()> {
    let mut file = try!(File::create(name));
    try!(file.write_fmt(format_args!("ref: {}\n", the_ref)));
    Ok(())
}
