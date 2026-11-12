//! Sessions over real sockets on 127.0.0.1.

use super::*;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU16, Ordering};

/// A port of its own for each test: a client still alive from another test would walk into a host that got its old port back.
fn host(hz: u8) -> (Session, SocketAddr) {
	static NEXT: AtomicU16 = AtomicU16::new(0);
	let base = 20_000 + (std::process::id() % 20_000) as u16;
	loop {
		let port = base + NEXT.fetch_add(1, Ordering::Relaxed);
		if let Ok(h) = Session::host(format!("127.0.0.1:{port}").parse().unwrap(), "Host", hz) {
			let a = h.local_addr().unwrap();
			return (h, a);
		}
	}
}

fn join(at: SocketAddr, name: &str, hz: u8) -> Session {
	Session::join(at, name, hz, Instant::now()).unwrap()
}

fn hero(tick: u32) -> HeroState {
	HeroState { tick, level: 1, pos: [tick as f32, 2.0, 3.0], ..Default::default() }
}

/// Polls every session until `done` is happy with the events so far (2 s at most).
fn pump(ss: &mut [&mut Session], done: impl Fn(&[Vec<Event>]) -> bool) -> Vec<Vec<Event>> {
	let mut ev = vec![Vec::new(); ss.len()];
	let end = Instant::now() + Duration::from_secs(2);
	while Instant::now() < end {
		for (s, e) in ss.iter_mut().zip(ev.iter_mut()) {
			e.extend(s.poll(Instant::now()));
		}
		if done(&ev) {
			return ev;
		}
		std::thread::sleep(Duration::from_millis(2));
	}
	panic!("timed out; events so far: {ev:?}");
}

fn has(ev: &[Event], f: impl Fn(&Event) -> bool) -> bool {
	ev.iter().any(f)
}

#[test]
fn a_client_joins_and_both_see_each_other() {
	let (mut h, at) = host(60);
	let mut c = join(at, "Clank", 60);
	pump(&mut [&mut h, &mut c], |e| {
		has(&e[0], |x| *x == Event::Joined { id: 1, name: "Clank".into() })
			&& has(&e[1], |x| *x == Event::Connected { id: 1 })
			&& has(&e[1], |x| *x == Event::Joined { id: 0, name: "Host".into() })
	});
	assert_eq!(c.id(), Some(1));
	assert_eq!(c.name_of(0), Some("Host"));
	c.send_state(&hero(5));
	h.send_state(&hero(9));
	pump(&mut [&mut h, &mut c], |e| {
		has(&e[0], |x| *x == Event::State { id: 1, hero: hero(5) }) && has(&e[1], |x| *x == Event::State { id: 0, hero: hero(9) })
	});
}

#[test]
fn the_host_relays_between_clients() {
	let (mut h, at) = host(50);
	let mut a = join(at, "A", 50);
	let mut b = join(at, "B", 50);
	pump(&mut [&mut h, &mut a, &mut b], |e| {
		has(&e[1], |x| matches!(x, Event::Connected { .. })) && has(&e[2], |x| matches!(x, Event::Connected { .. }))
	});
	let (ia, ib) = (a.id().unwrap(), b.id().unwrap());
	assert_ne!(ia, ib);
	a.send_state(&hero(77));
	pump(&mut [&mut h, &mut a, &mut b], |e| has(&e[2], |x| *x == Event::State { id: ia, hero: hero(77) }));
}

#[test]
fn a_full_session_and_the_other_region_are_refused() {
	let (mut h, at) = host(60);
	let mut cs: Vec<Session> = (0..MAX_PLAYERS - 1).map(|i| join(at, &format!("P{i}"), 60)).collect();
	{
		let mut all: Vec<&mut Session> = std::iter::once(&mut h).chain(cs.iter_mut()).collect();
		pump(&mut all, |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	}
	let mut late = join(at, "Late", 60);
	pump(&mut [&mut h, &mut late], |e| has(&e[1], |x| *x == Event::Rejected(RejectReason::Full)));
	assert!(late.is_over());
	let (mut h2, at2) = host(60);
	let mut pal = join(at2, "Pal", 50);
	pump(&mut [&mut h2, &mut pal], |e| has(&e[1], |x| *x == Event::Rejected(RejectReason::Region)));
}

#[test]
fn leaving_and_going_silent_are_noticed() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	let mut d = join(at, "D", 60);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	let (ic, id) = (c.id().unwrap(), d.id().unwrap());
	c.close();
	pump(&mut [&mut h], |e| has(&e[0], |x| *x == Event::Left { id: ic }));
	// Take in what the host sent D meanwhile (the new roster), so the silence below starts now.
	std::thread::sleep(Duration::from_millis(50));
	d.poll(Instant::now());
	// Past the timeout, with nothing heard from D: the host drops it and D gives the host up.
	let later = Instant::now() + TIMEOUT + Duration::from_secs(1);
	assert!(h.poll(later).contains(&Event::Left { id }));
	assert!(d.poll(later).contains(&Event::Lost));
	assert!(d.is_over());
}

