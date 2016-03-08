use std::io::Result as IoResult;
use clap::{self, Arg, ArgMatches};

use super::{validators, SubCommand};

use remote::GitClient;
use remote::httpclient::GitHttpClient;
use remote::tcpclient::GitTcpClient;
use packfile::refs;
use store::Repo;
use hyper::Url;

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
    let (mut client, dir): (Box<GitClient>, _) = match Url::parse(&params.repo) {
        Ok(url) => {
            // TODO: There has to be a better way to do this.
            let dir = params.dir.unwrap_or_else(|| {
                url.path().unwrap()
                    .last().unwrap()
                    .split('.')
                    .next().unwrap()
                    .to_owned()
            });
            (Box::new(GitHttpClient::new(&params.repo)), dir)
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
