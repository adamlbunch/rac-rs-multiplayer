//! [AP] The game's own prompts where a location sells or trades its reward ("Buy Heli-Pack for 1,000 bolts", the R.Y.N.O., the
//! nanotech, the Persuader trade ...) name what the multiworld put there instead, as APRac2 does for Ratchet & Clank 2's vendors and
//! NPCs. The texts are the game's: read from the player's own disc (frontend::Text, ORACLE_ISO) and found in RAM by their bytes, then
//! the highlighted name in them is replaced in place (a name that does not fit drops the price after it first, then is cut).
//! Nothing of the game's text is in this code: only the text ids.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// (text id, location id, which highlighted name in the text): the prompts of a location that sells or trades its reward.
/// The text ids sit in each level's block of the game's text (Novalis 0x3e8.., Kerwan 0xbb8.., Rilgar 0x1388..), next to the
/// texts of the same place; the nanotech's are in the code of Orxon's seller (class 1326).
const PROMPTS: [(u32, u32, usize); 19] = [
	(0x3f3, super::LOC_INFOBOT + 3, 0),  // Novalis: the infobot sold for 500 bolts (Kerwan's)
	(0x3f4, super::LOC_INFOBOT + 3, 0),
	(0xbd0, super::LOC_ITEM + 2, 0),     // Kerwan: the Heli-Pack
	(0xbd2, super::LOC_ITEM + 2, 0),
	(0xbd1, super::LOC_ITEM + 12, 0),    // Kerwan: the Swingshot
	(0xbd3, super::LOC_ITEM + 12, 0),
	(0x139e, super::LOC_ITEM + 23, 0),   // Rilgar: the R.Y.N.O.
	(0x13a1, super::LOC_ITEM + 23, 0),
	(0x1787, super::LOC_ITEM + 29, 0),   // Blarg Station: the Grindboots
	(0x1788, super::LOC_ITEM + 29, 0),
	(0x271e, super::LOC_SPECIAL + 10 * 16 + 4, 0), // Orxon: Premium Nanotech
	(0x2720, super::LOC_SPECIAL + 10 * 16 + 4, 0),
	(0x271f, super::LOC_SPECIAL + 10 * 16 + 5, 0), // Orxon: Ultra Nanotech
	(0x2721, super::LOC_SPECIAL + 10 * 16 + 5, 0),
	(0x2b08, super::LOC_ITEM + 3, 0),    // Pokitaru: the Thruster-Pack
	(0x2b0f, super::LOC_ITEM + 35, 1),   // Pokitaru: the Raritanium trade (the second name: the Persuader)
	(0x2ee9, super::LOC_ITEM + 4, 0),    // Hoven: the Hydro-Pack
	(0x53ea, super::LOC_ITEM + 32, 0),   // Oltanis: the Gadgetron PDA
	(0x53eb, super::LOC_ITEM + 32, 0),
];

/// The highlight around a name in the game's text (it draws what is between in the highlight colour).
const HL_ON: u8 = 0x0c;
const HL_OFF: u8 = 0x08;
/// A level's texts load with it: ids `level * 1000 ..`; from 19 * 1000 on they are the game's own, always there. Looked for a few
/// times after a level starts, some seconds apart (a scan of RAM takes tens of milliseconds).
const LOOK_EVERY: Duration = Duration::from_secs(5);
const LOOKS: u8 = 3;
const RAM_LO: usize = 0x10_0000;

fn text_level(id: u32) -> i32 {
	(id / 1000) as i32
}

struct Texts {
	/// The game's own text of each prompt (from the disc), its place in RAM, and what was written there last.
	original: BTreeMap<u32, Vec<u8>>,
	at: BTreeMap<u32, usize>,
	written: BTreeMap<u32, Vec<u8>>,
	looked: Option<Instant>,
	/// The level the looks were for, and how many were made.
	level: i32,
	looks: u8,
	said: bool,
}

static TEXTS: Mutex<Option<Texts>> = Mutex::new(None);
static NAMES: Mutex<BTreeMap<u32, String>> = Mutex::new(BTreeMap::new());

/// The client said what each location holds ("Swingshot", "Hookshot for Link").
pub fn set_names(names: BTreeMap<u32, String>) {
	eprintln!("ap: the client says what {} locations hold", names.len());
	*NAMES.lock().unwrap_or_else(|e| e.into_inner()) = names;
}

/// A text as the run log shows it (the game's control bytes as `^`).
fn shown(s: &[u8]) -> String {
	s.iter().map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '^' }).collect()
}

/// The prompts' texts from the disc, in the language whose text is in RAM (tried in order).
fn load_originals() -> BTreeMap<u32, Vec<u8>> {
	let mut out = BTreeMap::new();
	let Ok(iso) = std::env::var("ORACLE_ISO") else { return out };
	let Ok(mut disc) = crate::iso::Disc::open(std::path::Path::new(&iso)) else { return out };
	let Ok(text) = crate::frontend::Text::load(&mut disc, 0) else { return out };
	for &(id, _, _) in &PROMPTS {
		let s = text.get(id);
		if !s.is_empty() {
			out.insert(id, s.chars().map(|c| c as u32 as u8).collect());
		}
	}
	out
}

