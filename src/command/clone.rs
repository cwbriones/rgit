use std::io::Result as IoResult;
use std::path::Path;
use clap::{self, Arg, ArgMatches};

use super::{validators, SubCommand};

use remote::GitClient;
use remote::httpclient::GitHttpClient;
use remote::tcpclient::GitTcpClient;
use packfile::refs;
use store::Repo;
use reqwest::Url;

pub struct Params {
    repo: String,
    dir: Option<String>,
}

pub fn spec() -> SubCommand {
    clap::SubCommand::with_name("clone")
        .about("Clone a remote repository")
        .arg(Arg::with_name("repo")
            .required(true)
            .validator(validators::is_url_or_ssh_repo)
        )
        .arg(Arg::with_name("dir"))
}

pub fn parse(matches: &ArgMatches) -> Params {
    let repo = matches.value_of("repo").unwrap();
    let dir  = matches.value_of("dir").map(|s| s.to_owned());
    Params {
        repo: repo.to_owned(),
        dir: dir
    }
}

pub fn execute(params: Params) -> IoResult<()> {
    let (mut client, dir): (Box<GitClient>, _) = match params.repo.parse::<Url>() {
        Ok(uri) => {
            // TODO: There has to be a better way to do this.
            let dir = params.dir.unwrap_or_else(|| {
                Path::new(uri.path())
                    .components()
                    .last()
                    .unwrap() // path is weird
                    .as_os_str()
                    .to_owned()
                    .into_string()
                    .unwrap() // path should be unicode
                    .split('.')
                    .next()
                    .unwrap() // path doesn't end in .git
                    .to_owned()
            });
            let mut repo = params.repo.to_owned();
            if !repo.ends_with(".git") {
                repo.push_str(".git");
            }
            if !repo.ends_with("/") {
                repo.push_str("/");
            }
            (Box::new(GitHttpClient::new(&repo)), dir)
        },
        Err(_) => {
            let client = try!(GitTcpClient::connect(&params.repo, "127.0.0.1", 9418));
            let dir = params.dir.unwrap_or(params.repo);
            (Box::new(client), dir)
        }
    };
    println!("Cloning into \"{}\"...", dir);

    let refs = try!(client.discover_refs());
    let packfile_data = try!(client.fetch_packfile(&refs));

    let repo = try!(Repo::from_packfile(&dir, &packfile_data));

    try!(refs::create_refs(&dir, &refs));
    try!(refs::update_head(&dir, &refs));

    // Checkout head and format refs
    try!(repo.checkout_head());
    Ok(())
}

//pub fn clone_ssh_priv(host: &str, user: &str, repo: &str) -> IoResult<()> {
//    let dir = repo.split(".")
//        .next().unwrap()
//        .to_owned();
//    let full_repo = [user, "/", repo].join("");
//    let mut client = GitSSHClient::new(host, &full_repo);
//
//    println!("Cloning into \"{}\"...", dir);
//
//    let refs = try!(client.discover_refs());
//    let packfile_data = try!(client.fetch_packfile(&refs));
//
//    let repo = try!(Repo::from_packfile(&dir, &packfile_data));
//
//    try!(refs::create_refs(&dir, &refs));
//    try!(refs::update_head(&dir, &refs));
//
//    // Checkout head and format refs
//    try!(repo.checkout_head());
//    Ok(())
//}

#[cfg(test)]
mod tests {
    use std::fs;
    use super::*;
    use std::error::Error;
    use std::io;

    #[test]
    fn test_clone() {
        let dir = "tests/clone-test".to_owned();
        if let Err(error) = fs::remove_dir_all(&dir) {
            if error.kind() != io::ErrorKind::NotFound {
                panic!("Error removing test-clone directory: {}", error.description());
            }
        }
        let params = Params {
            repo: "https://github.com/cwbriones/rgit.git".to_owned(),
            dir: Some(dir),
        };
        execute(params).unwrap()
    }
}

