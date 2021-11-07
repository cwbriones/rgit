use std::io::Result as IoResult;

use structopt::StructOpt;

use crate::remote::GitClient;
use crate::remote::sshclient::GitSSHClient;
use crate::packfile::refs::GitRef;

#[derive(StructOpt)]
#[structopt(name = "ls-remote-ssh", about = "list available refs in a remote repository, over ssh")]
pub struct SubcommandListRemoteSsh {
    host: String,
    repo: String,
    user: String,
}

impl SubcommandListRemoteSsh {
    ///
    /// Lists remote refs available in the given repo.
    ///
    pub fn execute(&self) -> IoResult<()> {
        let full_repo = [&self.user, "/", &self.repo].join("");
        let mut client = GitSSHClient::new(&self.host, &full_repo);
        client.discover_refs().map(|pktlines| {
            for p in &pktlines {
                let &GitRef{ref id, ref name} = p;
                println!("{}\t{}", id, name);
            }
        })
    }
}