#[test]
fn a_player_whose_packets_would_not_decode_is_not_silent() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Connected { .. })));
	let id = c.id().unwrap();
	// What C sent so far is read now, so below the host has only the bad packet to go on.
	std::thread::sleep(Duration::from_millis(50));
	h.poll(Instant::now());
	// C sends only something this version cannot read (a ping with a byte too many), ten seconds on: the host still hears C.
	let mut bad = proto::encode(&Msg::Ping);
	bad.push(0);
	c.sock.send_to(&bad, at).unwrap();
	std::thread::sleep(Duration::from_millis(50));
	let t = Instant::now() + Duration::from_secs(10);
	assert!(!h.poll(t).contains(&Event::Left { id }));
	assert!(!h.poll(t + TIMEOUT - Duration::from_secs(1)).contains(&Event::Left { id }));
	assert_eq!(h.stats().undecodable, 1);
}

#[test]
fn a_player_the_host_dropped_is_told_at_once() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Connected { .. })));
	let id = c.id().unwrap();
	// C's game stalls: some seconds of silence are borne (a level loading on a slow PC), longer than the timeout is gone.
	// (What C sent last is read at the first of these polls: the silence runs from there.)
	let t = Instant::now() + Duration::from_secs(8);
	assert!(!h.poll(t).contains(&Event::Left { id }));
	assert!(!h.poll(t + TIMEOUT - Duration::from_secs(1)).contains(&Event::Left { id }));
	assert!(h.poll(t + TIMEOUT + Duration::from_secs(1)).contains(&Event::Left { id }));
	// C goes on and sends its state: the host says it is not in, and C knows at once, to join again.
	c.send_state(&hero(1));
	let ev = pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| *x == Event::Lost));
	assert!(has(&ev[1], |x| *x == Event::Lost));
	assert!(c.is_over());
	// The host says so once, and C was told just now (not silent for long).
	assert_eq!(ev[0].iter().filter(|x| matches!(x, Event::Stranger(_))).count(), 1);
	assert!(c.silent_for(Instant::now()) < Duration::from_secs(1));
	assert!(!has(&ev[0], |x| matches!(x, Event::Joined { .. } | Event::State { .. })));
}

fn key(slot: u16) -> WorldKey {
	WorldKey { level: 1, slot, class: 500 }
}

