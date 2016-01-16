use reader::MyReaderExt;

use std::io;
use std::result::Result::Err;
use std::net::TcpStream;
use std::str;

///
/// Reads and parses packet-lines from the given connection
/// until a null packet is received.
///
pub fn receive(socket: &mut TcpStream) -> io::Result<Vec<String>> {
    let mut lines = vec![];
    loop {
        match read_packet_line(socket) {
            Ok(Some(line)) => {
                let s: String = str::from_utf8(&line[..]).unwrap().to_string();
                lines.push(s)
            }
            Ok(None)       => return Ok(lines),
            Err(e)         => return Err(e)
        }
    }
}

///
/// Helper for opening a `TcpStream` connection and performing an action with the socket.
///
pub fn with_connection<T, K>(host: &str, port: u16, consumer: K) -> io::Result<T>
    where K: Fn(&mut TcpStream) -> io::Result<T>
{
    let mut sock = try!(TcpStream::connect(&(host, port)));
    consumer(&mut sock)
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
pub fn receive_with_sideband(socket: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut packfile_data = Vec::new();
    loop {
        match try!(read_packet_line(socket)) {
            Some(line) => {
                if &line[..] == "NAK\n".as_bytes() {
                    continue;
                }
                match line[0] {
                    1 => {
                        // TODO: This only uses a loop because Vec::push_all was
                        // not stabilized for Rust 1.0
                        for i in &line[1..] {
                            packfile_data.push(*i)
                        }
                    }
                    2 => {
                        let msg = str::from_utf8(&line[1..]).unwrap();
                        print!("{}", msg);
                    }
                    _ => return Err(io::Error::new(io::ErrorKind::Other, "Git server returned error"))
                }
            }
            None => return Ok(packfile_data)
        }
    }
}

///
/// Reads and parses a packet-line from the server.
///
fn read_packet_line(socket: &mut TcpStream) -> io::Result<Option<Vec<u8>>> {
    let header = try!(socket.read_exact(4u64));
    let length_str = str::from_utf8(&header[..]).unwrap();
    let length = u64::from_str_radix(length_str, 16).unwrap();

    if length > 4 {
        let pkt = try!(socket.read_exact(length - 4));
        Ok(Some(pkt))
    } else {
        Ok(None)
    }
}

