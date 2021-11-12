use std::io;
use std::io::Write;
use std::net::TcpStream;

use anyhow::Result;

use super::GitClient;
use crate::packfile::refs::GitRef;

pub struct GitTcpClient {
    stream: TcpStream,
    repo: String,
    host: String,
    _port: u16,
}

impl GitTcpClient {
    //pub fn connect<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
    #[allow(dead_code)]
    pub fn connect(repo: &str, host: &str, port: u16) -> io::Result<Self> {
        let stream = TcpStream::connect((host, port))?;
        Ok(GitTcpClient {
            repo: repo.to_owned(),
            stream,
            host: host.to_owned(),
            _port: port,
        })
    }

    ///
    /// Creates the proto request needed to initiate a connection
    ///
    fn git_proto_request(&self) -> String {
        let s: String = [
            "git-upload-pack /",
            &self.repo[..],
            "\0host=",
            &self.host[..],
            "\0",
        ]
        .concat();
        super::pktline(&s[..])
    }
}

impl GitClient for GitTcpClient {
    fn discover_refs(&mut self) -> Result<Vec<GitRef>> {
        let payload = self.git_proto_request();
        self.stream.write_all(payload.as_bytes())?;

        let response = super::receive(&mut self.stream)?;
        let (_server_capabilities, refs) = super::parse_lines(&response);
        Ok(refs)
    }

    fn fetch_packfile(&mut self, want: &[GitRef]) -> Result<Vec<u8>> {
        let capabilities = ["multi_ack_detailed", "side-band-64k", "agent=git/1.8.1"];
        let request = super::create_negotiation_request(&capabilities[..], want);
        self.stream.write_all(request.as_bytes())?;

        super::receive_with_sideband(&mut self.stream)
    }
}
