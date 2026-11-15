//! [AP] The local port the Archipelago client (archipelago/rac1/Rac1Client.py) connects to: one JSON object a line, both ways, on
//! 127.0.0.1 only. One client at a time; a new one takes over. Its own thread ("ap-bridge"), so the client is answered while a level
//! loads or the game sits in a menu.
//!
//! From the game: `hello`, `state {level, mode}`, `found {loc: [..]}`, `death`, `goal`. From the client: `seed {seed, slot, options}`,
//! `items {items: [..]}` (every item received, in order), `names {names: {location: "what is there"}}`, `msg {text}`, `kill {text}`.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::time::Duration;

/// The port without RAC_AP's own (Archipelago's PS2 clients use PCSX2's 28011; this is the next free block).
pub const DEFAULT_PORT: u16 = 28111;

/// Lines waiting to go to the client (dropped while none is connected, but `found` is sent again on a connect).
static OUT: Mutex<Vec<String>> = Mutex::new(Vec::new());
static CONNECTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn send(line: String) {
	if CONNECTED.load(std::sync::atomic::Ordering::Relaxed) {
		OUT.lock().unwrap_or_else(|e| e.into_inner()).push(line);
	}
}

/// A JSON string's inside.
pub fn esc(s: &str) -> String {
	let mut o = String::with_capacity(s.len());
	for ch in s.chars() {
		match ch {
			'"' => o += "\\\"",
			'\\' => o += "\\\\",
			'\n' => o += "\\n",
			c if (c as u32) < 0x20 => o += &format!("\\u{:04x}", c as u32),
			c => o.push(c),
		}
	}
	o
}

pub fn start(port: u16) {
	let listener = match TcpListener::bind(("127.0.0.1", port)) {
		Ok(l) => l,
		Err(e) => {
			eprintln!("ap: could not open 127.0.0.1:{port} for the client: {e}");
			super::notice::push(format!("Archipelago: port {port} is busy"));
			return;
		}
	};
	let _ = std::thread::Builder::new().name("ap-bridge".into()).spawn(move || {
		for stream in listener.incoming().flatten() {
			eprintln!("ap: the client connected");
			serve(stream);
			CONNECTED.store(false, std::sync::atomic::Ordering::Relaxed);
			eprintln!("ap: the client left");
		}
	});
}

fn serve(stream: TcpStream) {
	let _ = stream.set_nodelay(true);
	let _ = stream.set_read_timeout(Some(Duration::from_millis(30)));
	let Ok(mut out) = stream.try_clone() else { return };
	let mut reader = BufReader::new(stream);
	OUT.lock().unwrap_or_else(|e| e.into_inner()).clear();
	CONNECTED.store(true, std::sync::atomic::Ordering::Relaxed);
	super::on_connect();
	let mut line = String::new();
	loop {
		match reader.read_line(&mut line) {
			Ok(0) => return,
			Ok(_) => {
				if line.ends_with('\n') {
					match crate::omni::json::parse(line.trim()) {
						Ok(m) => super::from_client(&m),
						Err(e) => eprintln!("ap: a line from the client is no JSON ({e})"),
					}
					line.clear();
				}
			}
			Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
			Err(_) => return,
		}
		let lines = std::mem::take(&mut *OUT.lock().unwrap_or_else(|e| e.into_inner()));
		for l in lines {
			if out.write_all(l.as_bytes()).and_then(|_| out.write_all(b"\n")).is_err() {
				return;
			}
		}
	}
}
