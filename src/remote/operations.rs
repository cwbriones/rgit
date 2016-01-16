use remote::tcpclient;
use packfile::PackFile;
use packfile::refs;

use std::fs;
use std::fs::File;

use std::io;
use std::io::Write;

use std::path;
use packfile::refs::GitRef;

///
/// Receives a packfile from the given git repo as a vector of bytes and a vector of refs.
///
pub fn receive_packfile(host: &str, port: u16, repo: &str) -> io::Result<(Vec<GitRef>, Vec<u8>)> {
    tcpclient::with_connection(host, port, |sock| {
        let payload = git_proto_request(host, repo).into_bytes();
        try!(sock.write_all(&payload[..]));

        let response = try!(tcpclient::receive(sock));
        let (_server_capabilities, refs) = parse_lines(response);

        // We ignore the actual capabilities of the git server since we do not support multiple
        // forms of communication.
        let capabilities = ["multi_ack_detailed", "side-band-64k", "agent=git/1.8.1"];
        let mut request = create_negotiation_request(&capabilities[..], &refs[..]);
        request.push_str("0000");
        request.push_str(&(pktline("done\n"))[..]);
        try!(sock.write_all(request.as_bytes()));

        let packfile = try!(tcpclient::receive_with_sideband(sock));
        Ok((refs, packfile))
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

    let mut file = try!(File::create(&filepath));
    try!(file.write_all(&packfile_data[..]));
    file = try!(File::open(&filepath));

    let parsed_packfile = PackFile::from_file(file);
    match parsed_packfile.unpack_all() {
        Ok(_) => (),
        Err(e) => {
            println!("Error: {:?}", e);
            panic!("Error unpacking parsed packfile");
        }
    }
    refs::create_refs(refs);

    // Checkout head and format refs
    Ok(())
}

/// Encodes a packet-line for communcation.
fn pktline(msg: &str) -> String {
    format!("{:04x}{}", 4 + msg.len() as u8, msg)
}

/// Creates the proto request needed to initiate a
/// connection.
fn git_proto_request(host: &str, repo: &str) -> String {
    let s: String = ["git-upload-pack /", repo, "\0host=", host, "\0"].concat();
    pktline(&s[..])
}

/// Lists remote refs available in the given repo.
pub fn ls_remote(host: &str, port: u16, repo: &str) -> i32 {
    match ls_remote_priv(host, port, repo) {
        Ok(pktlines) => {
            print_packetlines(&pktlines);
            0
        },
        Err(_) => -1
    }
}

fn print_packetlines(pktlines: &Vec<GitRef>) {
    for p in pktlines.iter() {
        let &GitRef{id: ref id, name: ref name} = p;
        print!("{}\t{}\n", id, name);
    }
}

///
/// Lists all the refs from the given git repo.
///
fn ls_remote_priv(host: &str, port: u16, repo: &str) -> io::Result<Vec<GitRef>> {
    tcpclient::with_connection(host, port, |sock| {
        let payload = git_proto_request(host, repo).into_bytes();
        try!(sock.write_all(&payload[..]));

        let lines = try!(tcpclient::receive(sock));

        // Tell the server to close the connection
        let flush_pkt = "0000".as_bytes();
        try!(sock.write_all(flush_pkt));

        let (capabilities, refs) = parse_lines(lines);
        Ok(refs)
    })
}

// Create a want request for each packet
// append capabilities to the first ref request
// only send refs that are not peeled and in refs/{heads,tags}
// -- PKT-LINE("want" SP obj-id SP capability-list LF)
// -- PKT-LINE("want" SP obj-id LF)
fn create_negotiation_request(capabilities: &[&str], refs: &[GitRef]) -> String {
    let mut lines = Vec::with_capacity(refs.len());
    let filtered = refs.iter().filter(|&&GitRef{name: ref r, ..}| {
        !r.ends_with("^{}") && (r.starts_with("refs/heads") || r.starts_with("refs/tags"))
    });
    for (i, r) in filtered.enumerate() {
        let &GitRef{id: ref o, ..} = r;
        if i == 0 {
            let caps = capabilities.connect(" ");
            let line: String = ["want ", &o[..], " ", &caps[..], "\n"].concat();
            lines.push(pktline(&line[..]));
        }
        let line: String = ["want ", &o[..], "\n"].concat();
        lines.push(pktline(&line[..]));
    }
    lines.concat()
}

/// Parses all packetlines received from the server into a list of capabilities and a list of refs.
pub fn parse_lines(lines: Vec<String>) -> (Vec<String>, Vec<GitRef>) {
    assert!(lines.len() > 1);
    let mut iter = lines.iter().map(|s| s.trim_right());

    // First line contains capabilities separated by '\0'
    let mut parsed = Vec::new();
    let first = iter.next().unwrap();
    let (capabilities, first_ref) = parse_first_line(first);
    parsed.push(first_ref);

    for line in iter {
        parsed.push(parse_line(line))
    }
    (capabilities, parsed)
}

/// Parses the first packetline from the server into a list of capabilities and a ref.
fn parse_first_line(line: &str) -> (Vec<String>, GitRef) {
    let split = line
        .split('\0')
        .collect::<Vec<_>>();
    let the_ref = parse_line(split[0]);
    let capabilities = split[1].split(' ').map(|s| s.to_string()).collect::<Vec<_>>();
    (capabilities, the_ref)
}

/// Parses a packetline from the server into a ref.
fn parse_line(line: &str) -> GitRef {
    let split = line
        .split(' ')
        .collect::<Vec<_>>();

    let (obj_id, name) = (split[0], split[1]);
    GitRef{id: obj_id.to_owned(), name: name.to_owned()}
}
