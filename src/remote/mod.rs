use std::io::Read;
use std::str;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;

use crate::packfile::refs::GitRef;

pub mod httpclient;
pub mod sshclient;
pub mod tcpclient;

pub trait GitClient {
    fn discover_refs(&mut self) -> Result<Vec<GitRef>>;
    fn fetch_packfile(&mut self, want: &[GitRef]) -> Result<Vec<u8>>;
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
    let filtered = refs.iter().filter(|&&GitRef { name: ref r, .. }| {
        !r.ends_with("^{}") && (r.starts_with("refs/heads") || r.starts_with("refs/tags"))
    });
    for (i, r) in filtered.enumerate() {
        let &GitRef { id: ref o, .. } = r;
        if i == 0 {
            let caps = capabilities.join(" ");
            // if this is a space it is correctly multiplexed
            let line: String = ["want ", &o[..], " ", &caps[..], "\n"].concat();
            lines.push(pktline(&line[..]));
        }
        let line: String = ["want ", &o[..], "\n"].concat();
        lines.push(pktline(&line[..]));
    }
    lines.push("0000".to_owned());
    lines.push(pktline("done\n"));
    lines.concat()
}

///
/// Parses all packetlines received from the server into a list of capabilities and a list of refs.
///
fn parse_lines(lines: &[String]) -> Result<(Vec<String>, Vec<GitRef>)> {
    let mut iter = lines.iter().map(|s| s.trim_end());

    // First line contains capabilities separated by '\0'
    let mut parsed = Vec::new();
    let first = iter
        .next()
        .ok_or_else(|| anyhow!("expected at least one line"))?;
    let (capabilities, first_ref) = parse_first_line(first);
    parsed.push(first_ref);

    for line in iter {
        parsed.push(parse_line(line))
    }
    Ok((capabilities, parsed))
}

///
/// Parses the first packetline from the server into a list of capabilities and a ref.
///
fn parse_first_line(line: &str) -> (Vec<String>, GitRef) {
    let split = line.split('\0').collect::<Vec<_>>();
    let the_ref = parse_line(split[0]);
    let capabilities = split[1]
        .split(' ')
        .map(|s| s.to_owned())
        .collect::<Vec<_>>();
    (capabilities, the_ref)
}

///
/// Parses a line from the server into a ref.
///
fn parse_line(line: &str) -> GitRef {
    let split = line.split(' ').collect::<Vec<_>>();

    let (obj_id, name) = (split[0], split[1]);
    GitRef {
        id: obj_id.to_owned(),
        name: name.to_owned(),
    }
}

///
/// Reads and parses packet-lines from the given connection
/// until a null packet is received.
///
fn receive<R: Read>(reader: &mut R) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    let mut line = Vec::new();
    loop {
        read_packet_line(reader, &mut line)?;
        if line.is_empty() {
            return Ok(lines);
        } else {
            let s = str::from_utf8(&line[..])?;
            lines.push(s.into())
        }
    }
}

///
/// Receives a multiplexed response from the git server.
/// The mulitplexing protocol encodes channel information as the first
/// byte returned with each reponse packetline.
///
/// There are three channels:
///    1. Packetfile data
///    2. Progress information to be printed to STDERR
///    3. Error message from server, abort operation
///
pub fn receive_with_sideband<R: Read>(reader: &mut R) -> Result<Vec<u8>> {
    let mut packfile_data = Vec::new();
    let mut line = Vec::new();
    loop {
        read_packet_line(reader, &mut line)?;
        match &line[..] {
            b"NAK\n" => continue,
            [1, packdata @ ..] => packfile_data.extend_from_slice(packdata),
            [2, msg @ ..] => {
                let msg = str::from_utf8(msg)?;
                eprint!("{}", msg);
            }
            [3, msg @ ..] => {
                let msg = str::from_utf8(msg)?;
                eprint!("error: {}", msg);
                return Err(anyhow!("git server returned error",));
            }
            [] => return Ok(packfile_data),
            _ => return Err(anyhow!("invalid response from server")),
        }
    }
}

///
/// Reads and parses a pkt-line from the server.
///
fn read_packet_line<R: Read>(reader: &mut R, buf: &mut Vec<u8>) -> Result<()> {
    let mut header = [0; 4];
    reader.read_exact(&mut header).context("pkt-line header")?;
    let length_str = str::from_utf8(&header[..])?;
    let length = u64::from_str_radix(length_str, 16)?;

    if length > 4 {
        buf.resize((length - 4) as usize, 0);
        reader.read_exact(&mut buf[..])?;
        Ok(())
    } else {
        buf.clear();
        Ok(())
    }
}
