use std::io;

use remote::GitClient;
use remote::httpclient::GitHttpClient;
use packfile::refs;
use packfile::refs::GitRef;
use store::Repo;

pub fn clone_priv(repo: &str, dir: &str) -> io::Result<()> {
    println!("Cloning into \"{}\"...", dir);

    let mut client = GitHttpClient::new(repo);
    let refs = try!(client.discover_refs());
    let packfile_data = try!(client.fetch_packfile(&refs));

    let repo = try!(Repo::from_packfile(dir, &packfile_data));

    try!(refs::create_refs(dir, &refs));
    try!(refs::update_head(dir, &refs));

    // Checkout head and format refs
    try!(repo.checkout_head());
    Ok(())
}

///
/// Lists remote refs available in the given repo.
///
pub fn ls_remote(host: &str, port: u16, repo: &str) -> i32 {
    //let mut client = GitTcpClient::connect(repo, host, port).unwrap();
    let mut client = GitHttpClient::new(repo);
    match client.discover_refs() {
        Ok(pktlines) => {
            print_packetlines(&pktlines);
            0
        },
        Err(_) => -1
    }
}

fn print_packetlines(pktlines: &[GitRef]) {
    for p in pktlines.iter() {
        let &GitRef{ref id, ref name} = p;
        print!("{}\t{}\n", id, name);
    }
}
