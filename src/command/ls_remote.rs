use std::io::Result as IoResult;

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
    pub fn execute(&self) -> IoResult<()> {
        let mut client = GitHttpClient::new(&self.repo);
        client.discover_refs().map(|pktlines| {
            for p in &pktlines {
                let &GitRef{ref id, ref name} = p;
                println!("{}\t{}", id, name);
            }
        })
    }
}
