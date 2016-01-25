use std::io::Result as IoResult;

use remote::GitClient;
use remote::httpclient::GitHttpClient;
use remote::tcpclient::GitTcpClient;
use packfile::refs;
use packfile::refs::GitRef;
use store::Repo;
use hyper::Url;

pub fn clone_priv(repo: &str, maybe_dir: Option<String>) -> IoResult<()> {
    let (mut client, dir): (Box<GitClient>, _) = match Url::parse(repo) {
        Ok(url) => {
            // TODO: There has to be a better way to do this.
            let dir = maybe_dir.unwrap_or_else(|| {
                url.path().unwrap()
                    .last().unwrap()
                    .split(".")
                    .next().unwrap()
                    .to_string()
            });
            (Box::new(GitHttpClient::new(repo)), dir)
        },
        Err(_) => {
            let dir = maybe_dir.unwrap_or(repo.to_string());
            let client = try!(GitTcpClient::connect(repo, "127.0.0.1", 9418));
            (Box::new(client), dir)
        }
    };
    println!("Cloning into \"{}\"...", dir);

    let refs = try!(client.discover_refs());
    let packfile_data = try!(client.fetch_packfile(&refs));

    let repo = try!(Repo::from_packfile(&dir, &packfile_data));

    try!(refs::create_refs(&dir, &refs));
    try!(refs::update_head(&dir, &refs));

    // Checkout head and format refs
    try!(repo.checkout_head());
    Ok(())
}

///
/// Lists remote refs available in the given repo.
///
pub fn ls_remote(repo: &str) -> IoResult<()> {
    let mut client = GitHttpClient::new(repo);
    client.discover_refs().map(|pktlines| {
        print_packetlines(&pktlines);
    })
}

fn print_packetlines(pktlines: &[GitRef]) {
    for p in pktlines.iter() {
        let &GitRef{ref id, ref name} = p;
        print!("{}\t{}\n", id, name);
    }
}
