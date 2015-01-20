extern crate libc;
extern crate "miniz-sys" as miniz;

use std::old_io::File;
use std::mem;

use self::miniz::{MZ_OK, MZ_STREAM_END, MZ_DEFAULT_WINDOW_BITS};

type git_zstream = miniz::mz_stream;

struct ZlibReader {
    /* We always read in 4kB chunks. */
    buffer: [u8; 4096],
    offset: usize,
    len: usize,
    off_t: isize,
    consumed_bytes: usize,
}
// static git_SHA_CTX ctx;

impl ZlibReader {
    fn new() -> ZlibReader {
        ZlibReader {
            buffer: [0; 4096],
            offset: 0,
            len: 0,
            off_t: 0,
            consumed_bytes: 0,
        }
    }

    // Check that at least min bytes are filled in the buffer
    fn fill(&mut self, min: uint) -> *const u8 {
        0 as *const u8
    }

    // Marked bytes read from the file as used,
    // otherwise we need to rewind the filestream
    fn use_bytes(&mut self, bytes: usize) {
        if bytes > self.len {
            panic!("used more bytes than were available");
        }
        self.len -= bytes;
        self.offset += bytes;

        // TODO: Make sure off_t is sufficiently large not to wrap
        self.consumed_bytes += bytes;
    }

    pub fn get_data(&mut self, file: &mut File, size: usize) -> Vec<u8> {
        // ==================================================
        // Create the stream object and allocate the buffer
        // ==================================================
        // git_zstream stream;
        // void *buf = xmallocz(size);
        // memset(&stream, 0, sizeof(stream));

        let mut vbuf = Vec::with_capacity(size);
        let mut stream: miniz::mz_stream = unsafe { mem::zeroed() };

        // ==================================================
        // Initialize the stream
        // ==================================================

        let mut buf = vbuf.as_mut_ptr();
        stream.next_out = buf;
        stream.avail_out = size as libc::c_uint;

        stream.next_in = self.fill(1);
        stream.avail_in = self.len as u32;

        unsafe { miniz::mz_inflateInit2(&mut stream as *mut git_zstream, MZ_DEFAULT_WINDOW_BITS) };

        // ==================================================
        // Deflate loop
        // ==================================================

        loop {
            let ret = unsafe { miniz::mz_inflate(&mut stream as *mut git_zstream, 0) };
            let bytes_to_use = self.len - stream.avail_in as usize;
            self.use_bytes(bytes_to_use);

            if (stream.total_out == size as u64 && ret == MZ_STREAM_END) {
                break;
            }
            if ret != MZ_OK {
                drop(buf);
                panic!("inflate returned {}", ret);
        // 		if (!recover)
        // 			exit(1);
        // 		has_errors = 1;
        // 		break;
            }
            // ==================================================
            // Prepare for next iteration, there's more data
            // ==================================================
            stream.next_in = self.fill(1);
            stream.avail_in = self.len as u32;
        }

        unsafe { miniz::mz_deflateEnd(&mut stream as *mut git_zstream) };
        vbuf
    }
}

// How the unpacking goes in the git source:
// read the file into the buffer using fill where:
//     buffer - actual data read in
//     len - number of bytes in the buffer
//
// Initialize the values used in inflation:
//     buf - the array where we'll store the object, it has a fixed
//     size since we know the number of bytes before we read
//     stream - the stream object we need to use the library
//
//     stream.next_out - where the data will be inflated into
//     stream.size - the size of buf
//     stream.next_in - where to pull the data from to inflate,
//     we use fill to guarantee n bytes and return the filled buffer
//     stream.avail_in - the number of bytes available as input
//
//     initialize the stream object with git_inflate_init
//
//     loop:
//     -inflate into the stream
//     -check how much we read and mark that amount of bytes as used
//         -check if we've read size number of bytes successfully, if not
//         we handle the error
//         -otherwise we break
//     - refill the buffer and reset the number of bytes available
