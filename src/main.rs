extern crate getopts;
extern crate flate2;
extern crate crypto;
extern crate rustc_serialize;
extern crate byteorder;

#[macro_use]
extern crate nom;

use std::env;
use remote::operations as remote_ops;

mod remote;
mod packfile;
mod store;
mod reader;
mod delta;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        let status_code = run_command(&args[1], &args[2..]);
        std::process::exit(status_code)
    } else {
        let usage =
            "usage: rgit <command> [<args>]\n\n\
            Supported Commands:\n\
            ls-remote <repo>           List references in a remote repository\n";
        print!("{}", usage);
    }
}

fn run_command(command: &String, args: &[String]) -> i32 {
    let argc = args.len();
    match &command[..] {
        "clone" => {
            if 0 < argc && argc <= 2 {
                let repo = &args[0];
                let dir = if args.len() == 2 {
                    &args[1]
                } else {
                    repo
                };
                match remote_ops::clone_priv("127.0.0.1", 9418, repo, dir) {
                    Ok(_) => 0,
                    Err(_) => -1
                }
            } else {
                println!("incorrect number of arguments");
                -1
            }
        },
        "test-delta" => {
            if argc == 2 {
                let (source, delta) = (&args[0], &args[1]);
                delta::patch_file(&source[..], &delta[..]);
                0
            } else {
                println!("incorrect number of arguments");
                -1
            }
        },
        "ls-remote" => {
            if argc == 1 {
                let repo = &args[0];
                remote_ops::ls_remote("127.0.0.1", 9418, repo)
            } else {
                println!("incorrect number of arguments");
                -1
            }
        },
        unknown => {
            println!("Unknown command: {}", unknown);
            -1
        }
    }
}
