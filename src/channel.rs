extern crate serde_json;

use encoding::{Encoding, ByteWriter, EncoderTrap};
use encoding::types::RawEncoder;
use encoding::all::ASCII;
use mio::tcp::TcpStream;
use serde_json::Value;
use std::io;
use std::io::{BufRead, Write};

fn hex_escape(_encoder: &mut RawEncoder, input: &str, output: &mut ByteWriter) -> bool {
    for escape in input.chars().map(|ch| format!("\\u{:04X}", ch as isize)) {
        output.write_bytes(escape.as_bytes());
    }
    true
}

static HEX_ESCAPE: EncoderTrap = EncoderTrap::Call(hex_escape);


pub struct Channel {
    reader: io::BufReader<TcpStream>,
    writer: TcpStream,
}

impl Channel {
    pub fn new(socket: TcpStream) -> Channel {
        Channel {
            reader: io::BufReader::new(socket.try_clone().unwrap()),
            writer: socket,
        }
    }

    pub fn read(self: &mut Channel) -> io::Result<Vec<Value>> {
        let mut result: Vec<Value> = Vec::new();
        loop {
            let mut s = String::new();
            match self.reader.read_line(&mut s) {
                Ok(_) => {
                    match serde_json::from_str(&s) {
                        Ok(r) => {
                            result.push(r);
                        },
                        Err(f) => {
                            println!("Error parsing json: {}", f.to_string());
                            break;
                        },
                    }
                },
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    // Socket is not ready anymore, stop reading
                    break;
                },
                Err(f) => {
                    println!("Error reading a line: {}", f.to_string());
                    break;
                }
            }
        }
        if result.is_empty() {
            println!("Socket closed!");
            Err(io::Error::new(io::ErrorKind::ConnectionReset, "Socket closed"))
        } else {
            Ok(result)
        }
    }

    pub fn write(self: &mut Channel, message: Value) -> io::Result<()> {
        let msg_str = format!("{}\n", serde_json::to_string(&message)?);
        let encoded = ASCII.encode(&msg_str, HEX_ESCAPE).expect("encoding to ASCII will not fail");
        self.writer.write(&encoded)?;
        self.writer.flush()?;
        Ok(())
    }
}
