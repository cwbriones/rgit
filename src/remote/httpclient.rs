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
    pub fn new<U: IntoUrl>(url: U) -> Self {
        GitHttpClient {
            url: url.into_url().unwrap(),
            client: Client::new(),
        }
    }
}

impl GitClient for GitHttpClient {

    fn discover_refs(&mut self) -> io::Result<Vec<GitRef>> {
        let discovery_url = [self.url.serialize(), REF_DISCOVERY_ENDPOINT.to_string()].join("");
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
        let pack_endpoint = [self.url.serialize(), UPLOAD_PACK_ENDPOINT.to_string()].join("");

        let mut res = self.client
            .post(&pack_endpoint)
            .body(&body)
            .send()
            .unwrap();

        let mut contents = Vec::new();
        try!(res.read_to_end(&mut contents));
        let mut reader = io::BufReader::new(&contents[..]);
        super::receive_with_sideband(&mut reader)
    }
}

