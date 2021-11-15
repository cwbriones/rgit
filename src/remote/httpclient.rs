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

const REF_DISCOVERY_ENDPOINT: &str = "info/refs?service=git-upload-pack";
const UPLOAD_PACK_ENDPOINT: &str = "git-upload-pack";

impl GitHttpClient {
    pub fn new<U>(u: U) -> Result<Self>
    where
        U: IntoUrl,
    {
        let mut url = u.into_url()?;
        // TODO: I think the initial redirect is consuming the http post body when
        // the caller specifies a clone with http
        if let Some("github.com") = url.domain() {
            url.set_scheme("https").expect("could not set scheme");
        }
        let client = Client::builder()
            .redirect(redirect::Policy::limited(1))
            .build()?;
        Ok(GitHttpClient { url, client })
    }
}

impl GitClient for GitHttpClient {
    fn discover_refs(&mut self) -> Result<Vec<GitRef>> {
        let mut discovery_url = self.url.join(REF_DISCOVERY_ENDPOINT)?;
        discovery_url.set_query(Some("service=git-upload-pack"));

        let mut res = self.client.get(discovery_url).send()?;

        // The server first sends a header to verify the service is correct
        match super::read_packet_line(&mut res)? {
            Some(v) if v == b"# service=git-upload-pack\n" => {}
            _ => return Err(anyhow!("expected git-upload-pack header in response")),
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

        super::receive_with_sideband(&mut res)
    }
}
