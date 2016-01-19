use std::io;
use std::io::{Read,Write};
use std::str;
use std::fs::File;

use hyper::Url;
use hyper::client;
use hyper::client::{IntoUrl,Client};
use hyper::header::Headers;

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
        use std::str;
        let discovery_url = [self.url.serialize(), REF_DISCOVERY_ENDPOINT.to_string()].join("");
        let mut res = self.client.get(&discovery_url).send().unwrap();
        let mut contents = String::new();
        try!(res.read_to_string(&mut contents));
        let lines: Vec<_> = contents
            .split("\n")
            .map(|s| s.to_string()).collect();
        let refs = parse_lines(&lines[1..(lines.len() - 1)]);
        Ok(refs)
    }

    fn fetch_packfile(&mut self, want: &[GitRef]) -> io::Result<Vec<u8>> {
        let capabilities = ["multi_ack_detailed", "side-band-64k", "agent=git/1.8.1"];
        let body = super::create_negotiation_request(&capabilities, want);
        let mut req = try!(File::create("request"));
        try!(req.write_all(body.as_bytes()));
        let pack_endpoint = [self.url.serialize(), UPLOAD_PACK_ENDPOINT.to_string()].join("");
        //let mut headers = Headers::new();
        //headers.set_raw("Content-Type", vec![b"application/x-git-upload-pack-request".to_vec()]);
        //print!("POST to {} with:\n{}", pack_endpoint, body);
        let mut res = self.client
            .post(&pack_endpoint)
            .body(&body)
            //.headers(headers)
            .send()
            .unwrap();
        //println!("{:?}", res);
        let nak = try!(read_packet_line(&mut res));
        assert_eq!(nak.unwrap(), b"NAK\n");
        let mut contents = Vec::new();
        try!(res.read_to_end(&mut contents));
        //println!("Got file with length {}", contents.len());
        let mut reader = io::BufReader::new(&contents[..]);
        receive_with_sideband(&mut reader)
        //Ok(contents)
    }
}

// A better way to do this is to read from the serve into a vector array and then read from that
// and parse like we would the other server
fn parse_lines(lines: &[String]) -> Vec<GitRef> {
    let first_line: Vec<_> = lines[0].split("\0").collect();
    let _capabilities = first_line[1];

    let mut parsed = Vec::with_capacity(lines.len());
    let first_ref = super::parse_line(&first_line[0][8..]);
    parsed.push(first_ref);
    parsed.extend(lines.iter().skip(1).map(|l| super::parse_line(&l[4..])));
    parsed
}

///
/// Reads and parses a packet-line from the server.
///
fn read_packet_line<R: Read>(res: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut header = [0; 4];
    try!(res.read_exact(&mut header));
    let length_str = str::from_utf8(&header[..]).unwrap();
    let length = u64::from_str_radix(length_str, 16).unwrap();

    if length > 4 {
        let mut pkt = vec![0; (length - 4) as usize];
        try!(res.read_exact(&mut pkt));
        Ok(Some(pkt))
    } else {
        Ok(None)
    }
}

///
/// Receives a multiplexed response from the git server.
/// The mulitplexing protocol encodes channel information as the first
/// byte returned with each reponse packetline.
///
/// There are three channels:
///    1. Packetfile data
///    2. Progress information to be printed to STDERR
///    3. Error message from server, abort operation
///
pub fn receive_with_sideband<R: Read>(res: &mut R) -> io::Result<Vec<u8>> {
    let mut packfile_data = Vec::new();
    loop {
        match try!(read_packet_line(res)) {
            Some(line) => {
                if &line[..] == "NAK\n".as_bytes() {
                    continue;
                }
                match line[0] {
                    1 => {
                        // TODO: This only uses a loop because Vec::push_all was
                        // not stabilized for rust 1.0
                        for i in &line[1..] {
                            packfile_data.push(*i)
                        }
                    }
                    2 => {
                        let msg = str::from_utf8(&line[1..]).unwrap();
                        print!("{}", msg);
                    }
                    _ => return Err(io::Error::new(io::ErrorKind::Other, "git server returned error"))
                }
            }
            None => return Ok(packfile_data)
        }
    }
}
