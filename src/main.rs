extern crate bytes;
extern crate encoding;
extern crate getopts;
extern crate mio;
extern crate serde;
extern crate serde_json;
extern crate slab;

use bytes::{BufMut, BytesMut};
use encoding::{Encoding, ByteWriter, EncoderTrap};
use encoding::types::RawEncoder;
use encoding::all::ASCII;
use getopts::Options;
use mio::*;
use mio::tcp::{TcpListener, TcpStream};
use serde_json::Value;
use slab::Slab;
use std::env;
use std::io;
use std::io::Read;
use std::io::Write;
use std::vec::Vec;


// hexadecimal numeric character reference replacement
fn hex_escape(_encoder: &mut RawEncoder, input: &str, output: &mut ByteWriter) -> bool {
    for escape in input.chars().map(|ch| format!("\\u{:04X}", ch as isize)) {
        output.write_bytes(escape.as_bytes());
    }
    true
}

static HEX_ESCAPE: EncoderTrap = EncoderTrap::Call(hex_escape);

pub struct Channel {
    socket: TcpStream,
    in_buffer: BytesMut,
}

impl Channel {
    pub fn new(socket: TcpStream) -> Channel {
        Channel {
            socket: socket,
            in_buffer: BytesMut::with_capacity(2048),
        }
    }
}

fn read(channel: &mut Channel) -> std::io::Result<Vec<String>> {
    let mut socket = channel.socket.try_clone()?;
    let buffer = &mut channel.in_buffer;
    loop {
        match unsafe {
            socket.read(buffer.bytes_mut())
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
        let mut result: Vec<String> = Vec::new();
        let mut total_len = 0;
        for piece in buffer.split(|b| *b == b'\n') {
            let new_total = total_len + piece.len() + 1;
            if new_total > buffer.len() {
                break;
            }
            result.push(String::from_utf8_lossy(piece.as_ref()).to_string());
            total_len = new_total;
        }
        buffer.advance(total_len);
        Ok(result)
    }
}

fn serve(router: &str, sock: &str, prefix: &str) -> std::io::Result<()> {
    let listener = TcpListener::bind(&sock.parse().expect("Wrong format for socket address"))?;
    let mut stream = TcpStream::connect(&router.parse().expect("Wrong format for router address"))?;
    stream.write(prefix.as_bytes())?;
    stream.write(b"\n")?;

    let poll = Poll::new()?;
    let mut channels = Slab::new();

    const SERVER: Token = Token(0);
    const ROUTER: Token = Token(1);

    channels.insert(Channel::new(stream.try_clone()?));

    poll.register(&listener, SERVER, Ready::readable(), PollOpt::level())?;
    poll.register(&stream, ROUTER, Ready::readable(), PollOpt::level())?;

    let mut events = Events::with_capacity(1024);

    loop {
        poll.poll(&mut events, None)?;

        for event in events.iter() {
            match event.token() {
                SERVER => {
                    let (client, addr) = listener.accept().expect("Accept success");
                    println!("Got connection from {}", addr);
                    let channel = Channel::new(client.try_clone()?);
                    let t: Token = Token(channels.insert(channel) + 1);
                    poll.register(&client, t, Ready::readable(), PollOpt::level())?;
                }
                ROUTER => {
                    match read(&mut channels[0]) {
                        Ok(strings) => {
                            for s in strings {
                                let mut message: Value = serde_json::from_str(&*s).unwrap();
                                let channel = message["to"]["channel"].clone();
                                let parts: Vec<&str> = channel.as_str().unwrap().splitn(3, ':').collect();
                                message["to"]["channel"] = Value::String(String::from(parts[2]));
                                let client = &mut channels[parts[1].parse::<usize>().unwrap()].socket;
                                serde_json::to_writer(client.try_clone()?, &message)?;
                                client.write(b"\n")?;
                            }
                        }
                        Err(f) => {
                            println!("Error from router: {}", f.to_string());
                            return Err(f);
                        }
                    }
                }
                Token(token) => {
                    println!("Got event with token {}", token);
                    let idx = token - 1;
                    match read(&mut channels[idx]) {
                        Ok(strings) => {
                            for s in strings {
                                let mut message: Value = serde_json::from_str(&*s).unwrap();
                                let new_from = format!("{}:{}:{}", prefix, idx, message["from"]["channel"].as_str().unwrap());
                                message["from"]["channel"] = Value::String(new_from);
                                let msg_str = format!("{}\n", serde_json::to_string(&message).unwrap());
                                let encoded = ASCII.encode(&msg_str, HEX_ESCAPE).unwrap();
                                stream.write(&encoded)?;
                            }
                        }
                        Err(_) => {
                            channels.remove(idx);
                        }
                    }
                }
            }
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut opts = Options::new();
    opts.reqopt("r", "router-socket", "router address", "HOST:PORT");
    opts.reqopt("s", "socket", "socket to listen to", "HOST:PORT");
    opts.optopt("p", "prefix", "router prefix, default \"tcp\"", "PREFIX");
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => {
            println!("{}", f.to_string());
            std::process::exit(-1);
        }
    };
    match serve(&matches.opt_str("r").unwrap(),
                &matches.opt_str("s").unwrap(),
                &matches.opt_str("p").unwrap_or(String::from("tcp"))) {
        Ok(m) => m,
        Err(f) => {
            println!("Serve exited: {}", f.to_string());
            std::process::exit(-1);
        }
    }
}
