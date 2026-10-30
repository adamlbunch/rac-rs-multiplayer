//! A reliable, ordered stream of messages over UDP, one `Link` per pair of players: what must arrive (an enemy killed, a crate
//! broken) is numbered, sent again every `RESEND` until the other side acknowledges it, and handed over in order. Only the
//! logic lives here, with the time as an argument; `session.rs` carries the packets.

use crate::proto::{Msg, Reliable};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How long an unacknowledged message waits before it goes out again.
const RESEND: Duration = Duration::from_millis(200);
/// Messages in flight at once.
const WINDOW: usize = 32;

pub struct Link {
	/// The number the next message gets (numbering starts at 1: an ack of 0 acknowledges nothing).
	next: u32,
	unacked: VecDeque<(u32, Reliable)>,
	sent_at: Option<Instant>,
	/// The number of the next message we can hand over.
	expect: u32,
}

impl Default for Link {
	fn default() -> Link {
		Link { next: 1, unacked: VecDeque::new(), sent_at: None, expect: 1 }
	}
}

impl Link {
	/// Queues `body` for the other side.
	pub fn push(&mut self, body: Reliable) {
		self.unacked.push_back((self.next, body));
		self.next = self.next.wrapping_add(1);
		self.sent_at = None;
	}

	/// What to send now: the oldest unacknowledged messages, when something is new or `RESEND` has passed.
	pub fn due(&mut self, now: Instant) -> Vec<Msg> {
		if self.unacked.is_empty() || self.sent_at.is_some_and(|t| now.saturating_duration_since(t) < RESEND) {
			return Vec::new();
		}
		self.sent_at = Some(now);
		self.unacked.iter().take(WINDOW).map(|(seq, body)| Msg::Reliable { seq: *seq, body: *body }).collect()
	}

	/// The other side has everything up to `seq`.
	pub fn ack(&mut self, seq: u32) {
		while self.unacked.front().is_some_and(|(s, _)| *s <= seq) {
			self.unacked.pop_front();
		}
	}

	/// A message came in: what to hand over (only the next in order: a repeat or a gap gives nothing, the sender sends again),
	/// and the number to acknowledge.
	pub fn receive(&mut self, seq: u32, body: Reliable) -> (Option<Reliable>, u32) {
		let got = (seq == self.expect).then_some(body);
		if got.is_some() {
			self.expect = self.expect.wrapping_add(1);
		}
		(got, self.expect.wrapping_sub(1))
	}

	pub fn pending(&self) -> usize {
		self.unacked.len()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::proto::WorldKey;

	fn body(slot: u16) -> Reliable {
		Reliable::Removed(WorldKey { level: 1, slot, class: 500 })
	}

	fn seqs(msgs: &[Msg]) -> Vec<u32> {
		msgs.iter().filter_map(|m| if let Msg::Reliable { seq, .. } = m { Some(*seq) } else { None }).collect()
	}

	#[test]
	fn sends_at_once_then_waits_then_sends_again_until_acknowledged() {
		let (mut l, t) = (Link::default(), Instant::now());
		assert!(l.due(t).is_empty());
		l.push(body(1));
		l.push(body(2));
		assert_eq!(seqs(&l.due(t)), [1, 2]);
		assert!(l.due(t + RESEND / 2).is_empty());
		assert_eq!(seqs(&l.due(t + RESEND)), [1, 2]);
		l.ack(1);
		assert_eq!(seqs(&l.due(t + RESEND * 2)), [2]);
		l.ack(2);
		assert!(l.due(t + RESEND * 3).is_empty());
		assert_eq!(l.pending(), 0);
	}

	#[test]
	fn hands_over_in_order_once_each_however_the_network_delivers() {
		let mut rx = Link::default();
		// 2 before 1: it waits to be sent again. 1 twice: handed over once.
		assert_eq!(rx.receive(2, body(2)), (None, 0));
		assert_eq!(rx.receive(1, body(1)), (Some(body(1)), 1));
		assert_eq!(rx.receive(1, body(1)), (None, 1));
		assert_eq!(rx.receive(2, body(2)), (Some(body(2)), 2));
	}

	#[test]
	fn a_lossy_network_loses_nothing_and_keeps_the_order() {
		let (mut tx, mut rx, mut got) = (Link::default(), Link::default(), Vec::new());
		for s in 0..100 {
			tx.push(body(s));
		}
		// Every third packet is lost, and every other ack too.
		let (mut n, mut t) = (0u32, Instant::now());
		while tx.pending() > 0 {
			t += RESEND;
			for m in tx.due(t) {
				n += 1;
				let Msg::Reliable { seq, body } = m else { continue };
				if n % 3 == 0 {
					continue;
				}
				let (b, ack) = rx.receive(seq, body);
				got.extend(b);
				if n % 2 == 0 {
					tx.ack(ack);
				}
			}
			assert!(n < 100_000, "stuck");
		}
		assert_eq!(got, (0..100).map(body).collect::<Vec<_>>());
	}

	#[test]
	fn the_window_limits_what_is_in_flight() {
		let (mut l, t) = (Link::default(), Instant::now());
		for s in 0..100 {
			l.push(body(s));
		}
		assert_eq!(l.due(t).len(), WINDOW);
	}
}
