use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Result;
use reqwest::Url;
use structopt::StructOpt;

use crate::packfile::refs;
use crate::store::Repo;

#[derive(StructOpt)]
#[structopt(name = "clone", about = "clone a remote repository")]
pub struct SubcommandClone {
    #[structopt(parse(try_from_str = super::parse_git_url))]
    remote_url: Url,
    dir: Option<PathBuf>,
}

impl SubcommandClone {
    pub fn execute(&self) -> Result<()> {
        let dir = self
            .dir
            .clone()
            .or_else(|| {
                // TODO: URLDecode path?
                let path = Path::new(self.remote_url.path());
                path.with_extension("")
                    .file_name()
                    .map(|p| Path::new(p).to_owned())
            })
            .ok_or_else(|| anyhow!("could not infer repo directory from url"))?;

        let mut client = super::create_client(&self.remote_url)?;
        println!("Cloning into \"{}\"...", dir.as_os_str().to_string_lossy());

        let refs = client.discover_refs()?;
        let packfile_data = client.fetch_packfile(&refs)?;

        let repo = Repo::from_packfile(&dir, &packfile_data)?;

        refs::create_refs(repo.gitdir(), &refs)?;
        refs::update_head(repo.gitdir(), &refs)?;

        // Checkout head and format refs
        repo.checkout_head()?;
        Ok(())
    }
}
