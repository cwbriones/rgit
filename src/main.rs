extern crate getopts;
extern crate flate2;
extern crate crypto;
extern crate rustc_serialize;
extern crate byteorder;
extern crate hyper;
extern crate clap;
extern crate ssh2;

#[macro_use]
extern crate nom;

use clap::{Arg, App, SubCommand};

use remote::operations as remote_ops;

use std::process;

mod remote;
mod packfile;
mod store;
mod delta;

fn main() {
    let app_matches = App::new("rgit")
        .version("0.1.0")
        .about("A Git implementation in Rust.")
        .subcommand(SubCommand::with_name("clone")
            .about("Clone a remote repository")
            .arg(Arg::with_name("repo")
                .required(true)
            )
            .arg(Arg::with_name("dir"))
        )
        .subcommand(SubCommand::with_name("clone-ssh")
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
        )
        .subcommand(SubCommand::with_name("ls-remote")
            .about("List available refs in a remote repository")
            .arg(Arg::with_name("repo")
                 .required(true)
            )
        )
        .subcommand(SubCommand::with_name("ls-remote-ssh")
            .about("List available refs in a remote repository using ssh")
            .arg(Arg::with_name("host")
                 .required(true)
            )
            .arg(Arg::with_name("user")
                 .required(true)
            )
            .arg(Arg::with_name("repo")
                 .required(true)
            )
        )
        .subcommand(SubCommand::with_name("test-delta")
            .about("Reconstruct an object given a base and delta file")
            .arg(Arg::with_name("base")
                 .required(true)
            )
            .arg(Arg::with_name("delta")
                 .required(true)
            )
        )
        .get_matches();

    let result = match app_matches.subcommand_name() {
        Some(s @ "clone") => {
            let matches = app_matches.subcommand_matches(s).unwrap();
            let repo = matches.value_of("repo").unwrap();
            let dir  = matches.value_of("dir").map(|s| s.to_string());
            remote_ops::clone_priv(repo, dir)
        },
        Some(s @ "clone-ssh") => {
            let matches = app_matches.subcommand_matches(s).unwrap();
            let host = matches.value_of("host").unwrap();
            let user = matches.value_of("user").unwrap();
            let repo = matches.value_of("repo").unwrap();
            remote_ops::clone_ssh_priv(host, user, repo)
        },
        Some(s @ "ls-remote") => {
            let matches = app_matches.subcommand_matches(s).unwrap();
            let repo = matches.value_of("repo").unwrap();
            remote_ops::ls_remote(repo)
        },
        Some(s @ "ls-remote-ssh") => {
            let matches = app_matches.subcommand_matches(s).unwrap();
            let host = matches.value_of("host").unwrap();
            let user = matches.value_of("user").unwrap();
            let repo = matches.value_of("repo").unwrap();
            remote_ops::ls_remote_ssh(host, user, repo)
        },
        Some(s @ "test-delta") => {
            let matches = app_matches.subcommand_matches(s).unwrap();
            let source = matches.value_of("source").unwrap();
            let delta  = matches.value_of("delta").unwrap();
            delta::patch_file(source, delta)
        },
        Some(_) => unreachable!(),
        None    => {
            println!("{}", app_matches.usage());
            Ok(())
        }
    };
    if let Err(e) = result {
        println!("Error: {}", e);
        process::exit(-1)
    }
}
