//! Sessions over real sockets on 127.0.0.1.

use super::*;

fn host(hz: u8) -> (Session, SocketAddr) {
	let h = Session::host("127.0.0.1:0".parse().unwrap(), "Host", hz).unwrap();
	let a = h.local_addr().unwrap();
	(h, a)
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