#[test]
fn the_host_decides_the_world_and_late_arrivals_are_told() {
	let (mut h, at) = host(60);
	let mut a = join(at, "A", 60);
	let mut b = join(at, "B", 60);
	pump(&mut [&mut h, &mut a, &mut b], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	// A client reports: the host and the other client hear it, the reporter does not.
	a.report(key(7));
	let ev = pump(&mut [&mut h, &mut a, &mut b], |e| has(&e[0], |x| *x == Event::Removed(key(7))) && has(&e[2], |x| *x == Event::Removed(key(7))));
	assert!(!has(&ev[1], |x| matches!(x, Event::Removed(_))));
	// The host's own report goes to both clients and raises nothing on the host.
	h.report(key(8));
	let ev = pump(&mut [&mut h, &mut a, &mut b], |e| e[1..].iter().all(|ev| has(ev, |x| *x == Event::Removed(key(8)))));
	assert!(!has(&ev[0], |x| matches!(x, Event::Removed(_))));
	// Both clients report the same object: it counts once.
	a.report(key(9));
	b.report(key(9));
	let ev = pump(&mut [&mut h, &mut a, &mut b], |e| has(&e[0], |x| *x == Event::Removed(key(9))));
	assert_eq!(ev[0].iter().filter(|x| **x == Event::Removed(key(9))).count(), 1);
	// Someone joining now is told everything, in order.
	let mut late = join(at, "Late", 60);
	let ev = pump(&mut [&mut h, &mut a, &mut b, &mut late], |e| has(&e[3], |x| *x == Event::Removed(key(9))));
	let told: Vec<_> = ev[3].iter().filter(|x| matches!(x, Event::Removed(_))).cloned().collect();
	assert_eq!(told, [7, 8, 9].map(|s| Event::Removed(key(s))));
}

#[test]
fn enemies_are_assigned_streamed_and_hit_across_games() {
	let (mut h, at) = host(60);
	let mut a = join(at, "A", 60);
	let mut b = join(at, "B", 60);
	pump(&mut [&mut h, &mut a, &mut b], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	let ia = a.id().unwrap();
	// The host gives an enemy to A: both clients hear it.
	h.assign(key(5), ia);
	pump(&mut [&mut h, &mut a, &mut b], |e| e[1..].iter().all(|ev| has(ev, |x| *x == Event::Owner { key: key(5), owner: ia })));
	// A streams more poses than fit one datagram: the host and B get them all, in A's name.
	let poses: Vec<EnemyPose> = (0..MAX_POSES as u16 + 5).map(|slot| EnemyPose { slot, ..Default::default() }).collect();
	a.send_enemies(1, 40, &poses);
	let count = |ev: &[Event]| ev.iter().map(|x| if let Event::Enemies { id, poses, .. } = x { assert_eq!(*id, ia); poses.len() } else { 0 }).sum::<usize>();
	pump(&mut [&mut h, &mut a, &mut b], |e| count(&e[0]) == poses.len() && count(&e[2]) == poses.len());
	// B hits it: the host and A hear, whoever runs it takes the hit.
	b.send_hit(key(5), 2.0, 0x20);
	let hit = Event::Hit { key: key(5), damage: 2.0, mask: 0x20 };
	pump(&mut [&mut h, &mut a, &mut b], |e| has(&e[0], |x| *x == hit) && has(&e[1], |x| *x == hit));
	// The host hits one too, and a late arrival learns who runs what.
	h.send_hit(key(6), 1.0, 0x20);
	pump(&mut [&mut h, &mut a, &mut b], |e| e[1..].iter().all(|ev| has(ev, |x| *x == Event::Hit { key: key(6), damage: 1.0, mask: 0x20 })));
	drop(b);
	let mut late = join(at, "Late", 60);
	pump(&mut [&mut h, &mut a, &mut late], |e| has(&e[2], |x| *x == Event::Owner { key: key(5), owner: ia }));
}

#[test]
fn the_same_spawn_from_two_games_counts_once() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Connected { .. })));
	let now = Instant::now();
	// The host's game spawns it, then the client's game spawns the same one a little later: its copy stands down.
	h.report_spawn(1, 5, 872, [10.0, 0.0, 0.0], false, false, now);
	let ev = pump(&mut [&mut h, &mut c], |e| has(&e[0], |x| matches!(x, Event::Spawned { owner: 0, local: 1, .. })) && has(&e[1], |x| matches!(x, Event::Spawned { owner: 0, .. })));
	let id = ev[0].iter().find_map(|x| if let Event::Spawned { id, .. } = x { Some(*id) } else { None }).unwrap();
	assert!(id >= SPAWNED_ID);
	c.report_spawn(7, 5, 872, [11.0, 0.0, 0.0], false, false, now);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| *x == Event::Duplicate { local: 7, of: id }));
	// Far away, another class or another level is a new object, and it is the client's.
	c.report_spawn(8, 5, 872, [90.0, 0.0, 0.0], false, false, now);
	c.report_spawn(9, 5, 873, [10.0, 0.0, 0.0], false, false, now);
	c.report_spawn(10, 6, 872, [10.0, 0.0, 0.0], false, false, now);
	let ev = pump(&mut [&mut h, &mut c], |e| [8, 9, 10].iter().all(|l| has(&e[1], |x| matches!(x, Event::Spawned { local, owner, .. } if local == l && *owner == 1))));
	let ids: HashSet<u16> = ev[1].iter().filter_map(|x| if let Event::Spawned { id, .. } = x { Some(*id) } else { None }).collect();
	assert_eq!(ids.len(), 3);
	// Someone joining later hears of what is alive, not of what is gone.
	let gone = WorldKey { level: 5, slot: id, class: 872 };
	h.report(gone);
	let mut late = join(at, "Late", 60);
	let ev = pump(&mut [&mut h, &mut c, &mut late], |e| e[2].iter().filter(|x| matches!(x, Event::Spawned { .. })).count() == 3);
	assert!(!has(&ev[2], |x| matches!(x, Event::Spawned { id: i, .. } if *i == id)));
}

#[test]
fn a_burst_from_one_game_is_every_shot() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Connected { .. })));
	let now = Instant::now();
	for local in 0..4 {
		c.report_spawn(local, 3, 545, [5.0, 5.0, 5.0], false, false, now);
	}
	let ev = pump(&mut [&mut h, &mut c], |e| e[1].iter().filter(|x| matches!(x, Event::Spawned { owner: 1, .. })).count() == 4);
	assert!(!has(&ev[1], |x| matches!(x, Event::Duplicate { .. })));
	// The same spawner in the other game makes the same four: each is a copy of one, and the fifth is its own.
	for local in 0..5 {
		h.report_spawn(local, 3, 545, [5.0, 5.0, 5.0], false, false, now);
	}
	let ev = pump(&mut [&mut h, &mut c], |e| e[0].iter().filter(|x| matches!(x, Event::Duplicate { .. })).count() == 4 && has(&e[0], |x| matches!(x, Event::Spawned { owner: 0, local: 4, .. })));
	assert_eq!(ev[0].iter().filter(|x| matches!(x, Event::Duplicate { .. })).count(), 4);
}

