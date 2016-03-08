use std::io::Result as IoResult;
use clap::{self, Arg, ArgMatches};

use super::SubCommand;

use remote::GitClient;
use remote::sshclient::GitSSHClient;
use packfile::refs::GitRef;

pub struct Params<'a> {
    host: &'a str,
    repo: &'a str,
    user: &'a str,
}

pub fn spec() -> SubCommand {
    clap::SubCommand::with_name("ls-remote-ssh")
        .about("List available refs in a remote repository via SSH")
        .arg(Arg::with_name("host")
             .required(true)
        )
        .arg(Arg::with_name("repo")
             .required(true)
        )
        .arg(Arg::with_name("user")
             .required(true)
        )
}

pub fn parse<'a>(matches: &'a ArgMatches) -> Params<'a> {
    let host = matches.value_of("host").unwrap();
    let repo = matches.value_of("repo").unwrap();
    let user = matches.value_of("user").unwrap();
    Params {
        host: host,
        repo: repo,
        user: user
    }
}

///
/// Lists remote refs available in the given repo.
///
pub fn execute(params: Params) -> IoResult<()> {
    let full_repo = [params.user, "/", params.repo].join("");
    let mut client = GitSSHClient::new(params.host, &full_repo);
    client.discover_refs().map(|pktlines| {
        for p in &pktlines {
            let &GitRef{ref id, ref name} = p;
            print!("{}\t{}\n", id, name);
        }
    })
}
