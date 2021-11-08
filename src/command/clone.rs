use std::path::Path;

use anyhow::Result;
use reqwest::Url;
use structopt::StructOpt;

use crate::remote::GitClient;
use crate::remote::httpclient::GitHttpClient;
use crate::remote::tcpclient::GitTcpClient;
use crate::packfile::refs;
use crate::store::Repo;
use super::validators;

#[derive(StructOpt)]
#[structopt(name = "clone", about = "clone a remote repository")]
pub struct SubcommandClone {
    #[structopt(validator = validators::is_url_or_ssh_repo)]
    repo: String,
    dir: Option<String>,
}

impl SubcommandClone {
    pub fn execute(&self) -> Result<()> {
        let (mut client, dir): (Box<dyn GitClient>, _) = match self.repo.parse::<Url>() {
            Ok(uri) => {
                // TODO: There has to be a better way to do this.
                let dir = self.dir.clone().unwrap_or_else(|| {
                    Path::new(uri.path())
                        .components()
                        .last()
                        .unwrap() // path is weird
                        .as_os_str()
                        .to_owned()
                        .into_string()
                        .unwrap() // path should be unicode
                        .split('.')
                        .next()
                        .unwrap() // path doesn't end in .git
                        .to_owned()
                });
                let mut repo = self.repo.to_owned();
                if !repo.ends_with(".git") {
                    repo.push_str(".git");
                }
                if !repo.ends_with('/') {
                    repo.push('/');
                }
                (Box::new(GitHttpClient::new(&repo)), dir)
            },
            Err(_) => {
                let client = GitTcpClient::connect(&self.repo, "127.0.0.1", 9418)?;
                let dir = self.dir.clone().unwrap_or_else(|| self.repo.clone());
                (Box::new(client), dir)
            }
        };
        println!("Cloning into \"{}\"...", dir);

        let refs = client.discover_refs()?;
        let packfile_data = client.fetch_packfile(&refs)?;

        let repo = Repo::from_packfile(&dir, &packfile_data)?;

        refs::create_refs(&dir, &refs)?;
        refs::update_head(&dir, &refs)?;

        // Checkout head and format refs
        repo.checkout_head()?;
        Ok(())
    }
}

//pub fn clone_ssh_priv(host: &str, user: &str, repo: &str) -> IoResult<()> {
//    let dir = repo.split(".")
//        .next().unwrap()
//        .to_owned();
//    let full_repo = [user, "/", repo].join("");
//    let mut client = GitSSHClient::new(host, &full_repo);
//
//    println!("Cloning into \"{}\"...", dir);
//
//    let refs = client.discover_refs()?;
//    let packfile_data = client.fetch_packfile(&refs)?;
//
//    let repo = Repo::from_packfile(&dir, &packfile_data)?;
//
//    refs::create_refs(&dir, &refs)?;
//    refs::update_head(&dir, &refs)?;
//
//    // Checkout head and format refs
//    repo.checkout_head()?;
//    Ok(())
//}

// #[cfg(test)]
// mod tests {
//     use std::fs;
//     use super::*;
//     use std::io;
//
//     #[test]
//     fn test_clone() {
//         let dir = "tests/clone-test".to_owned();
//         if let Err(error) = fs::remove_dir_all(&dir) {
//             if error.kind() != io::ErrorKind::NotFound {
//                 panic!("Error removing test-clone directory: {}", error);
//             }
//         }
//         let params = Params {
//             repo: "https://github.com/cwbriones/rgit.git".to_owned(),
//             dir: Some(dir),
//         };
//         execute(params).unwrap()
//     }
// }
