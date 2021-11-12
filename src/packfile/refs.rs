use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::{
    Path,
    PathBuf,
};

use anyhow::Result;

#[derive(Debug)]
pub struct GitRef {
    pub id: String,
    pub name: String,
}

pub fn create_refs(repo: &str, refs: &[GitRef]) -> Result<()> {
    let (tags, branches): (Vec<_>, Vec<_>) = refs
        .iter()
        .filter(|r| !r.name.ends_with("^{}"))
        .partition(|r| r.name.starts_with("refs/tags"));

    write_refs(repo, "refs/remotes/origin", &branches)?;
    write_refs(repo, "refs/tags", &tags)?;
    Ok(())
}

fn write_refs(repo: &str, parent_path: &str, refs: &[&GitRef]) -> Result<()> {
    let mut path = PathBuf::new();
    path.push(parent_path);

    for r in refs {
        let mut full_path = path.clone();
        let simple_name = Path::new(&r.name).file_name().unwrap();
        full_path.push(&simple_name);
        create_ref(repo, full_path.to_str().unwrap(), &r.id)?;
    }
    Ok(())
}

pub fn update_head(repo: &str, refs: &[GitRef]) -> Result<()> {
    if let Some(head) = refs.iter().find(|r| r.name == "HEAD") {
        let sha1 = &head.id;
        let true_ref = refs.iter().find(|r| r.name != "HEAD" && r.id == *sha1);
        let dir = true_ref.map_or("refs/heads/master", |r| &r.name[..]);
        create_ref(repo, dir, sha1)?;
        create_sym_ref(repo, "HEAD", dir)?;
    }
    Ok(())
}

///
/// Creates a ref in the given repository.
///
fn create_ref(repo: &str, path: &str, id: &str) -> Result<()> {
    let mut full_path = PathBuf::new();
    full_path.push(repo);
    full_path.push(".git");
    full_path.push(path);
    fs::create_dir_all(full_path.parent().unwrap())?;
    let mut file = File::create(full_path)?;
    file.write_fmt(format_args!("{}\n", id))?;
    Ok(())
}

///
/// Creates a symbolic ref in the given repository.
///
fn create_sym_ref(repo: &str, name: &str, the_ref: &str) -> Result<()> {
    let mut path = PathBuf::new();
    path.push(repo);
    path.push(".git");
    path.push(name);
    let mut file = File::create(path)?;
    file.write_fmt(format_args!("ref: {}\n", the_ref))?;
    Ok(())
}