/// After each field in a level (ap.rs `tick`): the prompts found in RAM are written with the names the client sent.
pub fn tick(ram: &mut [u8], level: i32) {
	let names = NAMES.lock().unwrap_or_else(|e| e.into_inner()).clone();
	if names.is_empty() {
		return;
	}
	let mut g = TEXTS.lock().unwrap_or_else(|e| e.into_inner());
	let t = g.get_or_insert_with(|| Texts { original: load_originals(), at: BTreeMap::new(), written: BTreeMap::new(), looked: None, level: -1, looks: 0, said: false });
	if t.original.is_empty() {
		return;
	}
	// A place no longer holding the text (another level's text loaded there) is looked for again.
	let stale: Vec<u32> = t.at.iter().filter(|&(id, &a)| !holds(ram, a, &t.original[id]) && t.written.get(id).is_none_or(|w| !holds(ram, a, w))).map(|(id, _)| *id).collect();
	// Texts gone from their place: the level loaded again (a death, the escape), they are looked for again from the start.
	if !stale.is_empty() {
		t.looks = 0;
	}
	for id in stale {
		t.at.remove(&id);
		t.written.remove(&id);
	}
	if t.level != level {
		(t.level, t.looks) = (level, 0);
	}
	let missing = t.original.keys().any(|&id| !t.at.contains_key(&id) && (text_level(id) == level || text_level(id) >= 19));
	if missing && t.looks < LOOKS && t.looked.is_none_or(|l| l.elapsed() >= LOOK_EVERY) {
		t.looked = Some(Instant::now());
		t.looks += 1;
		find(ram, t, level);
		let left: Vec<String> = t.original.keys().filter(|&&id| !t.at.contains_key(&id) && (text_level(id) == level || text_level(id) >= 19)).map(|id| format!("{id:#x}")).collect();
		if !left.is_empty() && t.looks == LOOKS {
			eprintln!("ap: prompts {} of level {level} not in RAM", left.join(", "));
		}
	}
	let mut done = 0;
	for &(id, loc, which) in &PROMPTS {
		let (Some(&a), Some(orig), Some(name)) = (t.at.get(&id), t.original.get(&id), names.get(&loc)) else { continue };
		let new = rename(orig, which, name);
		if t.written.get(&id) != Some(&new) {
			if let Some(dst) = ram.get_mut(a..a + orig.len()) {
				dst.fill(0);
				dst[..new.len()].copy_from_slice(&new);
				eprintln!("ap: prompt {id:#x} at {a:#x} now reads \"{}\"", shown(&new));
				t.written.insert(id, new);
			}
		}
		done += 1;
	}
	if done > 0 && !t.said {
		t.said = true;
		eprintln!("ap: {done} of the game's prompts name what the multiworld put at their place");
	}
}

/// Whether RAM at `a` holds the text `s` (and its end).
fn holds(ram: &[u8], a: usize, s: &[u8]) -> bool {
	ram.get(a..a + s.len()) == Some(s) && ram.get(a + s.len()) == Some(&0)
}

/// The prompts of this level (and the game's own) not placed yet, looked for in RAM: a text is the whole of a NUL-ended string (the
/// byte before is a NUL too).
fn find(ram: &[u8], t: &mut Texts, level: i32) {
	let want: Vec<(u32, Vec<u8>)> = t
		.original
		.iter()
		.filter(|(id, _)| !t.at.contains_key(id) && (text_level(**id) == level || text_level(**id) >= 19))
		.map(|(id, s)| (*id, s.clone()))
		.collect();
	for (id, s) in want {
		let Some(&first) = s.first() else { continue };
		let end = ram.len().saturating_sub(s.len() + 1);
		let mut i = RAM_LO;
		while i < end {
			if ram[i] == first && ram[i - 1] == 0 && holds(ram, i, &s) {
				t.at.insert(id, i);
				break;
			}
			i += 1;
		}
	}
}

/// `orig` with its `which`-th highlighted name replaced by `name`, no longer than `orig`: what follows the name goes first, then the
/// name is cut.
fn rename(orig: &[u8], which: usize, name: &str) -> Vec<u8> {
	let name: Vec<u8> = name.chars().map(|c| if (' '..='~').contains(&c) { c as u8 } else { b'?' }).collect();
	let mut starts = orig.iter().enumerate().filter(|&(_, &b)| b == HL_ON).map(|(i, _)| i);
	let Some(s) = starts.nth(which) else { return orig.to_vec() };
	let Some(e) = orig[s..].iter().position(|&b| b == HL_OFF).map(|p| s + p) else { return orig.to_vec() };
	let (head, tail) = (&orig[..s + 1], &orig[e..]);
	let fits = |n: usize, with_tail: bool| head.len() + n + if with_tail { tail.len() } else { 1 } <= orig.len();
	let (n, with_tail) = if fits(name.len(), true) {
		(name.len(), true)
	} else if fits(name.len(), false) {
		(name.len(), false)
	} else {
		(orig.len().saturating_sub(head.len() + 1), false)
	};
	let mut out = head.to_vec();
	out.extend_from_slice(&name[..n.min(name.len())]);
	if with_tail {
		out.extend_from_slice(tail);
	} else {
		out.push(HL_OFF);
	}
	out.truncate(orig.len());
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_name_goes_in_the_highlight_and_the_price_goes_first_when_it_is_long() {
		let orig = b"\x12 Buy \x0cThing\x08 for 1,000 bolts";
		assert_eq!(rename(orig, 0, "Gizmo"), b"\x12 Buy \x0cGizmo\x08 for 1,000 bolts".to_vec());
		let long = rename(orig, 0, "A Much Longer Name Here");
		assert!(long.len() <= orig.len());
		assert!(long.starts_with(b"\x12 Buy \x0cA Much Longer"));
		assert_eq!(*long.last().unwrap(), HL_OFF);
		let two = b"\x12 Trade \x0cOne\x08 for \x0cTwo\x08";
		assert_eq!(rename(two, 1, "Six"), b"\x12 Trade \x0cOne\x08 for \x0cSix\x08".to_vec());
	}
}
