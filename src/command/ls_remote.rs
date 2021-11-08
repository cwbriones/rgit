use anyhow::Result;
use structopt::StructOpt;

use crate::remote::GitClient;
use crate::remote::httpclient::GitHttpClient;
use crate::packfile::refs::GitRef;

#[derive(StructOpt)]
#[structopt(name = "ls-remote", about = "list available refs in a remote repository")]
pub struct ListRemote {
    repo: String,
}

///
/// Lists remote refs available in the given repo.
///
impl ListRemote {
    pub fn execute(&self) -> Result<()> {
        let mut client = GitHttpClient::new(&self.repo);
        let pktlines = client.discover_refs()?;
        for p in &pktlines {
            let &GitRef{ref id, ref name} = p;
            println!("{}\t{}", id, name);
        }
        Ok(())
    }
}