#[test]
fn a_lasting_object_is_a_copy_whenever_a_game_gets_there() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Connected { .. })));
	h.travel(5);
	let then = Instant::now();
	h.report_spawn(1, 5, 872, [10.0, 0.0, 0.0], true, false, then);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Spawned { owner: 0, .. })));
	// A minute later: too late for a copy, unless it lasts.
	let later = then + Duration::from_secs(60);
	c.report_spawn(7, 5, 872, [10.0, 0.0, 0.0], true, false, later);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Duplicate { local: 7, .. })));
	c.report_spawn(8, 5, 872, [10.0, 0.0, 0.0], false, false, later);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Spawned { owner: 1, local: 8, .. })));
	// Back on the level after the host left it, everything starts again.
	h.travel(3);
	h.travel(5);
	c.report_spawn(9, 5, 872, [10.0, 0.0, 0.0], true, false, later);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Spawned { owner: 1, local: 9, .. })));
}

#[test]
fn a_spawned_object_changes_hands() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Connected { .. })));
	h.travel(5);
	let now = Instant::now();
	c.report_spawn(1, 5, 872, [10.0, 0.0, 0.0], true, false, now);
	let ev = pump(&mut [&mut h, &mut c], |e| has(&e[0], |x| matches!(x, Event::Spawned { owner: 1, .. })));
	let id = ev[0].iter().find_map(|x| if let Event::Spawned { id, .. } = x { Some(*id) } else { None }).unwrap();
	let key = WorldKey { level: 5, slot: id, class: 872 };
	// To the host, and whoever joins later hears the new owner, whichever way they read it.
	h.assign(key, 0);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| *x == Event::Owner { key, owner: 0 }));
	let mut late = join(at, "Late", 60);
	pump(&mut [&mut h, &mut c, &mut late], |e| has(&e[2], |x| matches!(x, Event::Spawned { id: i, owner: 0, .. } if *i == id)));
	// The game it left comes back to the level: what it makes is a copy of the object it had. The game that has it now is no copy.
	c.report_spawn(2, 5, 872, [10.0, 0.0, 0.0], true, false, now + Duration::from_secs(60));
	pump(&mut [&mut h, &mut c, &mut late], |e| has(&e[1], |x| *x == Event::Duplicate { local: 2, of: id }));
	// Gone for good: nobody joining later hears of its owner.
	h.report(key);
	let mut later = join(at, "Later", 60);
	let ev = pump(&mut [&mut h, &mut c, &mut late, &mut later], |e| has(&e[3], |x| matches!(x, Event::Connected { .. })));
	std::thread::sleep(Duration::from_millis(30));
	let more = later.poll(Instant::now());
	assert!(!ev[3].iter().chain(&more).any(|x| matches!(x, Event::Owner { key: k, .. } if *k == key)));
}

#[test]
fn effects_reach_everyone_else() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	let mut d = join(at, "D", 60);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	let cid = c.id().unwrap();
	let events: Vec<FxEvent> = (0..MAX_FX as u16 + 3).map(|k| FxEvent::Cue { slot: k, cue: 4, arg: 0 }).collect();
	c.send_fx(3, &events);
	let got = |e: &[Event]| e.iter().filter_map(|x| if let Event::Fx { id, level: 3, events } = x { (*id == cid).then_some(events.len()) } else { None }).sum::<usize>();
	let ev = pump(&mut [&mut h, &mut c, &mut d], |e| got(&e[0]) == events.len() && got(&e[2]) == events.len());
	assert_eq!(got(&ev[1]), 0);
	// The host's own reach the clients.
	h.send_fx(3, &events[..2]);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| ev.iter().any(|x| matches!(x, Event::Fx { id: 0, .. }))));
}

#[test]
fn moving_machinery_reaches_everyone_else() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	let mut d = join(at, "D", 60);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	let cid = c.id().unwrap();
	let poses: Vec<EnemyPose> = (0..MAX_POSES as u16 + 2).map(|slot| EnemyPose { slot, ..Default::default() }).collect();
	c.send_machines(4, 9, &poses);
	let got = |e: &[Event]| e.iter().map(|x| if let Event::Machines { id, level: 4, poses, .. } = x { assert_eq!(*id, cid); poses.len() } else { 0 }).sum::<usize>();
	let ev = pump(&mut [&mut h, &mut c, &mut d], |e| got(&e[0]) == poses.len() && got(&e[2]) == poses.len());
	assert_eq!(got(&ev[1]), 0);
	// Machinery is not enemies.
	assert!(!ev.iter().flatten().any(|x| matches!(x, Event::Enemies { .. })));
	h.send_machines(4, 10, &poses[..1]);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| ev.iter().any(|x| matches!(x, Event::Machines { id: 0, .. }))));
}

#[test]
fn the_teams_progress_reaches_everyone_once_and_late_arrivals_too() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	let mut d = join(at, "D", 60);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	let item = Event::Story(Progress::Item(26));
	c.progress(Progress::Item(26));
	let ev = pump(&mut [&mut h, &mut c, &mut d], |e| has(&e[0], |x| *x == item) && has(&e[2], |x| *x == item));
	assert!(!has(&ev[1], |x| *x == item));
	// Again from D: nobody hears it twice.
	d.progress(Progress::Item(26));
	h.progress(Progress::Planet(3));
	let planet = Event::Story(Progress::Planet(3));
	let ev = pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| *x == planet)));
	assert!(!ev.iter().flatten().any(|x| *x == item));
	drop(d);
	let mut late = join(at, "Late", 60);
	pump(&mut [&mut h, &mut c, &mut late], |e| has(&e[2], |x| *x == item) && has(&e[2], |x| *x == planet));
}

