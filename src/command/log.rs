use std::io::Result as IoResult;
use structopt::StructOpt;

use crate::store::Repo;

#[derive(StructOpt)]
#[structopt(name = "log", about = "show commit logs")]
pub struct SubcommandLog {
    revision: Option<String>,
}

impl SubcommandLog {
    pub fn execute(&self) -> IoResult<()> {
        let repo = Repo::from_enclosing()?;
        let rev = self.revision.clone().unwrap_or("HEAD".into());
        // Refactor this into a commit walker and pass a closure that calls
        // std::process::Command::new("less") to pipe it
        repo.log(&rev)?;
        Ok(())
    }
}
