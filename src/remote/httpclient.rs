use std::io;
use std::io::Read;

use reqwest::Url;
use reqwest::IntoUrl;
use reqwest::blocking::Client;
use reqwest::redirect;

use super::GitClient;
use packfile::refs::GitRef;

pub struct GitHttpClient {
    url: Url,
    client: Client,
}

const REF_DISCOVERY_ENDPOINT: &'static str = "info/refs?service=git-upload-pack";
const UPLOAD_PACK_ENDPOINT: &'static str = "git-upload-pack";

impl GitHttpClient {
    pub fn new<U>(u: U) -> Self
        where U: IntoUrl,
    {
        let mut url = u.into_url().unwrap();
        // TODO: I think the initial redirect is consuming the http post body when
        // the caller specifies a clone with http
        if let Some("github.com") = url.domain() {
            url.set_scheme("https").unwrap();
        }
        let client = Client::builder()
            .redirect(redirect::Policy::limited(1))
            .build()
            .unwrap();
        GitHttpClient {
            url: url,
            client: client,
        }
    }
}

impl GitClient for GitHttpClient {

    fn discover_refs(&mut self) -> io::Result<Vec<GitRef>> {
        let mut discovery_url = self.url.join(REF_DISCOVERY_ENDPOINT).unwrap();
        discovery_url.set_query(Some("service=git-upload-pack"));

        let mut res = self.client.get(discovery_url)
            .send()
            .unwrap();

        // The server first sends a header to verify the service is correct
        let first = try!(super::read_packet_line(&mut res)).unwrap();
        assert_eq!(first, b"# service=git-upload-pack\n");

        // The server then sends a flush packet "0000"
        let mut flush = [0; 4];
        try!(res.read_exact(&mut flush));
        assert_eq!(&flush, b"0000");

        let decoded = try!(super::receive(&mut res));
        let (_server_capabilities, refs) = super::parse_lines(&decoded);

        Ok(refs)
    }

    fn fetch_packfile(&mut self, want: &[GitRef]) -> io::Result<Vec<u8>> {
        let capabilities = ["multi_ack_detailed", "side-band-64k", "agent=git/1.8.1"];
        let body = super::create_negotiation_request(&capabilities, want);
        let pack_endpoint = self.url.join(UPLOAD_PACK_ENDPOINT).unwrap();

        let mut res = self.client.post(pack_endpoint)
            .body(body)
            .send()
            .unwrap();

        super::receive_with_sideband(&mut res)
    }
}