#[test]
fn an_objects_variables_reach_everyone_else_and_the_newest_a_late_arrival() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	let mut d = join(at, "D", 60);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	let key = WorldKey { level: 1, slot: 297, class: 280 };
	let turned = |w: u32| Event::Vars { key, state: 1, words: vec![0, w, 7] };
	c.send_vars(key, 1, &[0, 0x3e00_0000, 7]);
	let ev = pump(&mut [&mut h, &mut c, &mut d], |e| has(&e[0], |x| *x == turned(0x3e00_0000)) && has(&e[2], |x| *x == turned(0x3e00_0000)));
	assert!(!has(&ev[1], |x| matches!(x, Event::Vars { .. })));
	// Turned further, from D now: the late arrival hears only the newest.
	d.send_vars(key, 1, &[0, 0x3e85_d7a8, 7]);
	pump(&mut [&mut h, &mut c, &mut d], |e| has(&e[1], |x| *x == turned(0x3e85_d7a8)));
	drop(d);
	let mut late = join(at, "Late", 60);
	let ev = pump(&mut [&mut h, &mut c, &mut late], |e| has(&e[2], |x| *x == turned(0x3e85_d7a8)));
	assert!(!has(&ev[2], |x| *x == turned(0x3e00_0000)));
	// Too many words: the first MAX_VARS travel.
	h.send_vars(key, 2, &[9; MAX_VARS + 4]);
	pump(&mut [&mut h, &mut c, &mut late], |e| e[1..].iter().all(|ev| has(ev, |x| *x == Event::Vars { key, state: 2, words: vec![9; MAX_VARS] })));
}

#[test]
fn a_cutscene_started_in_one_game_reaches_every_other_game() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	let mut d = join(at, "D", 60);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	let scene = Event::Scene { level: 1, id: 3 };
	c.send_scene(1, 3);
	let ev = pump(&mut [&mut h, &mut c, &mut d], |e| has(&e[0], |x| *x == scene) && has(&e[2], |x| *x == scene));
	assert!(!has(&ev[1], |x| *x == scene));
	h.send_scene(1, 0x4001);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| *x == Event::Scene { level: 1, id: 0x4001 })));
}

#[test]
fn an_object_brought_back_is_no_longer_gone_and_can_go_again() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	let mut d = join(at, "D", 60);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	let k = WorldKey { level: 1, slot: 0x1a5b, class: 660 };
	c.report(k);
	pump(&mut [&mut h, &mut c, &mut d], |e| has(&e[0], |x| *x == Event::Removed(k)) && has(&e[2], |x| *x == Event::Removed(k)));
	// The level sent it again in C's game: everyone hears it is back, C itself not.
	c.back(k);
	let ev = pump(&mut [&mut h, &mut c, &mut d], |e| has(&e[0], |x| *x == Event::Back(k)) && has(&e[2], |x| *x == Event::Back(k)));
	assert!(!has(&ev[1], |x| *x == Event::Back(k)));
	// Its next end is reported anew, and someone joining later is not told it is gone.
	d.report(k);
	pump(&mut [&mut h, &mut c, &mut d], |e| has(&e[0], |x| *x == Event::Removed(k)) && has(&e[1], |x| *x == Event::Removed(k)));
	h.back(k);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| *x == Event::Back(k))));
	let mut late = join(at, "Late", 60);
	let ev = pump(&mut [&mut h, &mut c, &mut d, &mut late], |e| has(&e[3], |x| matches!(x, Event::Connected { .. })));
	// A while more, for anything the host sends a newcomer after the welcome.
	let mut more = Vec::new();
	let end = Instant::now() + Duration::from_millis(300);
	while Instant::now() < end {
		h.poll(Instant::now());
		more.extend(late.poll(Instant::now()));
		std::thread::sleep(Duration::from_millis(2));
	}
	assert!(!has(&ev[3], |x| *x == Event::Removed(k)) && !has(&more, |x| *x == Event::Removed(k)));
}

#[test]
fn a_blow_on_a_switch_reaches_every_other_game() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	let mut d = join(at, "D", 60);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	let key = WorldKey { level: 7, slot: 412, class: 1201 };
	let t = Event::Touch { key, damage: 1.0, mask: 0x1_0000 };
	c.touch(key, 1.0, 0x1_0000);
	let ev = pump(&mut [&mut h, &mut c, &mut d], |e| has(&e[0], |x| *x == t) && has(&e[2], |x| *x == t));
	assert!(!has(&ev[1], |x| *x == t));
	h.touch(key, 1.0, 0x1_0000);
	pump(&mut [&mut h, &mut c, &mut d], |e| has(&e[1], |x| *x == t));
}

