extern crate getopts;
extern crate flate2;
extern crate crypto;
extern crate rustc_serialize;
extern crate byteorder;
extern crate hyper;
extern crate clap;
extern crate ssh2;
extern crate crc;
extern crate chrono;
extern crate regex;

#[macro_use]
extern crate nom;

use clap::App;

use std::process;

mod remote;
mod packfile;
mod store;
mod delta;
mod command;

macro_rules! subcommand_dispatch {
    ($application:ident, $result:ident, $($name:expr => $subcommand:ident),+) => {
        let $result = {
            let app_matches = $application
                $(.subcommand(command::$subcommand::spec()))+
                .get_matches();
            match app_matches.subcommand_name() {
                $(
                Some($name) => {
                    let params = command::$subcommand::parse(&app_matches
                        .subcommand_matches($name)
                        .unwrap());
                    command::$subcommand::execute(params)
                },
                )+
                Some(_) => unreachable!(),
                None => {
                    println!("{}", app_matches.usage());
                    Ok(())
                }
            }
        }
    }
}

fn main() {
    let app = App::new("rgit")
        .version("0.1.0")
        .about("A Git implementation in Rust.");

    subcommand_dispatch!(app, result,
        "clone" => clone,
        "clone_ssh" => clone_ssh,
        "ls-remote" => ls_remote,
        "ls-remote-ssh" => ls_remote_ssh,
        "log" => log,
        "test-delta" => test_delta);

    if let Err(e) = result {
        println!("Error: {}", e);
        process::exit(-1)
    }
}

