extern crate getopts;

use std::os;
use std::io::IoResult;

mod tcpclient;

fn main() {
    let args: Vec<String> = os::args();
    let program = args[0].clone();

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

fn run_command(command: &String, args: &[String]) -> int {
    match command.as_slice() {
        "ls-remote" => {
            ls_remote("127.0.0.1", 9418, "rgit");
            0
        },
        unknown => {
            println!("Unknown command: {}", unknown);
            -1
        }
    }
}

fn pktline(msg: &str) -> String {
    let mut s = format!("{:04x}", 4 + msg.len() as u8);
    s.push_str(msg);
    s
}

fn git_proto_request(host: &str, repo: &str) -> String {
    let mut result = std::string::String::from_str("git-upload-pack /");

    result.push_str(repo);
    result.push_str("\0host=");
    result.push_str(host);
    result.push_str("\0");

    pktline(result.as_slice())
}

fn ls_remote(host: &str, port: u16, repo: &str) -> IoResult<Vec<String>> {
    tcpclient::with_connection(host, port, |sock| {
        let payload = git_proto_request(host, repo).into_bytes();
        try!(sock.write(payload.as_slice()));

        let lines = try!(tcpclient::receive(sock));
        println!("");

        let flush_pkt = "0000".as_bytes();
        try!(sock.write(flush_pkt));
        Ok(lines)
    })
}