#[test]
fn a_client_gives_out_the_enemies_of_its_level() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	let mut d = join(at, "D", 60);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	let (key, owner) = (WorldKey { level: 3, slot: 790, class: 573 }, d.id().unwrap());
	c.assign(key, owner);
	let o = Event::Owner { key, owner };
	let ev = pump(&mut [&mut h, &mut c, &mut d], |e| has(&e[0], |x| *x == o) && has(&e[2], |x| *x == o));
	assert!(!has(&ev[1], |x| *x == o));
	// Whoever joins later hears it too.
	let mut late = join(at, "Late", 60);
	pump(&mut [&mut h, &mut c, &mut d, &mut late], |e| has(&e[3], |x| *x == o));
}

#[test]
fn a_players_own_shot_is_nobodys_copy() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Connected { .. })));
	let now = Instant::now();
	h.report_spawn(1, 0, 305, [5.0, 5.0, 5.0], false, true, now);
	c.report_spawn(1, 0, 305, [5.0, 5.0, 5.0], false, true, now);
	let ev = pump(&mut [&mut h, &mut c], |e| has(&e[0], |x| matches!(x, Event::Spawned { owner: 0, .. })) && has(&e[1], |x| matches!(x, Event::Spawned { owner: 1, local: 1, .. })));
	assert!(!ev.iter().flatten().any(|x| matches!(x, Event::Duplicate { .. })));
}

#[test]
fn spawned_objects_stay_out_of_the_world_list() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Connected { .. })));
	let key = WorldKey { level: 3, slot: SPAWNED_ID + 4, class: 545 };
	c.report(key);
	pump(&mut [&mut h, &mut c], |e| has(&e[0], |x| *x == Event::Removed(key)));
	let Role::Host { world, gone, .. } = &h.role else { unreachable!() };
	assert!(world.is_empty() && gone.is_empty());
}

#[test]
fn clients_are_told_where_the_host_goes() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Connected { .. })));
	h.travel(3);
	h.travel(3);
	let ev = pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| *x == Event::Travel(3)));
	assert_eq!(ev[1].iter().filter(|x| **x == Event::Travel(3)).count(), 1);
	// A client's own word for where the host is does nothing, and someone joining later is told.
	c.travel(5);
	let mut late = join(at, "Late", 60);
	pump(&mut [&mut h, &mut c, &mut late], |e| has(&e[2], |x| *x == Event::Travel(3)));
}

#[test]
fn a_client_that_travels_by_itself_takes_the_team_along() {
	let (mut h, at) = host(60);
	let mut a = join(at, "A", 60);
	let mut b = join(at, "B", 60);
	pump(&mut [&mut h, &mut a, &mut b], |e| has(&e[1], |x| matches!(x, Event::Connected { .. })) && has(&e[2], |x| matches!(x, Event::Connected { .. })));
	h.travel(1);
	pump(&mut [&mut h, &mut a, &mut b], |e| has(&e[1], |x| *x == Event::Travel(1)) && has(&e[2], |x| *x == Event::Travel(1)));
	// A takes a ship to level 3: only the host hears of it, once, and the others are told when the host's game has gone there.
	let id = a.id().unwrap();
	a.went(3);
	let ev = pump(&mut [&mut h, &mut a, &mut b], |e| has(&e[0], |x| *x == Event::Went { id, level: 3 }));
	assert_eq!(ev[0].iter().filter(|x| matches!(x, Event::Went { .. })).count(), 1);
	assert!(!has(&ev[2], |x| matches!(x, Event::Travel(3) | Event::Went { .. })));
	h.travel(3);
	pump(&mut [&mut h, &mut a, &mut b], |e| has(&e[1], |x| *x == Event::Travel(3)) && has(&e[2], |x| *x == Event::Travel(3)));
	// The host's own word for a trip of its own does nothing.
	h.went(5);
	let ev = pump(&mut [&mut h, &mut a, &mut b], |_| true);
	assert!(ev.iter().all(|e| !has(e, |x| matches!(x, Event::Went { .. } | Event::Travel(5)))));
}

#[test]
fn a_report_made_before_the_welcome_goes_out_after_it() {
	let (mut h, at) = host(60);
	let mut c = join(at, "Early", 60);
	c.report(key(3));
	pump(&mut [&mut h, &mut c], |e| has(&e[0], |x| *x == Event::Removed(key(3))));
}

#[test]
fn a_quiet_client_pings_and_stays_in() {
	let (mut h, at) = host(60);
	let mut c = join(at, "Loading", 60);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Connected { .. })));
	// Ten seconds (two timeouts) without a state, as while a level loads: the pings keep it in, on both sides.
	let t0 = Instant::now();
	for k in 1..=20 {
		let t = t0 + Duration::from_millis(500) * k;
		let ce = c.poll(t);
		std::thread::sleep(Duration::from_millis(5));
		let he = h.poll(t);
		assert!(!he.iter().any(|x| matches!(x, Event::Left { .. })), "host at {k}: {he:?}");
		assert!(!ce.contains(&Event::Lost), "client at {k}");
	}
}

