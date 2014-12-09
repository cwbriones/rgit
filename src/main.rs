use std::io::{IoResult, TcpStream};

mod tcpclient;

fn main() {
    match ls_remote("127.0.0.1", 9418, "rgit") {
        Ok(_) => {
            println!("refs obtained");
        },
        Err(_) => {
            println!("Error!");
        }
    };
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
    let mut sock = TcpStream::connect((host, port)).unwrap();

    let payload = git_proto_request(host, repo).into_bytes();
    try!(sock.write(payload.as_slice()));

    let lines = try!(tcpclient::receive(&mut sock));
    println!("");

    let flush_pkt = "0000".as_bytes();
    try!(sock.write(flush_pkt));
    Ok(lines)
}
