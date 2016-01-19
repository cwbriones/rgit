use std::io;

use packfile::refs::GitRef;

pub mod tcpclient;
pub mod httpclient;
pub mod operations;

pub trait GitClient {
    // Required Methods
    fn discover_refs(&mut self) -> io::Result<Vec<GitRef>>;
    fn fetch_packfile(&mut self, want: &[GitRef]) -> io::Result<Vec<u8>>;
}

///
/// Encodes a packet-line for communcation.
///
fn pktline(msg: &str) -> String {
    format!("{:04x}{}", 4 + msg.len() as u8, msg)
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
            let caps = capabilities.join(" ");
            // if this is a space it is correctly multiplexed
            let line: String = ["want ", &o[..], " ", &caps[..], "\n"].concat();
            lines.push(pktline(&line[..]));
        }
        let line: String = ["want ", &o[..], "\n"].concat();
        lines.push(pktline(&line[..]));
    }
    lines.push("0000".to_string());
    lines.push(pktline("done\n"));
    lines.concat()
}

///
/// Parses all packetlines received from the server into a list of capabilities and a list of refs.
///
fn parse_lines(lines: &[String]) -> (Vec<String>, Vec<GitRef>) {
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

///
/// Parses the first packetline from the server into a list of capabilities and a ref.
///
fn parse_first_line(line: &str) -> (Vec<String>, GitRef) {
    let split = line
        .split('\0')
        .collect::<Vec<_>>();
    let the_ref = parse_line(split[0]);
    let capabilities = split[1].split(' ').map(|s| s.to_string()).collect::<Vec<_>>();
    (capabilities, the_ref)
}

///
/// Parses a line from the server into a ref.
///
fn parse_line(line: &str) -> GitRef {
    let split = line
        .split(' ')
        .collect::<Vec<_>>();

    let (obj_id, name) = (split[0], split[1]);
    GitRef{id: obj_id.to_owned(), name: name.to_owned()}
}
