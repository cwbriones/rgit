use std::ffi::OsStr;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::Result;

#[derive(Debug)]
pub struct GitRef {
    pub id: String,
    pub name: String,
}

pub fn create_refs<P: AsRef<Path>>(gitdir: P, refs: &[GitRef]) -> Result<()> {
    let (tags, branches): (Vec<_>, Vec<_>) = refs
        .iter()
        .filter(|r| !r.name.ends_with("^{}"))
        .partition(|r| r.name.starts_with("refs/tags"));

    let gitdir = gitdir.as_ref();
    write_refs(gitdir.join("refs/remotes/origin"), &branches)?;
    write_refs(gitdir.join("refs/tags"), &tags)?;
    Ok(())
}

fn write_refs<P1: AsRef<Path>>(dir: P1, refs: &[&GitRef]) -> Result<()> {
    for r in refs {
        let simple_name = Path::new(&r.name)
            .file_name()
            .expect("constructed path should have a filename");
        create_ref(dir.as_ref(), simple_name, &r.id)?;
    }
    Ok(())
}

pub fn update_head<P: AsRef<Path>>(gitdir: P, refs: &[GitRef]) -> Result<()> {
    if let Some(head) = refs.iter().find(|r| r.name == "HEAD") {
        let sha1 = &head.id;
        let true_ref = refs.iter().find(|r| r.name != "HEAD" && r.id == *sha1);
        let relpath = true_ref.map_or("refs/heads/master", |r| &r.name[..]);

        let path = gitdir.as_ref().join(relpath);
        let (dir, name) = split_path(&path).unwrap();
        create_ref(dir, name, sha1)?;
        create_sym_ref(gitdir, "HEAD", relpath)?;
    }
    Ok(())
}

///
/// Creates a ref in the given repository.
///
fn create_ref<P1: AsRef<Path>, P2: AsRef<Path>>(dir: P1, name: P2, id: &str) -> Result<()> {
    let dir = dir.as_ref();
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }
    let mut full_path = dir.to_owned();
    full_path.push(name);
    let mut file = File::create(full_path)?;
    file.write_fmt(format_args!("{}\n", id))?;
    Ok(())
}

///
/// Creates a symbolic ref in the given repository.
///
fn create_sym_ref<P: AsRef<Path>>(gitdir: P, name: &str, the_ref: &str) -> Result<()> {
    let path = gitdir.as_ref().join(name);
    let mut file = File::create(path)?;
    file.write_fmt(format_args!("ref: {}\n", the_ref))?;
    Ok(())
}

fn split_path<P: AsRef<Path>>(path: &P) -> Option<(&Path, &OsStr)> {
    let path = path.as_ref();
    path.file_name()
        .and_then(|fname| path.parent().map(|p| (p, fname)))
}
