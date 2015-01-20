#![allow(unstable)]
#![feature(advanced_slice_patterns)]
extern crate getopts;

use std::os;
use remote::operations as remote_ops;

mod remote;
mod zlib;

fn main() {
    let args: Vec<String> = os::args();

    if args.len() > 1 {
        let status_code = run_command(&args[1], args.slice_from(2));
        os::set_exit_status(status_code);
    } else {
        let usage =
            "usage: rgit <command> [<args>]\n\n\
            Supported Commands:\n\
            ls-remote <repo>           List references in a remote repository\n";
        print!("{}", usage);
    }
}

fn run_command(command: &String, args: &[String]) -> isize {
    match command.as_slice() {
        "test" => {
            match remote_ops::clone_priv("127.0.0.1", 9418, "rgit") {
                Ok(_) => 0,
                Err(_) => -1
            }
        }
        "ls-remote" => {
            remote_ops::ls_remote("127.0.0.1", 9418, "rgit")
        },
        unknown => {
            println!("Unknown command: {}", unknown);
            -1
        }
    }
}
