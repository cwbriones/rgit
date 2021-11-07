use std::io::Result as IoResult;

use clap::{self, Arg, ArgMatches};

use crate::delta;
use super::SubCommand;

pub struct Params<'a> {
    source: &'a str,
    delta: &'a str
}

pub fn spec() -> SubCommand {
    clap::SubCommand::with_name("test-delta")
        .about("Reconstruct an object given a source and delta file")
        .arg(Arg::with_name("source")
             .required(true)
        )
        .arg(Arg::with_name("delta")
             .required(true)
        )
}

pub fn parse<'a>(matches: &'a ArgMatches) -> Params<'a> {
    Params {
        source: matches.value_of("source").unwrap(),
        delta: matches.value_of("delta").unwrap(),
    }
}

pub fn execute(params: Params) -> IoResult<()> {
    delta::patch_file(params.source, params.delta)
}
