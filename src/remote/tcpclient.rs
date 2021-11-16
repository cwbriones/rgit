use std::io::Write;
use std::net::IpAddr;
use std::net::TcpStream;
use std::net::ToSocketAddrs;

use anyhow::anyhow;
use anyhow::Result;

use super::GitClient;
use crate::packfile::refs::GitRef;

pub struct GitTcpClient {
    stream: TcpStream,
    repo: String,
    host: IpAddr,
}

impl GitTcpClient {
    pub fn connect<A: ToSocketAddrs>(addr: A, repo: &str) -> Result<Self> {
        let addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| anyhow!("empty addrs"))?;
        let stream = TcpStream::connect(addr)?;
        Ok(GitTcpClient {
            repo: repo.to_owned(),
            stream,
            host: addr.ip(),
        })
    }

    ///
    /// Creates the proto request needed to initiate a connection
    ///
    fn git_proto_request(&self) -> Vec<u8> {
        let mut request = Vec::new();
        let s: String = [
            "git-upload-pack /",
            &self.repo[..],
            "\0host=",
            &self.host.to_string(),
            "\0",
        ]
        .concat();
        super::write_pktline(&s[..], &mut request);
        request
    }
}

impl GitClient for GitTcpClient {
    fn discover_refs(&mut self) -> Result<Vec<GitRef>> {
        let payload = self.git_proto_request();
        self.stream.write_all(&payload)?;

        let response = super::receive(&mut self.stream)?;
        let (_server_capabilities, refs) = super::parse_lines(&response)?;
        Ok(refs)
    }

    fn fetch_packfile(&mut self, want: &[GitRef]) -> Result<Vec<u8>> {
        let capabilities = ["multi_ack_detailed", "side-band-64k", "agent=git/1.8.1"];
        let request = super::create_negotiation_request(&capabilities[..], want);
        self.stream.write_all(&request[..])?;

        super::receive_with_sideband(&mut self.stream)
    }
}
