#![feature(core)]
#![feature(collections)]

use std::{str, num};
use std::old_io::{IoResult, TcpStream};
use std::old_io::{IoError, OtherIoError};

// TODO:
// receive_fully
// send

/// Reads and parses packet-lines from the given connection
/// until a null packet is received.
pub fn receive(socket: &mut TcpStream) -> IoResult<Vec<String>> {
    let mut lines = vec![];
    loop {
        match read_packet_line(socket) {
            Ok(Some(line)) => {
                let s: String = str::from_utf8(line.as_slice()).unwrap().to_string();
                lines.push(s)
            }
            Ok(None)       => return Ok(lines),
            Err(e)         => return Err(e)
        }
    }
}

pub fn with_connection<T, K>(host: &str, port: u16, consumer: K) -> IoResult<T>
    where K: Fn(&mut TcpStream) -> IoResult<T>
{
    let mut sock = try!(TcpStream::connect((host, port)));
    consumer(&mut sock)
}

/// Receives a multiplexed response from the git server.
/// The mulitplexing protocol encodes channel information as the first
/// byte returned with each reponse packetline,
/// There are three channels:
///    1. Packetfile data
///    2. Progress information to be printed to STDERR
///    3. Error message from server, abort operation
pub fn receive_with_sideband(socket: &mut TcpStream) -> IoResult<Vec<u8>> {
    let mut packfile_data = Vec::new();
    loop {
        match try!(read_packet_line(socket)) {
            Some(line) => {
                if line.as_slice() == "NAK\n".as_bytes() {
                    continue;
                }
                match line.as_slice() {
                    [1, rest..] => {
                        packfile_data.push_all(line.slice_from(1))
                    }
                    [2, msg_bytes..] => {
                        let msg = str::from_utf8(msg_bytes).unwrap();
                        print!("{}", msg);
                    }
                    stuff => {
                        return Err(IoError {
                            kind: OtherIoError,
                            desc: "Git server returned error",
                            detail: None,
                        })
                    }
                }
            }
            None => return Ok(packfile_data)
        }
    }
    Ok(packfile_data)
}

/// Reads and parses a packet-line from the server.
fn read_packet_line(socket: &mut TcpStream) -> IoResult<Option<Vec<u8>>> {
    let header = try!(socket.read_exact(4));
    let length_str = str::from_utf8(header.as_slice()).unwrap();
    let length: usize = num::from_str_radix(length_str, 16).unwrap();

    if length > 4 {
        let pkt = try!(socket.read_exact(length - 4));
        Ok(Some(pkt))
    } else {
        Ok(None)
    }
}

