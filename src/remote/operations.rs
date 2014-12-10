use std::io::IoResult;
use remote::tcpclient;

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

// Lists all the refs from the given git repo.
pub fn ls_remote(host: &str, port: u16, repo: &str) -> IoResult<Vec<String>> {
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

