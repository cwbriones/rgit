use std::io::IoResult;
use remote::tcpclient;

// --------------------------------------------
// Receive Packfile algorithm:
// --------------------------------------------
pub fn receive_packfile(host: &str, port: u16, repo: &str) -> IoResult<(Vec<PacketLine>, Vec<u8>)> {
    tcpclient::with_connection(host, port, |sock| {
        let payload = git_proto_request(host, repo).into_bytes();
        try!(sock.write(payload.as_slice()));

        let response = try!(tcpclient::receive(sock));
        let packets = parse_lines(response);

        let caps = ["multi_ack_detailed", "side-band-64k", "agent=git/1.8.1"];
        let mut request = create_negotiation_request(caps.as_slice(), packets.as_slice());
        request.push_str("0000");
        request.push_str(pktline("done\n").as_slice());
        try!(sock.write(request.as_bytes()));

        let packfile = try!(tcpclient::receive_with_sideband(sock));
        Ok((packets, packfile))
    })
}

pub fn clone_priv(host: &str, port: u16, repo: &str) -> IoResult<()> {
    use std::io;
    use std::io::{fs, File, Open, Write};

    let (refs, packfile) = try!(receive_packfile(host, port, repo));

    let dir = Path::new("temp_repo");
    fs::mkdir(&dir, io::USER_RWX);

    let filepath = dir.join("pack_file_incoming");

    if let Ok(mut file) = File::open_mode(&filepath, Open, Write) {
        file.write(packfile.as_slice());
    }
    // parse packfile
    // checkout head
    Ok(())
}

#[deriving(Show)]
enum PacketLine {
    FirstLine(String, String, Vec<String>),
    RefLine(String, String),
    LastLine
}

// Encodes a packet-line for communcation.
fn pktline(msg: &str) -> String {
    format!("{:04x}{}", 4 + msg.len() as u8, msg)
}

// Creates the proto request needed to initiate a
// connection.
fn git_proto_request(host: &str, repo: &str) -> String {
    let s = ["git-upload-pack /", repo, "\0host=", host, "\0"].concat();
    pktline(s.as_slice())
}

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

// Create a want request for each packet
// append capabilities to the first ref request
// only send refs that are not peeled and in refs/{heads,tags}
// -- PKT-LINE("want" SP obj-id SP capability-list LF)
// -- PKT-LINE("want" SP obj-id LF)
fn create_negotiation_request(capabilities: &[&str], refs: &[PacketLine]) -> String {
    let mut lines = Vec::with_capacity(refs.len());
    let mut filtered = refs.iter().filter(|item| {
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
            PacketLine::RefLine(ref o, ref r) => {
                if i == 0 {
                    let caps = capabilities.connect(" ");
                    let line = ["want ", o.as_slice(), " ", caps.as_slice(), "\n"].concat();
                    lines.push(pktline(line.as_slice()));
                }
                let line = ["want ", o.as_slice(), "\n"].concat();
                lines.push(pktline(line.as_slice()));
            },
            _ => ()
        };
    }
    lines.concat()
}

pub fn parse_lines(lines: Vec<String>) -> Vec<PacketLine> {
    lines.iter().map(|s| parse_line(s.trim_right_chars('\n'))).collect::<Vec<_>>()
}

// TODO: This is messy and inefficient since we don't need to create this many owned strings
pub fn parse_line(line: &str) -> PacketLine {
    let split_str = line
        .split('\0')
        .collect::<Vec<_>>();

    match split_str.as_slice() {
        [object_ref, capabilities] => {
            let v = object_ref.as_slice().split(' ').collect::<Vec<_>>();
            let c = capabilities.as_slice().split(' ').map(|s| s.to_string()).collect::<Vec<_>>();
            match v.as_slice() {
                [ref obj_id, ref r] => PacketLine::FirstLine(obj_id.to_string(), r.to_string(), c),
                _ => PacketLine::LastLine
            }
        },
        [object_ref] => {
            let v = object_ref.as_slice().split(' ').collect::<Vec<_>>();
            match v.as_slice() {
                [obj_id, r] => PacketLine::RefLine(obj_id.to_string(), r.to_string()),
                _ => PacketLine::LastLine
            }
        }
        _ => panic!("error parsing packetline")
    }
}
