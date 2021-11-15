use anyhow::Context;
use anyhow::Result;
use structopt::StructOpt;

use crate::packfile::refs::GitRef;
use crate::remote::httpclient::GitHttpClient;
use crate::remote::GitClient;

#[derive(StructOpt)]
#[structopt(
    name = "ls-remote",
    about = "list available refs in a remote repository"
)]
pub struct ListRemote {
    repo: String,
}

///
/// Lists remote refs available in the given repo.
///
impl ListRemote {
    pub fn execute(&self) -> Result<()> {
        let mut client = GitHttpClient::new(&self.repo).with_context(|| "create http client")?;
        let pktlines = client.discover_refs()?;
        for p in &pktlines {
            let &GitRef { ref id, ref name } = p;
            println!("{}\t{}", id, name);
        }
        Ok(())
    }
}
