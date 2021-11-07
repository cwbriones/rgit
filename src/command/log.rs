use std::io::Result as IoResult;
use clap::{self, Arg, ArgMatches};
use super::SubCommand;

use crate::store::Repo;

pub struct Params<'a> {
    revision: Option<&'a str>,
}

pub fn spec() -> SubCommand {
    clap::SubCommand::with_name("log")
        .about("Show commit logs")
        .arg(Arg::with_name("revision"))
}

pub fn parse<'a>(matches: &'a ArgMatches) -> Params<'a> {
    let revision = matches.value_of("revision");
    Params {
        revision
    }
}

pub fn execute(params: Params) -> IoResult<()> {
    let repo = Repo::from_enclosing()?;
    let rev = params.revision.unwrap_or("HEAD");
    // Refactor this into a commit walker and pass a closure that calls
    // std::process::Command::new("less") to pipe it
    repo.log(rev)?;
    Ok(())
}
