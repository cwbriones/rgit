use std::io::Result as IoResult;
use clap::{self, Arg, ArgMatches};

use super::{validators, SubCommand};

use remote::GitClient;
use remote::httpclient::GitHttpClient;
use packfile::refs::GitRef;

pub struct Params<'a> {
    repo: &'a str,
}

pub fn spec() -> SubCommand {
    clap::SubCommand::with_name("ls-remote")
        .about("List available refs in a remote repository")
        .arg(Arg::with_name("repo")
             .required(true)
             .validator(validators::is_url_or_ssh_repo)
        )
}

pub fn parse<'a>(matches: &'a ArgMatches) -> Params<'a> {
    let repo = matches.value_of("repo").unwrap();
    Params {
        repo: repo
    }
}

///
/// Lists remote refs available in the given repo.
///
pub fn execute(params: Params) -> IoResult<()> {
    let mut client = GitHttpClient::new(params.repo);
    client.discover_refs().map(|pktlines| {
        for p in &pktlines {
            let &GitRef{ref id, ref name} = p;
            print!("{}\t{}\n", id, name);
        }
    })
}

