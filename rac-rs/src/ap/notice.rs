//! [AP] The client's lines on the screen (an item sent or received, a death link): up to three at once, low in the middle of the
//! picture, each for a few seconds, drawn through the mods' text layer in a list of their own (omni/draw.rs `submit_ap`).

use std::sync::Mutex;
use std::time::{Duration, Instant};

const SHOWN: Duration = Duration::from_secs(5);
const MAX_LINES: usize = 3;
const SIZE: f32 = 0.7;
/// Game 2D space (512 x 448): the lines start here and go down.
const TOP: f32 = 330.0;
const WHITE: u32 = 0xFFFF_FFFF;
const SHADOW: u32 = 0x0000_00C0;

static LINES: Mutex<Vec<(String, Option<Instant>)>> = Mutex::new(Vec::new());

/// A line to show (queued while others are up).
pub fn push(text: String) {
	let mut l = LINES.lock().unwrap_or_else(|e| e.into_inner());
	if l.len() < 32 {
		l.push((text, None));
	}
}

/// After each field: the lines up now go to the text layer (none outside play: the list is emptied).
pub fn draw(show: bool) {
	let mut l = LINES.lock().unwrap_or_else(|e| e.into_inner());
	l.retain(|(_, t)| t.is_none_or(|t| t.elapsed() < SHOWN));
	let (mut cmds, mut pool) = (Vec::new(), Vec::new());
	if show {
		let step = SIZE * crate::omni::draw_quads::LINE * 1.2;
		for (i, (text, t)) in l.iter_mut().take(MAX_LINES).enumerate() {
			t.get_or_insert_with(Instant::now);
			let text = clean(text);
			let text_off = pool.len() as u32;
			pool.extend_from_slice(text.as_bytes());
			cmds.push(crate::omni::OmniDrawCmd { kind: crate::omni::draw_quads::KIND_TEXT, layer: 0, align: 1, mod_index: 0, x: 256.0, y: TOP + step * i as f32, w: SIZE, h: 0.0, color: WHITE, color2: SHADOW, value: 0.0, icon: 0, text_off, text_len: text.len() as u32, reserved0: 0, reserved1: 0 });
		}
	}
	crate::omni::draw::submit_ap(&cmds, &pool);
}

/// What the game font has: printable ASCII, and at most a line's width.
fn clean(s: &str) -> String {
	let t: String = s.chars().map(|c| if (' '..='~').contains(&c) { c } else { '?' }).collect();
	if t.len() > 64 { format!("{}...", &t[..61]) } else { t }
}
