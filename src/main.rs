#![feature(io)]
#![feature(net)]
#![feature(old_path)]
#![feature(core)]
#![feature(collections)]
#![feature(exit_status)]

extern crate getopts;
extern crate flate2;
extern crate crypto;

use std::env;
use remote::operations as remote_ops;

mod remote;
mod pack;
mod reader;
mod delta;
mod object;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        let status_code = run_command(&args[1], &args[2..]);
        env::set_exit_status(status_code);
    } else {
        let usage =
            "usage: rgit <command> [<args>]\n\n\
            Supported Commands:\n\
            ls-remote <repo>           List references in a remote repository\n";
        print!("{}", usage);
    }
}

fn run_command(command: &String, args: &[String]) -> i32 {
    match &command[..] {
        "test" => {
            match remote_ops::clone_priv("127.0.0.1", 9418, "rgit") {
                Ok(_) => 0,
                Err(_) => -1
            }
        },
        "test-delta" => {
            if let [ref source, ref delta] = args {
                delta::patch_file(&source[..], &delta[..]);
                0
            } else {
                println!("incorrect number of arguments");
                -1
            }
        },
        "ls-remote" => {
            remote_ops::ls_remote("127.0.0.1", 9418, "rgit")
        },
        unknown => {
            println!("Unknown command: {}", unknown);
            -1
        }
    }
}
