extern crate serde_json;

use bytes::{BufMut, BytesMut};
use mio::tcp::TcpStream;
use serde_json::Value;
use std::cmp::max;
use std::io;
use std::io::Read;

pub struct Channel {
    pub socket: TcpStream,
    in_buffer: BytesMut,
}

impl Channel {
    pub fn new(socket: TcpStream) -> Channel {
        Channel {
            socket: socket,
            in_buffer: BytesMut::with_capacity(2048),
        }
    }
    pub fn read(self: &mut Channel) -> io::Result<Vec<Value>> {
        let buffer = &mut self.in_buffer;
        loop {
            if buffer.remaining_mut() < 128 {
                let cap = max(2048, buffer.capacity() * 2) - buffer.len();
                println!("Capacity down to {}, resizing by {}", buffer.capacity(), cap);
                buffer.reserve(cap);
            }
            match unsafe {
                self.socket.read(buffer.bytes_mut())
            } {
                Ok(0) => {
                    break;
                }
                Ok(count) => unsafe {
                    buffer.advance_mut(count);
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // Socket is not ready anymore, stop reading
                    break;
                }
                e => panic!("err={:?}", e), // Unexpected error
            }
        }
        if buffer.is_empty() {
            println!("Socket closed!");
            Err(io::Error::new(io::ErrorKind::ConnectionReset, "Socket closed"))
        } else {
            let mut result: Vec<Value> = Vec::new();
            let mut total_len = 0;
            for piece in buffer.split(|b| *b == b'\n') {
                let new_total = total_len + piece.len() + 1;
                if new_total > buffer.len() {
                    break;
                }
                if piece.len() == 0 {
                    continue;
                }
                match serde_json::from_slice(piece) {
                    Ok(r) => {
                        result.push(r)
                    },
                    Err(f) => {
                        println!("Error parsing json: \"{}\" {}",
                                 String::from_utf8_lossy(piece), f.to_string())
                    },
                }
                total_len = new_total;
            }
            buffer.advance(total_len);
            Ok(result)
        }
    }
}
