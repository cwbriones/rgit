use ssh2::{Session};

use std::io::Write;
use std::io::Result as IoResult;
use std::net::TcpStream;

use packfile::refs::GitRef;
use super::GitClient;

pub struct GitSSHClient {
    sess: Session,
    repo: String
}

impl GitSSHClient {
    pub fn new(host: &str, repo: &str) -> Self {
        let stream = TcpStream::connect(host).unwrap();
        let mut sess = Session::new().unwrap();
        sess.set_tcp_stream(stream);
        sess.handshake().unwrap();
        {
            let mut agent = sess.agent().unwrap();
            agent.connect().unwrap();
            sess.userauth_agent("git").unwrap();
        }
        assert!(sess.authenticated());

        GitSSHClient {
            sess: sess,
            repo: repo.to_owned(),
        }
    }
}

impl GitClient for GitSSHClient {
    fn discover_refs(&mut self) -> IoResult<Vec<GitRef>> {
        let mut chan = self.sess.channel_session().unwrap();
        let command = format!("git-upload-pack {}", self.repo);
        chan.exec(&command).unwrap();

        let response = try!(super::receive(&mut chan));
        let (_server_capabilities, refs) = super::parse_lines(&response);
        Ok(refs)
    }

    fn fetch_packfile(&mut self, want: &[GitRef]) -> IoResult<Vec<u8>> {
        let capabilities = ["multi_ack_detailed", "side-band-64k", "agent=git/1.8.1"];

        // FIXME: We shouldn't have to call this command twice because then we are just
        // doing twice the work receiving the refs.
        let command = format!("git-upload-pack {}", self.repo);
        let mut chan = self.sess.channel_session().unwrap();
        chan.exec(&command).unwrap();

        try!(super::receive(&mut chan));
        //let (_server_capabilities, refs) = super::parse_lines(&response);

        let request = super::create_negotiation_request(&capabilities[..], &want[..]);

        try!(chan.write_all(request.as_bytes()));
        super::receive_with_sideband(&mut chan)
    }
}
