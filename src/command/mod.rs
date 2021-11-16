use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use reqwest::Url;

use crate::remote::httpclient::GitHttpClient;
use crate::remote::sshclient::GitSSHClient;
use crate::remote::tcpclient::GitTcpClient;
use crate::remote::GitClient;

pub mod clone;
pub mod log;
pub mod ls_remote;
pub mod test_delta;

fn parse_git_url(input: &str) -> Result<Url> {
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

fn parse_scp_url(input: &str) -> nom::IResult<&str, Url> {
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

fn create_client(remote_url: &Url) -> Result<Box<dyn GitClient>> {
    match remote_url.scheme() {
        "ssh" => {
            let host = remote_url
                .host_str()
                .ok_or_else(|| anyhow!("host required for ssh"))?;
            let path = remote_url.path();
            let client = GitSSHClient::new(host, path).with_context(|| "create ssh client")?;
            Ok(Box::new(client))
        }
        "http" | "https" => {
            let client =
                GitHttpClient::new(remote_url.clone()).with_context(|| "create http client")?;
            Ok(Box::new(client))
        }
        "git" => {
            let host = remote_url
                .host_str()
                .ok_or_else(|| anyhow!("host required for ssh"))?;
            let path = remote_url.path();
            let client = GitTcpClient::connect(host, path)?;
            Ok(Box::new(client))
        }
        scheme => Err(anyhow!("unsupported url scheme: {}", scheme)),
    }
}
