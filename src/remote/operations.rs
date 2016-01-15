use remote::tcpclient;
use packfile::PackFile;

use std::fs;
use std::fs::File;

use std::io;
use std::io::Write;

use std::path;

// --------------------------------------------
// Receive Packfile algorithm:
// --------------------------------------------
pub fn receive_packfile(host: &str, port: u16, repo: &str) -> io::Result<(Vec<PacketLine>, Vec<u8>)> {
    tcpclient::with_connection(host, port, |sock| {
        let payload = git_proto_request(host, repo).into_bytes();
        try!(sock.write_all(&payload[..]));

        let response = try!(tcpclient::receive(sock));
        let packets = parse_lines(response);

        let caps = ["multi_ack_detailed", "side-band-64k", "agent=git/1.8.1"];
        let mut request = create_negotiation_request(&caps[..], &packets[..]);
        request.push_str("0000");
        request.push_str(&(pktline("done\n"))[..]);
        try!(sock.write_all(request.as_bytes()));

        let packfile = try!(tcpclient::receive_with_sideband(sock));
        Ok((packets, packfile))
    })
}

pub fn clone_priv(host: &str, port: u16, repo: &str, dir: &str) -> io::Result<()> {
    use std::env;

    let (refs, packfile_data) = try!(receive_packfile(host, port, repo));

    let mut p = path::PathBuf::new();
    p.push(dir);
    p.push(".git");
    p.push("objects");

    try!(fs::create_dir_all(&p));
    try!(env::set_current_dir(&p));

    let filepath = path::Path::new("./pack");

    let mut file = File::create(&filepath).unwrap();
    let _ = file.write_all(&packfile_data[..]);
    file = File::open(&filepath).unwrap();

    let parsed_packfile = PackFile::from_file(file);
    parsed_packfile.unpack_all().ok().expect("Error unpacking parsed packfile");

    // checkout head
    Ok(())
}

// FIXME: This is poorly designed. The receiver should receive on an Optional
// since LastLine is representing this special sentinel value. The capabilities
// shouldn't be tied to the structure since they are only returned in the initial
// response.
#[derive(Debug)]
pub enum PacketLine {
    FirstLine(String, String, Vec<String>),
    RefLine(String, String),
    LastLine
}

pub struct Refline(String, String);

// Encodes a packet-line for communcation.
fn pktline(msg: &str) -> String {
    format!("{:04x}{}", 4 + msg.len() as u8, msg)
}

// Creates the proto request needed to initiate a
// connection.
fn git_proto_request(host: &str, repo: &str) -> String {
    let s: String = ["git-upload-pack /", repo, "\0host=", host, "\0"].concat();
    pktline(&s[..])
}

pub fn ls_remote(host: &str, port: u16, repo: &str) -> i32 {
    match ls_remote_priv(host, port, repo) {
        Ok(pktlines) => {
            print_packetlines(&pktlines);
            0
        },
        Err(_) => -1
    }
}

fn print_packetlines(pktlines: &Vec<PacketLine>) {
    for p in pktlines.iter() {
        match *p {
            PacketLine::FirstLine(ref o, ref r, _) => print!("{}\t{}\n", o, r),
            PacketLine::RefLine(ref o, ref r) => print!("{}\t{}\n", o, r),
            _ => println!("")
        }
    }
}

// Lists all the refs from the given git repo.
fn ls_remote_priv(host: &str, port: u16, repo: &str) -> io::Result<Vec<PacketLine>> {
    tcpclient::with_connection(host, port, |sock| {
        let payload = git_proto_request(host, repo).into_bytes();
        try!(sock.write_all(&payload[..]));

        let lines = try!(tcpclient::receive(sock));

        // Tell the server to close the connection
        let flush_pkt = "0000".as_bytes();
        try!(sock.write_all(flush_pkt));

        Ok(parse_lines(lines))
    })
}

// Create a want request for each packet
// append capabilities to the first ref request
// only send refs that are not peeled and in refs/{heads,tags}
// -- PKT-LINE("want" SP obj-id SP capability-list LF)
// -- PKT-LINE("want" SP obj-id LF)
fn create_negotiation_request(capabilities: &[&str], refs: &[PacketLine]) -> String {
    let mut lines = Vec::with_capacity(refs.len());
    let filtered = refs.iter().filter(|item| {
        match *item {
            &PacketLine::FirstLine(_, ref r, _) => {
                !r.ends_with("^{}") && (r.starts_with("refs/heads") || r.starts_with("refs/tags"))
            },
            &PacketLine::RefLine(_, ref r) => {
                !r.ends_with("^{}") && (r.starts_with("refs/heads") || r.starts_with("refs/tags"))
            },
            _ => false
        }
    });

    for (i, r) in filtered.enumerate() {
        match *r {
            PacketLine::RefLine(ref o, _) => {
                if i == 0 {
                    let caps = capabilities.connect(" ");
                    let line: String = ["want ", &o[..], " ", &caps[..], "\n"].concat();
                    lines.push(pktline(&line[..]));
                }
                let line: String = ["want ", &o[..], "\n"].concat();
                lines.push(pktline(&line[..]));
            },
            _ => ()
        };
    }
    lines.concat()
}

pub fn parse_lines(lines: Vec<String>) -> Vec<PacketLine> {
    lines.iter().map(|s| parse_line(s.trim_right())).collect::<Vec<_>>()
}

// TODO: This is messy and inefficient since we don't need to create this many owned strings
pub fn parse_line(line: &str) -> PacketLine {
    let split_str = line
        .split('\0')
        .collect::<Vec<_>>();

    match split_str.len() {
        1 => {
            let object_ref = split_str[0];
            let v = object_ref.split(' ').collect::<Vec<_>>();
            if v.len() == 2 {
                let (obj_id, r) = (v[0], v[1]);
                PacketLine::RefLine(obj_id.to_string(), r.to_string())
            } else {
                PacketLine::LastLine
            }
        },
        2 => {
            let object_ref = split_str[0];
            let capabilities = split_str[1];
            let v = object_ref.split(' ').collect::<Vec<_>>();
            let c = capabilities.split(' ').map(|s| s.to_string()).collect::<Vec<_>>();
            if v.len() == 2 {
                let (obj_id, r) = (v[0], v[1]);
                PacketLine::FirstLine(obj_id.to_string(), r.to_string(), c)
            } else {
                PacketLine::LastLine
            }
        },
        _ => panic!("error parsing packetline")
    }
}
