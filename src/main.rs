extern crate bytes;
extern crate encoding;
extern crate getopts;
extern crate mio;
extern crate serde_json;
extern crate slab;

use encoding::{Encoding, ByteWriter, EncoderTrap};
use encoding::types::RawEncoder;
use encoding::all::ASCII;
use mio::*;
use mio::tcp::{TcpListener, TcpStream};
use serde_json::Value;
use std::io;
use std::io::Write;
use std::vec::Vec;

mod channel;

fn hex_escape(_encoder: &mut RawEncoder, input: &str, output: &mut ByteWriter) -> bool {
    for escape in input.chars().map(|ch| format!("\\u{:04X}", ch as isize)) {
        output.write_bytes(escape.as_bytes());
    }
    true
}

static HEX_ESCAPE: EncoderTrap = EncoderTrap::Call(hex_escape);

fn serve(router: &str, sock: &str, prefix: &str) -> io::Result<()> {
    let listener = TcpListener::bind(&sock.parse().expect("Wrong format for socket address"))?;
    let mut stream = TcpStream::connect(&router.parse().expect("Wrong format for router address"))?;
    stream.write(prefix.as_bytes())?;
    stream.write(b"\n")?;

    let poll = Poll::new()?;
    let mut channels = slab::Slab::new();

    const SERVER: Token = Token(0);
    const ROUTER: Token = Token(1);

    channels.insert(channel::Channel::new(stream.try_clone()?));

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
                    let channel = channel::Channel::new(client.try_clone()?);
                    let t: Token = Token(channels.insert(channel) + 1);
                    poll.register(&client, t, Ready::readable(), PollOpt::level())?;
                }
                ROUTER => {
                    match channels[0].read() {
                        Ok(messages) => {
                            for mut message in messages {
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
                    let idx = token - 1;
                    match channels[idx].read() {
                        Ok(messages) => {
                            for mut message in messages {
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
    let args: Vec<String> = std::env::args().collect();

    let mut opts = getopts::Options::new();
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
