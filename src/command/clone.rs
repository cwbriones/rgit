use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use reqwest::Url;
use structopt::StructOpt;

use super::validators;
use crate::packfile::refs;
use crate::remote::httpclient::GitHttpClient;
use crate::remote::GitClient;
use crate::store::Repo;

#[derive(StructOpt)]
#[structopt(name = "clone", about = "clone a remote repository")]
pub struct SubcommandClone {
    #[structopt(validator = validators::is_url_or_ssh_repo)]
    repo: Url,
    dir: Option<PathBuf>,
}

impl SubcommandClone {
    // FIXME: Address clones
    pub fn execute(&self) -> Result<()> {
        let dir = self
            .dir
            .clone()
            .or_else(|| {
                // TODO: URLDecode path?
                let path = Path::new(self.repo.path());
                path.with_extension("")
                    .file_name()
                    .map(|p| Path::new(p).to_owned())
            })
            .ok_or_else(|| anyhow!("could not infer repo directory from url"))?;
        // Specific client should normalize, not here.
        let mut repo = self.repo.clone();
        normalize_path(&mut repo);
        let http_client = GitHttpClient::new(repo.clone()).with_context(|| "create http client")?;

        let (mut client, dir) = (Box::new(http_client), dir);
        //     Err(_) => {
        //         let client = GitTcpClient::connect(&self.repo, "127.0.0.1", 9418)?;
        //         let dir = self.dir.clone().unwrap_or_else(|| self.repo.clone());
        //         (Box::new(client), dir)
        //     }
        // };
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

fn normalize_path(url: &mut Url) {
    if url.path().ends_with('/') {
        return;
    }
    let mut path = url.path().to_string();
    if !path.ends_with(".git") {
        path.push_str(".git");
    }
    path.push('/');
    url.set_path(&path);
}
