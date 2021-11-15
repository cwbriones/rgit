use anyhow::anyhow;
use anyhow::Result;
use structopt::StructOpt;

use crate::packfile::refs;
use crate::remote::sshclient::GitSSHClient;
use crate::remote::GitClient;
use crate::store::Repo;

#[derive(StructOpt)]
#[structopt(name = "clone-ssh", about = "clone a remote repository over ssh")]
pub struct SubCommandCloneSsh {
    host: String,
    repo: String,
    user: String,
}

impl SubCommandCloneSsh {
    pub fn execute(self) -> Result<()> {
        let dir = self
            .repo
            .split('.')
            .next()
            .ok_or_else(|| anyhow!("repo did not end in .git"))?;
        let full_repo = [&self.user, "/", &self.repo].join("");
        let mut client = GitSSHClient::new(&self.host, &full_repo)?;

        println!("Cloning into \"{}\"...", dir);

        let refs = client.discover_refs()?;
        let packfile_data = client.fetch_packfile(&refs)?;

        let repo = Repo::from_packfile(dir, &packfile_data)?;

        refs::create_refs(dir, &refs)?;
        refs::update_head(dir, &refs)?;

        // Checkout head and format refs
        repo.checkout_head()?;
        Ok(())
    }
}
