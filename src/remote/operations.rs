use std::io::IoResult;
use remote::tcpclient;

#[deriving(Show)]
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

// fn create_negotiation_request(capabilities: &[String], refs: &[String]) -> String {
//     // Create a want request for each packet
//     // append capabilities to the first ref request
//     // only send refs that are not peeled and in refs/{heads,tags}kk
// }

pub fn ls_remote(host: &str, port: u16, repo: &str) -> int {
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
fn ls_remote_priv(host: &str, port: u16, repo: &str) -> IoResult<Vec<PacketLine>> {
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
    lines.iter().map(|s| parse_line(s.trim_right_chars('\n'))).collect::<Vec<_>>()
}

pub fn parse_line(line: &str) -> PacketLine {
    let split_str = line
        .split('\0')
        .collect::<Vec<_>>();

    match split_str.as_slice() {
        [objectid, reference] => {
            let c = reference.as_slice().split(' ').map(|s| s.to_string()).collect::<Vec<_>>();
            PacketLine::FirstLine(objectid.to_string(), reference.to_string(), c)
        },
        [otherline] => {
            let v = otherline.as_slice().split(' ').collect::<Vec<_>>();
            match v.as_slice() {
                [o, r] => PacketLine::RefLine(o.to_string(), r.to_string()),
                _ => PacketLine::LastLine
            }
        }
        _ => panic!("error parsing packetline")
    }
}
