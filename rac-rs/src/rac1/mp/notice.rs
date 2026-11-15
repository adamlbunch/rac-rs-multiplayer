//! Short lines at the top of the screen about the session, so a player sees what the run log says: joining, connected, who came
//! and went, a host that does not answer or turned this game away, multiplayer that could not start. One line stays while it holds
//! (`status`: joining, no answer, refused, the host lost); the others go after a few seconds on screen (`say`), counted only while
//! they show, so one said during a story movie shows after it. Drawn with the names (mp.rs `draw_names`) while the front end runs
//! (title, menus and its movies) and in the game, not over a movie or a scene there. Set from the game's thread and the network thread.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rac_mp::PlayerId;

/// The game's text space is 512 pixels wide; at `SIZE` a line holds about this many characters.
const WIDTH: usize = 58;
/// Lines that go: at most this many at once (the newest kept), and never older than this (a long scene: stale by then).
const KEEP: usize = 4;
const STALE: Duration = Duration::from_secs(90);
/// Fields a second, for the time a line shows (a PAL game's 50 show it a little longer).
const FIELDS_A_SECOND: u32 = 60;
/// Text size (1 = the font's 16-pixel line), the first line's top, and the colours (RGBA): white, a warning's yellow, the shadow.
pub(super) const SIZE: f32 = 0.75;
pub(super) const TOP: f32 = 22.0;
/// The first line's top on the front end: under the title's menu and above "Press Start", clear of the logo.
pub(super) const TOP_FRONT: f32 = 322.0;
pub(super) const WHITE: u32 = 0xFFFF_FFFF;
pub(super) const YELLOW: u32 = 0xFFD8_40FF;
pub(super) const SHADOW: u32 = 0x0000_00C0;

struct Lines {
	/// The line that stays, and whether it is a warning.
	status: Option<(String, bool)>,
	/// What to do next, on the title screen (`hint`): drawn under the line that stays.
	hint: Option<String>,
	/// The lines that go: the text, the fields it still shows, and when it was said.
	said: Vec<(String, u32, Instant)>,
	/// Everyone's names as last heard, so one who left is still named.
	names: BTreeMap<PlayerId, String>,
}

static LINES: Mutex<Lines> = Mutex::new(Lines { status: None, hint: None, said: Vec::new(), names: BTreeMap::new() });

fn lines_lock() -> std::sync::MutexGuard<'static, Lines> {
	LINES.lock().unwrap_or_else(|e| e.into_inner())
}

/// The line that stays (None: no such line). `warn`: drawn yellow.
pub(super) fn status(text: Option<String>, warn: bool) {
	lines_lock().status = text.map(|t| (t, warn));
}

/// What the player does next (None: nothing to say), set every field: on the title screen, how to get into the game.
pub(super) fn hint(text: Option<String>) {
	let mut l = lines_lock();
	if l.hint != text {
		l.hint = text;
	}
}

/// A line that goes after it showed for `secs` seconds.
pub(super) fn say(text: impl Into<String>, secs: u32) {
	let mut l = lines_lock();
	l.said.push((text.into(), secs * FIELDS_A_SECOND, Instant::now()));
	let over = l.said.len().saturating_sub(KEEP);
	l.said.drain(..over);
}

/// The names in the session now (kept: a name stays known after its player left).
pub(super) fn roster(roster: &[(PlayerId, String)]) {
	let mut l = lines_lock();
	for (id, name) in roster {
		if l.names.get(id) != Some(name) {
			l.names.insert(*id, name.clone());
		}
	}
}

/// A player's name as last heard ("A player" for one never named).
pub(super) fn name_of(id: PlayerId) -> String {
	lines_lock().names.get(&id).cloned().unwrap_or_else(|| "A player".into())
}

/// Once a field: the lines to draw now when they `show`, the one that stays first, each cut to the screen's width at a space, and
/// whether each is a warning. A line that goes counts this field only when it shows.
pub(super) fn lines(show: bool) -> Vec<(String, bool)> {
	let mut l = lines_lock();
	let now = Instant::now();
	l.said.retain(|(_, left, at)| *left > 0 && now.duration_since(*at) < STALE);
	if !show {
		return Vec::new();
	}
	for (_, left, _) in l.said.iter_mut() {
		*left -= 1;
	}
	let mut out = Vec::new();
	let all = l.status.iter().cloned().chain(l.hint.iter().map(|t| (t.clone(), false))).chain(l.said.iter().map(|(t, _, _)| (t.clone(), false)));
	for (text, warn) in all {
		out.extend(wrap(&text, WIDTH).into_iter().map(|t| (t, warn)));
	}
	out
}

/// `text` in lines of at most `width` characters, broken at spaces (a word longer than a line is cut).
fn wrap(text: &str, width: usize) -> Vec<String> {
	let mut out: Vec<String> = Vec::new();
	let mut line = String::new();
	for word in text.split_whitespace() {
		let word: String = word.chars().take(width).collect();
		if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
			out.push(std::mem::take(&mut line));
		}
		if !line.is_empty() {
			line.push(' ');
		}
		line.push_str(&word);
	}
	if !line.is_empty() {
		out.push(line);
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn long_lines_break_at_spaces() {
		assert_eq!(wrap("one two three", 7), vec!["one two", "three"]);
		assert_eq!(wrap("abcdefghij", 4), vec!["abcd"]);
		assert_eq!(wrap("  ", 10), Vec::<String>::new());
	}
}
