use anyhow::Result;
use structopt::StructOpt;

use crate::packfile::refs::GitRef;
use crate::remote::sshclient::GitSSHClient;
use crate::remote::GitClient;

#[derive(StructOpt)]
#[structopt(
    name = "ls-remote-ssh",
    about = "list available refs in a remote repository, over ssh"
)]
pub struct SubcommandListRemoteSsh {
    host: String,
    repo: String,
    user: String,
}

impl SubcommandListRemoteSsh {
    ///
    /// Lists remote refs available in the given repo.
    ///
    pub fn execute(&self) -> Result<()> {
        let full_repo = [&self.user, "/", &self.repo].join("");
        let mut client = GitSSHClient::new(&self.host, &full_repo)?;
        let pktlines = client.discover_refs()?;
        for p in &pktlines {
            let &GitRef { ref id, ref name } = p;
            println!("{}\t{}", id, name);
        }
        Ok(())
    }
}
