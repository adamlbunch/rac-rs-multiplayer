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
	// Six seconds on, with nothing heard from D: the host drops it and D gives the host up.
	let later = Instant::now() + Duration::from_secs(6);
	assert!(h.poll(later).contains(&Event::Left { id }));
	assert!(d.poll(later).contains(&Event::Lost));
	assert!(d.is_over());
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
	// A client's own word for it does nothing, and someone joining later is told.
	c.travel(5);
	let mut late = join(at, "Late", 60);
	pump(&mut [&mut h, &mut c, &mut late], |e| has(&e[2], |x| *x == Event::Travel(3)));
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
fn other_versions_are_told_and_strangers_ignored() {
	let (mut h, at) = host(60);
	let raw = UdpSocket::bind("127.0.0.1:0").unwrap();
	raw.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
	let mut b = proto::encode(&Msg::Hello { name: "Old".into(), tick_hz: 60 });
	b[4..6].copy_from_slice(&99u16.to_le_bytes());
	raw.send_to(&b, at).unwrap();
	raw.send_to(&proto::encode(&Msg::State { id: 0, hero: hero(1) }), at).unwrap();
	raw.send_to(b"garbage", at).unwrap();
	std::thread::sleep(Duration::from_millis(50));
	assert!(h.poll(Instant::now()).is_empty());
	let mut buf = [0u8; 64];
	let (n, _) = raw.recv_from(&mut buf).unwrap();
	assert_eq!(proto::decode(&buf[..n]), Ok(Msg::Reject(RejectReason::Version)));
}
