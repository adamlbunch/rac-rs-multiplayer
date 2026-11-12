//! A player's own things, kept by the team: his bolts, the items he owns and their ammo. Each game sends its player's every few
//! seconds in play (`Reliable::Keep`); the host keeps every player's by name and gives them to whoever joins, and every game remembers
//! what it heard, so a host that starts again gets them back from the players who rejoin it. A game that starts again after it
//! stopped (a crash, a closed window) and joins takes its player's back once it is in play: the bolts as kept, the items it lacks
//! given by the game's own `item.give` (one a field, as the team's progress is: mp/story.rs) and then their ammo as kept. Raj's
//! game stopped on Veldin and, back in, had the bolts of his last save (v0.4.1.4).
//!
//! A game that only lost the network keeps what it has: once a game has sent its own, it never takes one. A new player has none
//! to take: his game starts sending its own after `WAIT` fields of play.

use super::{note, notice, rd, Mp};
use ee_oracle::cpu::Cpu;
use rac_mp::{Keep, Name, Progress, KEEP_ITEMS};
use std::collections::BTreeMap;

/// The bolts (both discs, retail/vendor.rs), the owned byte of each item and its ammo (inventory.rs).
const BOLTS: u32 = 0x15_ED98;
const OWNED: u32 = 0x13_D4C0;
const AMMO: u32 = 0x13_D428;
/// Fields between two sends of a keep that changed.
const SEND_EVERY: u64 = 90;
/// Fields of play after joining before this game's own keep counts, when the team had none of this player's to give back.
const WAIT: u64 = 300;
/// The most bolts the game holds (retail/debugkeys.rs).
const MAX_BOLTS: u32 = 9_999_999;

#[derive(Default)]
pub(super) struct Keeps {
	/// Every player's last keep heard, by name (as `key` makes it).
	heard: BTreeMap<String, (Name, Keep)>,
	/// Ours to take back, once in play; `giving`: its items are being given (the items left are in `Mp::story_in`).
	pending: Option<Keep>,
	giving: bool,
	/// This game sent its own: it never takes one again.
	live: bool,
	/// Fields of play since we got in.
	played: u64,
	/// What we sent last, and the field.
	sent: Option<Keep>,
	sent_at: u64,
}

/// Names are the same player whatever their case and the spaces around them.
pub(super) fn key(name: &str) -> String {
	name.trim().to_lowercase()
}

fn read(ram: &[u8]) -> Keep {
	let mut k = Keep { bolts: rd(ram, BOLTS).min(MAX_BOLTS), ..Default::default() };
	for i in 0..KEEP_ITEMS {
		k.owned[i] = ram.get((OWNED as usize) + i).copied().unwrap_or(0);
		k.ammo[i] = rd(ram, AMMO + 4 * i as u32);
	}
	k
}

fn put(ram: &mut [u8], a: u32, v: u32) {
	if let Some(b) = ram.get_mut(a as usize..a as usize + 4) {
		b.copy_from_slice(&v.to_le_bytes());
	}
}

/// Another game's keep (or ours, given back): remembered, and taken if it is a stored copy of ours (`live`: a player's game has
/// them now, perhaps one of our name) and this game has not sent its own yet.
pub(super) fn heard(mp: &mut Mp, name: Name, keep: Keep, live: bool, my_name: &str) {
	let k = key(name.as_str());
	mp.keeps.heard.insert(k.clone(), (name, keep));
	if k == key(my_name) && !live && !mp.keeps.live {
		eprintln!("mp: the team kept {} bolts and {} items for us: they come back in play", keep.bolts, keep.owned.iter().filter(|&&b| b != 0).count());
		mp.keeps.pending = Some(keep);
	}
}

/// Back in with a host (perhaps a new one): it hears every keep this game remembers. The host keeps those it has none of.
pub(super) fn remembered(mp: &Mp, my_name: &str) -> Vec<(Name, Keep)> {
	let me = key(my_name);
	mp.keeps.heard.iter().filter(|(k, _)| **k != me).map(|(_, v)| *v).collect()
}

/// One field: ours taken back, in play; then ours sent when it changed.
pub(super) fn tick(c: &mut Cpu, mp: &mut Mp, in_play: bool) {
	if mp.my_id.is_none() || !in_play {
		return;
	}
	if !mp.keeps.live {
		if let Some(k) = mp.keeps.pending {
			if !mp.keeps.giving {
				// The items this game lacks are given the team's way, one a field.
				let mut n = 0;
				for i in 0..KEEP_ITEMS {
					let p = Progress::Item(i as u8);
					if k.owned[i] != 0 && c.mem.ram[OWNED as usize + i] == 0 && !mp.story_in.contains(&p) {
						mp.story_in.push(p);
						n += 1;
					}
				}
				eprintln!("mp: taking back what the team kept: {} items to give first", n);
				mp.keeps.giving = true;
				return;
			}
			if mp.story_in.iter().any(|p| matches!(p, Progress::Item(i) if (*i as usize) < KEEP_ITEMS && k.owned[*i as usize] != 0)) {
				return;
			}
			let ram = &mut c.mem.ram[..];
			for i in 0..KEEP_ITEMS {
				if k.owned[i] != 0 && ram[OWNED as usize + i] != 0 {
					put(ram, AMMO + 4 * i as u32, k.ammo[i]);
				}
			}
			let had = rd(ram, BOLTS);
			put(ram, BOLTS, k.bolts.min(MAX_BOLTS));
			eprintln!("mp: took back what the team kept: bolts {had} -> {}, ammo of the items set", k.bolts);
			note(format!("kept bolts {} and ammo written", k.bolts));
			notice::say(format!("Welcome back: your {} bolts and your weapons are back.", k.bolts), 8);
			mp.keeps.pending = None;
			mp.keeps.giving = false;
			mp.keeps.live = true;
		} else {
			mp.keeps.played += 1;
			if mp.keeps.played < WAIT {
				return;
			}
			mp.keeps.live = true;
		}
	}
	let now = read(&c.mem.ram);
	if mp.keeps.sent != Some(now) && (mp.keeps.sent.is_none() || mp.fields.saturating_sub(mp.keeps.sent_at) >= SEND_EVERY) {
		mp.keeps.sent = Some(now);
		mp.keeps.sent_at = mp.fields;
		super::wire().keep = Some(now);
	}
}