#[test]
fn a_join_waits_for_a_host_that_is_still_starting() {
	// Nobody listens on this port yet.
	let free = UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap();
	let start = Instant::now();
	let mut c = Session::join(free, "Early", 60, start).unwrap();
	// The first poll comes long after `join` (the game was still loading): the wait starts there.
	let first = start + CONNECT_TIMEOUT * 2;
	assert!(c.poll(first).is_empty());
	assert!(c.poll(first + TIMEOUT + Duration::from_secs(1)).is_empty());
	assert!(!c.is_over());
	assert_eq!(c.poll(first + CONNECT_TIMEOUT), vec![Event::Lost]);
	assert!(c.is_over());
}

#[test]
fn other_versions_are_told_and_strangers_are_not_let_in() {
	let (mut h, at) = host(60);
	let raw = UdpSocket::bind("127.0.0.1:0").unwrap();
	raw.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
	let mut b = proto::encode(&Msg::Hello { name: "Old".into(), tick_hz: 60 });
	b[4..6].copy_from_slice(&99u16.to_le_bytes());
	raw.send_to(&b, at).unwrap();
	raw.send_to(&proto::encode(&Msg::State { id: 0, hero: hero(1) }), at).unwrap();
	raw.send_to(b"garbage", at).unwrap();
	std::thread::sleep(Duration::from_millis(50));
	assert_eq!(h.poll(Instant::now()), vec![Event::Stranger(raw.local_addr().unwrap())]);
	let mut buf = [0u8; 64];
	let (n, _) = raw.recv_from(&mut buf).unwrap();
	assert_eq!(proto::decode(&buf[..n]), Ok(Msg::Reject(RejectReason::Version)));
	// The stranger's state raises nothing and is answered with Bye (it is not in); the garbage gets no answer.
	let (n, _) = raw.recv_from(&mut buf).unwrap();
	assert_eq!(proto::decode(&buf[..n]), Ok(Msg::Bye));
	raw.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
	assert!(raw.recv_from(&mut buf).is_err());
}

#[test]
fn the_setup_window_finds_a_host_and_who_is_there() {
	let (mut h, at) = host(60);
	let raw = UdpSocket::bind("127.0.0.1:0").unwrap();
	raw.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
	let ask = |h: &mut Session| {
		raw.send_to(&find::query(), at).unwrap();
		std::thread::sleep(Duration::from_millis(30));
		assert!(h.poll(Instant::now()).is_empty(), "a query is no event");
		let mut buf = [0u8; 64];
		let (n, _) = raw.recv_from(&mut buf).unwrap();
		find::read_info(&buf[..n]).unwrap()
	};
	// At the title screen: no level yet.
	assert_eq!(ask(&mut h), find::Info { version: proto::VERSION, tick_hz: 60, players: 1, max: MAX_PLAYERS as u8, level: -1, name: "Host".into() });
	let mut c = join(at, "Raj", 60);
	pump(&mut [&mut h, &mut c], |e| e[1].contains(&Event::Connected { id: 1 }));
	h.travel(3);
	let i = ask(&mut h);
	assert_eq!((i.players, i.level), (2, 3));
	// A host of any version answers the probe with its version, and the probe is not a broken datagram on it.
	raw.send_to(&find::VERSION_PROBE, at).unwrap();
	std::thread::sleep(Duration::from_millis(30));
	assert!(h.poll(Instant::now()).is_empty());
	let mut buf = [0u8; 64];
	let (n, _) = raw.recv_from(&mut buf).unwrap();
	assert_eq!(find::version_answer(&buf[..n]), Some(proto::VERSION));
	assert_eq!(h.stats().undecodable, 0);
	// A client answers neither.
	let ca: SocketAddr = ([127, 0, 0, 1], c.local_addr().unwrap().port()).into();
	raw.send_to(&find::query(), ca).unwrap();
	std::thread::sleep(Duration::from_millis(30));
	c.poll(Instant::now());
	raw.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
	assert!(raw.recv_from(&mut buf).is_err());
}

fn keep(bolts: u32) -> Keep {
	Keep { bolts, owned: [1; crate::proto::KEEP_ITEMS], ammo: [bolts; crate::proto::KEEP_ITEMS] }
}

