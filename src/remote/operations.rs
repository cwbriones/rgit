use std::io::IoResult;
use remote::tcpclient;

enum PacketLine {
    FirstLine(String, String, Vec<String>),
    RefLine(String, String),
    LastLine
}

// Encodes a packet-line for communcation.
fn pktline(msg: &str) -> String {
    let mut s = format!("{:04x}", 4 + msg.len() as u8);
    s.push_str(msg);
    s
}

// Creates the proto request needed to initiate a
// connection.
fn git_proto_request(host: &str, repo: &str) -> String {
    let mut result = String::from_str("git-upload-pack /");

    result.push_str(repo);
    result.push_str("\0host=");
    result.push_str(host);
    result.push_str("\0");

    pktline(result.as_slice())
}

fn create_negotiation_request(capabilities: &[String], refs: &[String]) -> String {
    // Create a want request for each packet
    // append capabilities to the first ref request
    // only send refs that are not peeled and in refs/{heads,tags}kk
}

// Lists all the refs from the given git repo.
pub fn ls_remote(host: &str, port: u16, repo: &str) -> IoResult<Vec<PacketLine>> {
    tcpclient::with_connection(host, port, |sock| {
        let payload = git_proto_request(host, repo).into_bytes();
        try!(sock.write(payload.as_slice()));

        let lines = try!(tcpclient::receive(sock));

        // Tell the server to close the connection
        let flush_pkt = "0000".as_bytes();
        try!(sock.write(flush_pkt));

        Ok(parse_lines(lines))
    })
}

pub fn parse_lines(lines: Vec<String>) -> Vec<PacketLine> {
    let mut result = vec![];
    for (i, line) in lines.into_iter().enumerate() {
        if i == 0 {
            result.push(parse_first_line(line));
        } else {
            result.push(parse_line(line))
        }
    }
    result
}

pub fn parse_first_line(line: String) -> PacketLine {
    let split_str = line
        .as_slice()
        .split('\0')
        .collect::<Vec<_>>();

    match split_str.as_slice() {
        [objectid, reference, capabilities] => {
            let v = capabilities.as_slice().split(' ').map(|s| {s.to_string()}).collect::<Vec<_>>();
            PacketLine::FirstLine(objectid.to_string(), reference.to_string(), v)
        }
        _ => panic!("error parsing first line!")
    }
}

pub fn parse_line(line: String) -> PacketLine {
    let split_str = line
        .as_slice()
        .split('\0')
        .collect::<Vec<_>>();

    match split_str.as_slice() {
        [objectid, reference] => PacketLine::RefLine(objectid.to_string(), reference.to_string()),
        [_objectid] => PacketLine::LastLine,
        _ => panic!("error parsing packetline")
    }
}
