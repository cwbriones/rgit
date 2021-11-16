use std::io::Read;

use anyhow::anyhow;
use anyhow::Result;
use reqwest::blocking::Client;
use reqwest::redirect;
use reqwest::IntoUrl;
use reqwest::Url;

use super::GitClient;
use crate::packfile::refs::GitRef;

pub struct GitHttpClient {
    url: Url,
    client: Client,
}

const REF_DISCOVERY_ENDPOINT: &str = "info/refs";
const UPLOAD_PACK_ENDPOINT: &str = "git-upload-pack";

impl GitHttpClient {
    pub fn new<U>(u: U) -> Result<Self>
    where
        U: IntoUrl,
    {
        let mut url = u.into_url()?;
        // Normalize the path, so that our endpoint joins later honor the correct
        // semantics of url.join
        if !url.path().ends_with('/') {
            let mut path = url.path().to_string();
            path.push('/');
            url.set_path(&path);
        }
        // TODO: I think the initial redirect is consuming the http post body when
        // the caller specifies a clone with http
        let client = Client::builder()
            .redirect(redirect::Policy::limited(3))
            .build()?;
        Ok(GitHttpClient { url, client })
    }
}

impl GitClient for GitHttpClient {
    fn discover_refs(&mut self) -> Result<Vec<GitRef>> {
        let mut discovery_url = self.url.join(REF_DISCOVERY_ENDPOINT)?;
        discovery_url.set_query(Some("service=git-upload-pack"));

        let mut res = self.client.get(discovery_url).send()?;
        if !res.status().is_success() {
            return Err(anyhow!("server responded {}", res.status()));
        }
        // The server first sends a header to verify the service is correct
        let mut line = Vec::new();
        super::read_packet_line(&mut res, &mut line)?;
        if line != b"# service=git-upload-pack\n" {
            return Err(anyhow!("expected git-upload-pack header in response"));
        }

        // The server then sends a flush packet "0000"
        let mut flush = [0; 4];
        res.read_exact(&mut flush)?;
        assert_eq!(&flush, b"0000");

        let decoded = super::receive(&mut res)?;
        let (_server_capabilities, refs) = super::parse_lines(&decoded)?;

        Ok(refs)
    }

    fn fetch_packfile(&mut self, want: &[GitRef]) -> Result<Vec<u8>> {
        let capabilities = ["multi_ack_detailed", "side-band-64k", "agent=git/1.8.1"];
        let body = super::create_negotiation_request(&capabilities, want);
        let pack_endpoint = self.url.join(UPLOAD_PACK_ENDPOINT)?;

        let mut res = self.client.post(pack_endpoint).body(body).send()?;
        if !res.status().is_success() {
            return Err(anyhow!("server responded {}", res.status()));
        }
        super::receive_with_sideband(&mut res)
    }
}
