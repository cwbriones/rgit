use std::io;
use std::io::Read;

use hyper::Url;
use hyper::client::{IntoUrl,Client};

use super::GitClient;
use packfile::refs::GitRef;

pub struct GitHttpClient {
    url: Url,
    client: Client
}

const REF_DISCOVERY_ENDPOINT: &'static str = "/info/refs?service=git-upload-pack";
const UPLOAD_PACK_ENDPOINT: &'static str = "/git-upload-pack";

impl GitHttpClient {
    pub fn new<U: IntoUrl>(u: U) -> Self {
        let url = u.into_url().unwrap();
        GitHttpClient {
            url: follow_url(url),
            client: Client::new(),
        }
    }
}

impl GitClient for GitHttpClient {

    fn discover_refs(&mut self) -> io::Result<Vec<GitRef>> {
        let discovery_url = [self.url.as_str(), REF_DISCOVERY_ENDPOINT].join("");
        let mut res = self.client.get(&discovery_url).send().unwrap();

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
        let pack_endpoint = [self.url.as_str(), UPLOAD_PACK_ENDPOINT].join("");

        let res = self.client
            .post(&pack_endpoint)
            .body(&body)
            .send()
            .unwrap();

        let mut reader = io::BufReader::with_capacity(16 * 1024, res);
        super::receive_with_sideband(&mut reader)
    }
}

///
/// Follows the given url to what is most likely the actual repository.
///
/// Hyper follows redirects by default but we have no way of extracting the actual
/// url and checking that it is fully qualified, since some services (e.g. GitHub)
/// don't return a redirect when making a packfile request to the wrong URL.
///
fn follow_url(url: Url) -> Url {
    url
}
