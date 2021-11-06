
use std::io::Result as IoResult;
use clap::{self, Arg, ArgMatches};

use super::SubCommand;

use remote::GitClient;
use remote::sshclient::GitSSHClient;
use packfile::refs;
use store::Repo;

pub struct Params<'a> {
    host: &'a str,
    repo: &'a str,
    user: &'a str,
}


pub fn spec() -> SubCommand {
    clap::SubCommand::with_name("clone-ssh")
        .about("Clone a remote repository using ssh")
        .arg(Arg::with_name("host")
            .required(true)
        )
        .arg(Arg::with_name("user")
            .required(true)
        )
        .arg(Arg::with_name("repo")
            .required(true)
        )
}

pub fn parse<'a>(matches: &'a ArgMatches) -> Params<'a> {
    let host = matches.value_of("host").unwrap();
    let repo = matches.value_of("repo").unwrap();
    let user = matches.value_of("user").unwrap();
    Params {
        host,
        repo,
        user
    }
}


pub fn execute(params: Params) -> IoResult<()> {
    let dir = params.repo.split('.')
        .next().unwrap();
    let full_repo = [params.user, "/", params.repo].join("");
    let mut client = GitSSHClient::new(params.host, &full_repo);

    println!("Cloning into \"{}\"...", dir);

    let refs = client.discover_refs()?;
    let packfile_data = client.fetch_packfile(&refs)?;

    let repo = Repo::from_packfile(dir, &packfile_data)?;

    refs::create_refs(dir, &refs)?;
    refs::update_head(dir, &refs)?;

    // Checkout head and format refs
    repo.checkout_head()?;
    Ok(())
}
