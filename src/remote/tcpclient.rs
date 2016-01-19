use std::io;
use std::io::{Read,Write};
use std::result::Result::Err;
use std::net::TcpStream;
use std::str;

use packfile::refs::GitRef;

use super::GitClient;

pub struct GitTcpClient {
    stream: TcpStream,
    repo: String,
    host: String,
    port: u16,
}

impl GitTcpClient {
    //pub fn connect<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
    pub fn connect(repo: &str, host: &str, port: u16) -> io::Result<Self> {
        let stream = try!(TcpStream::connect((host, port)));
        Ok(GitTcpClient{repo: repo.to_string(), stream: stream, host: host.to_string(), port: port})
    }

    ///
    /// Reads and parses a packet-line from the server.
    ///
    fn read_packet_line(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut header = [0; 4];
        self.stream.read_exact(&mut header).expect("error parsing header.");
        let length_str = str::from_utf8(&header[..]).unwrap();
        let length = u64::from_str_radix(length_str, 16).unwrap();

        if length > 4 {
            let mut pkt = vec![0; (length - 4) as usize];
            try!(self.stream.read_exact(&mut pkt));
            Ok(Some(pkt))
        } else {
            Ok(None)
        }
    }

    ///
    /// Reads and parses packet-lines from the given connection
    /// until a null packet is received.
    ///
    fn receive(&mut self) -> io::Result<Vec<String>> {
        let mut lines = vec![];
        loop {
            match self.read_packet_line() {
                Ok(Some(line)) => {
                    let s: String = str::from_utf8(&line[..]).unwrap().to_string();
                    lines.push(s)
                }
                Ok(None)       => return Ok(lines),
                Err(e)         => return Err(e)
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
    pub fn receive_with_sideband(&mut self) -> io::Result<Vec<u8>> {
        let mut packfile_data = Vec::new();
        loop {
            match try!(self.read_packet_line()) {
                Some(line) => {
                    if &line[..] == "NAK\n".as_bytes() {
                        continue;
                    }
                    match line[0] {
                        1 => {
                            // TODO: This only uses a loop because Vec::push_all was
                            // not stabilized for Rust 1.0
                            for i in &line[1..] {
                                packfile_data.push(*i)
                            }
                        }
                        2 => {
                            let msg = str::from_utf8(&line[1..]).unwrap();
                            print!("{}", msg);
                        }
                        _ => return Err(io::Error::new(io::ErrorKind::Other, "Git server returned error"))
                    }
                }
                None => return Ok(packfile_data)
            }
        }
    }

    ///
    /// Creates the proto request needed to initiate a connection
    ///
    fn git_proto_request(&self) -> String {
        let s: String = ["git-upload-pack /", &self.repo[..], "\0host=", &self.host[..], "\0"].concat();
        super::pktline(&s[..])
    }
}

impl GitClient for GitTcpClient {
    fn discover_refs(&mut self) -> io::Result<Vec<GitRef>> {
        let payload = self.git_proto_request();
        try!(self.stream.write_all(payload.as_bytes()));

        let response = try!(self.receive());
        let (_server_capabilities, refs) = super::parse_lines(&response);
        Ok(refs)
    }

    fn fetch_packfile(&mut self, want: &[GitRef]) -> io::Result<Vec<u8>> {
        let capabilities = ["multi_ack_detailed", "side-band-64k", "agent=git/1.8.1"];
        let mut request = super::create_negotiation_request(&capabilities[..], &want[..]);
        try!(self.stream.write_all(request.as_bytes()));

        self.receive_with_sideband()
    }
}
