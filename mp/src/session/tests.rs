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
