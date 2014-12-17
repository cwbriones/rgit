use std::{str, num};
use std::io::{IoResult, TcpStream};
use std::io::{IoError, OtherIoError};

// TODO:
// receive_fully
// send

/// Reads and parses packet-lines from the given connection
/// until a null packet is received.
pub fn receive(socket: &mut TcpStream) -> IoResult<Vec<String>> {
    let mut lines = vec![];
    loop {
        match read_packet_line(socket) {
            Ok(Some(line)) => lines.push(line),
            Ok(None)       => return Ok(lines),
            Err(e)         => return Err(e)
        }
    }
}

pub fn with_connection<T>(host: &str, port: u16, consumer: |&mut TcpStream| -> IoResult<T>) -> IoResult<T> {
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
                if line == "NAK\n" {
                    continue;
                }
                let bytes = line.into_bytes();
                match bytes.as_slice() {
                    [1, rest..] => packfile_data.push_all(bytes.slice_from(1)),
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
fn read_packet_line(socket: &mut TcpStream) -> IoResult<Option<String>> {
    let header = try!(socket.read_exact(4));
    let length_str = str::from_utf8(header.as_slice()).unwrap();
    let length: uint = num::from_str_radix(length_str, 16).unwrap();

    if length > 4 {
        let pkt = try!(socket.read_exact(length - 4));
        match String::from_utf8(pkt) {
            Ok(parsed) => Ok(Some(parsed)),
            Err(_) => Ok(None)
        }
    } else {
        Ok(None)
    }
}

