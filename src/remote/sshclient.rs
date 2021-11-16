use std::io::Write;
use std::net::TcpStream;

use anyhow::Result;
use ssh2::Session;

use super::GitClient;
use crate::packfile::refs::GitRef;

pub struct GitSSHClient {
    sess: Session,
    repo: String,
}

impl GitSSHClient {
    pub fn new(host: &str, repo: &str) -> Result<Self> {
        let stream = TcpStream::connect(host)?;
        let mut sess = Session::new()?;
        sess.set_tcp_stream(stream);
        sess.handshake()?;
        {
            let mut agent = sess.agent()?;
            agent.connect()?;
            sess.userauth_agent("git")?;
        }
        assert!(sess.authenticated());

        Ok(GitSSHClient {
            sess,
            repo: repo.to_owned(),
        })
    }
}

impl GitClient for GitSSHClient {
    fn discover_refs(&mut self) -> Result<Vec<GitRef>> {
        let mut chan = self.sess.channel_session()?;
        let command = format!("git-upload-pack {}", self.repo);
        chan.exec(&command)?;

        let response = super::receive(&mut chan)?;
        let (_server_capabilities, refs) = super::parse_lines(&response)?;
        Ok(refs)
    }

    fn fetch_packfile(&mut self, want: &[GitRef]) -> Result<Vec<u8>> {
        let capabilities = ["multi_ack_detailed", "side-band-64k", "agent=git/1.8.1"];

        // FIXME: We shouldn't have to call this command twice because then we are just
        // doing twice the work receiving the refs.
        let command = format!("git-upload-pack {}", self.repo);
        let mut chan = self.sess.channel_session()?;
        chan.exec(&command)?;

        super::receive(&mut chan)?;
        //let (_server_capabilities, refs) = super::parse_lines(&response);

        let request = super::create_negotiation_request(&capabilities[..], want);

        chan.write_all(&request[..])?;
        super::receive_with_sideband(&mut chan)
    }
}
