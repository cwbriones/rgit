use anyhow::Result;
use reqwest::Url;
use structopt::StructOpt;

use crate::packfile::refs::GitRef;

#[derive(StructOpt)]
#[structopt(
    name = "ls-remote",
    about = "list available refs in a remote repository"
)]
pub struct ListRemote {
    #[structopt(parse(try_from_str = super::parse_git_url))]
    remote_url: Url,
}

///
/// Lists remote refs available in the given repo.
///
impl ListRemote {
    pub fn execute(&self) -> Result<()> {
        let mut client = super::create_client(&self.remote_url)?;
        let pktlines = client.discover_refs()?;
        for p in &pktlines {
            let &GitRef { ref id, ref name } = p;
            println!("{}\t{}", id, name);
        }
        Ok(())
    }
}