#[test]
fn a_players_things_are_kept_by_name_and_given_back_when_he_comes_back() {
	let (mut h, at) = host(60);
	let mut c = join(at, "Dutch", 60);
	let mut d = join(at, "D", 60);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	let mine = Event::Keep { name: Name::new("Dutch"), keep: keep(500), live: true };
	c.keep(keep(500));
	pump(&mut [&mut h, &mut c, &mut d], |e| has(&e[0], |x| *x == mine) && has(&e[2], |x| *x == mine));
	h.keep(keep(7));
	let host_keep = Event::Keep { name: Name::new("Host"), keep: keep(7), live: true };
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| *x == host_keep)));
	// Another game's memory of Dutch, older: the host has his own, newer, and keeps it.
	d.remembered(Name::new("Dutch"), keep(1));
	h.keep(keep(8));
	let ev = pump(&mut [&mut h, &mut c, &mut d], |e| has(&e[1], |x| matches!(x, Event::Keep { keep, .. } if keep.bolts == 8)));
	assert!(!ev.iter().flatten().any(|x| matches!(x, Event::Keep { keep, .. } if keep.bolts == 1)));
	// A newcomer named as the host gets no stored copy of the host's things, nor of Dutch's: both play. (A reliable message after
	// the welcome comes after everything a late arrival is told.)
	let mut twin = join(at, "HOST", 60);
	pump(&mut [&mut h, &mut c, &mut d, &mut twin], |e| has(&e[3], |x| matches!(x, Event::Connected { .. })));
	h.progress(Progress::Item(5));
	let ev = pump(&mut [&mut h, &mut c, &mut d, &mut twin], |e| has(&e[3], |x| *x == Event::Story(Progress::Item(5))));
	assert!(!has(&ev[3], |x| matches!(x, Event::Keep { live: false, .. })));
	drop(twin);
	// Dutch's game stopped (silent); he comes back (another case, another address) and gets his things, as a stored copy.
	drop(c);
	std::thread::sleep(KEEP_SILENT + Duration::from_millis(200));
	let mut back = join(at, " dutch", 60);
	pump(&mut [&mut h, &mut back], |e| has(&e[1], |x| matches!(x, Event::Keep { name, keep: k, live: false } if name.as_str() == "Dutch" && k.bolts == 500)));
}

#[test]
fn a_host_that_started_again_gets_its_things_back_from_a_player() {
	let (mut h, _) = host(60);
	let mut c = join(h.local_addr().unwrap(), "C", 60);
	pump(&mut [&mut h, &mut c], |e| has(&e[1], |x| matches!(x, Event::Connected { .. })));
	drop(h);
	let (mut again, at) = host(60);
	let mut c2 = join(at, "C", 60);
	pump(&mut [&mut again, &mut c2], |e| has(&e[1], |x| matches!(x, Event::Connected { .. })));
	c2.remembered(Name::new("host"), keep(4242));
	pump(&mut [&mut again, &mut c2], |e| has(&e[0], |x| matches!(x, Event::Keep { name, keep, live: false } if name.as_str() == "host" && keep.bolts == 4242)));
}

#[test]
fn arcs_reach_everyone_else() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	let mut d = join(at, "D", 60);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	let cid = c.id().unwrap();
	let a = ArcPose { slot: 0x83b, p1: 0, p2: 0, rows: [0.25; 16], end: [1.0, 2.0, 3.0, 1.0] };
	c.send_arcs(0, 77, &[a]);
	let got = |e: &[Event]| has(e, |x| matches!(x, Event::Arcs { id, level: 0, arcs, .. } if *id == cid && arcs[..] == [a]));
	let ev = pump(&mut [&mut h, &mut c, &mut d], |e| got(&e[0]) && got(&e[2]));
	assert!(!has(&ev[1], |x| matches!(x, Event::Arcs { .. })));
}

#[test]
fn particles_and_draws_reach_everyone_else() {
	let (mut h, at) = host(60);
	let mut c = join(at, "C", 60);
	let mut d = join(at, "D", 60);
	pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Connected { .. }))));
	let cid = c.id().unwrap();
	// More particles than one datagram holds go in several.
	let ps: Vec<Particle> = (0..20u16).map(|k| Particle { slot: k, owner: crate::proto::NO_WORD, words: [k as u32; 16] }).collect();
	c.send_particles(2, &ps);
	let draws = Draws { slot: 0x44, calls: vec![crate::proto::DrawCall { queue: 1, func: 0x2e_1c78, code: 9 }], vars: vec![1, 2, 3] };
	c.send_draws(2, 600, &draws);
	let count = |e: &[Event]| e.iter().map(|x| match x { Event::Particles { id, level: 2, particles } if *id == cid => particles.len(), _ => 0 }).sum::<usize>();
	let drew = |e: &[Event]| has(e, |x| matches!(x, Event::Draws { id, level: 2, tick: 600, draws: d } if *id == cid && *d == draws));
	let ev = pump(&mut [&mut h, &mut c, &mut d], |e| count(&e[0]) == 20 && count(&e[2]) == 20 && drew(&e[0]) && drew(&e[2]));
	assert!(!has(&ev[1], |x| matches!(x, Event::Particles { .. } | Event::Draws { .. })));
	// The host's own go to the clients.
	h.send_particles(2, &ps[..3]);
	let ev = pump(&mut [&mut h, &mut c, &mut d], |e| e[1..].iter().all(|ev| has(ev, |x| matches!(x, Event::Particles { id: 0, particles, .. } if particles.len() == 3))));
	assert!(!has(&ev[0], |x| matches!(x, Event::Particles { .. })));
}