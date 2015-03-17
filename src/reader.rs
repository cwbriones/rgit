use std::io;
use std::io::Read;

pub trait MyReaderExt {
    fn read_byte(&mut self) -> io::Result<u8>;
    fn read_exact(&mut self, u64) -> io::Result<Vec<u8>>;
    fn read_be_u32(&mut self) -> io::Result<u32>;
}

impl<R:Read> MyReaderExt for R {

    fn read_byte(&mut self) -> io::Result<u8> {
        try!(self.bytes().next().ok_or(io::Error::new(io::ErrorKind::Other, "EOF", None)))
    }

    fn read_exact(&mut self, n: u64) -> io::Result<Vec<u8>> {
        let mut buf = vec![];
        try!(io::copy(&mut self.take(n), &mut buf));
        Ok(buf)
    }

    fn read_be_u32(&mut self) -> io::Result<u32> {
        let mut buf = [0; 4];
        match self.read(&mut buf) {
          Ok(s) if s == 4 => {
              let mut result = 0u32;

              // This is because I already know my system is be
              for i in buf.iter() {
                result = result << 8;
                result += *i as u32;
              }
              Ok(result)
          },
          _ => panic!("error read_be_32")
        }
    }
}

