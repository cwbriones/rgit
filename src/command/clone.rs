use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use reqwest::Url;
use structopt::StructOpt;

use crate::packfile::refs;
use crate::remote::httpclient::GitHttpClient;
use crate::remote::GitClient;
use crate::store::Repo;

fn parse_git_url<'a>(input: &'a str) -> Result<Url> {
    use nom::Finish;
    if let Ok(u) = input.parse::<Url>() {
        return Ok(u);
    }
    parse_scp_url(input)
        .finish()
        .map(|(_, url)| url)
        .map_err(|e| {
            // We need to map the output error, since by default nom
            // will tie it to the lifetime of the input string.
            anyhow::Error::from(nom::error::Error {
                input: e.input.to_string(),
                code: e.code,
            })
        })
}

fn parse_scp_url<'a>(input: &'a str) -> nom::IResult<&'a str, Url> {
    use nom::bytes::complete as bytes;
    use nom::character::complete as character;
    use nom::combinator::{
        map_res,
        rest,
    };
    use nom::sequence as seq;

    // Example: git@github.com:cwbriones/rgit
    let parts = seq::tuple((
        seq::terminated(bytes::take_until("@"), character::char('@')),
        seq::terminated(bytes::take_until(":"), character::char(':')),
        rest,
    ));
    map_res(parts, |(user, domain, path)| {
        let normalized = format!("ssh://{}@{}/{}", user, domain, path);
        Url::parse(&normalized)
    })(input)
}

#[derive(StructOpt)]
#[structopt(name = "clone", about = "clone a remote repository")]
pub struct SubcommandClone {
    #[structopt(parse(try_from_str = parse_git_url))]
    remote_url: Url,
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
                let path = Path::new(self.remote_url.path());
                path.with_extension("")
                    .file_name()
                    .map(|p| Path::new(p).to_owned())
            })
            .ok_or_else(|| anyhow!("could not infer repo directory from url"))?;
        // Specific client should normalize, not here.
        let http_client =
            GitHttpClient::new(self.remote_url.clone()).with_context(|| "create http client")?;

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
