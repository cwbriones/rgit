extern crate libc;
extern crate "miniz-sys" as miniz;

use std::io::File;
use miniz::{MZ_OK, MZ_STREAM_END, MZ_DEFAULT_WINDOW_BITS};

/* We always read in 4kB chunks. */
static buffer: [u8, ..4096] = [0, ..4096];
// static unsigned int offset, len;
// static off_t consumed_bytes;
// static git_SHA_CTX ctx;

type git_zstream = miniz::mz_stream;

// Check that at least min bytes are filled in the buffer
pub fn fill(file: &mut File, min: uint) {
}

// Marked bytes read from the file as used,
// otherwise we need to rewind the filestream
fn use_bytes(bytes: uint) {
    if bytes > len {
        fail!("used more bytes than were available");
    }
    len -= bytes;
    offset += bytes;

    // TODO: Make sure off_t is sufficiently large not to wrap
    consumed_bytes += bytes;
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

pub fn get_data(file: &mut File, size: uint) -> Vec<u8> {
    // ==================================================
    // Create the stream object and allocate the buffer
    // ==================================================
	// git_zstream stream;
	// void *buf = xmallocz(size);
	// memset(&stream, 0, sizeof(stream));
    let mut vbuf = Vec::with_capacity(size);
    let mut stream: miniz::mz_stream = unsafe { std::mem::zeroed() };
    // ==================================================
    // Initialize the stream
    // ==================================================
    let mut buf = vbuf.as_mut_ptr();
    stream.next_out = buf;
    stream.avail_out = size as libc::c_uint;

    stream.next_in = fill(1);
    stream.avail_in = len;

    miniz::mz_inflateInit2(&mut mz_stream as *mut mz_stream, MZ_DEFAULT_WINDOW_BITS);
    // ==================================================
    // Deflate loop
    // ==================================================

    loop {
        let ret = mz_inflate(&mut stream as *mut stream, 0);
        use_bytes(len - stream.avail_in);

        if (stream.total_out == size && ret == STREAM_END) {
            break;
        }
        if ret != Z_OK {
            drop(buf);
            fail!("inflate returned {}", ret);
	// 		if (!recover)
	// 			exit(1);
	// 		has_errors = 1;
	// 		break;
        }
        // ==================================================
        // Prepare for next iteration, there's more data
        // ==================================================
        // 	stream.next_in = fill(1);
        // 	stream.avail_in = len;
    }

	mz_deflate_end(&mut stream as *mut stream);
    Some(vbuf)
}
