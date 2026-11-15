//! The network thread ("mp-net"): it hands the session what the game left in `WIRE` and leaves what came in for the game, whether
//! the game ticks or not; and the opening of the session RAC_MP asks for.

use super::*;

/// The network thread: sends what the game left in `WIRE`, polls the session every few milliseconds whether the game ticks or
/// not, and leaves what came in for the game. A client that lost the host (`again`: the host, the name, the tick rate) keeps
/// joining again; one the host refused stops.
pub(super) fn run_net(mut s: Session, again: Option<(SocketAddr, String, u8)>) {
	// Whether this game ever got in: a host that never answered is most often one its firewall keeps out.
	let mut ever_in = s.is_host();
	NET.store(true, Ordering::Relaxed);
	loop {
		if QUIT.load(Ordering::Relaxed) {
			s.close();
			eprintln!("mp: at {} leaving: the others are told", clock());
			SAID_BYE.store(true, Ordering::Relaxed);
			return;
		}
		let t0 = Instant::now();
		let touches = std::mem::take(&mut wire().touches);
		let story = std::mem::take(&mut wire().story);
		story.into_iter().for_each(|p| s.progress(p));
		let scenes = std::mem::take(&mut wire().scenes);
		scenes.into_iter().for_each(|(level, id)| s.send_scene(level, id));
		let vars = std::mem::take(&mut wire().vars);
		vars.into_iter().for_each(|(key, state, words)| s.send_vars(key, state, &words));
		touches.into_iter().for_each(|(key, damage, mask)| s.touch(key, damage, mask));
		let (keep, remembered, arcs) = {
			let mut w = wire();
			(w.keep.take(), std::mem::take(&mut w.remembered), w.arcs.take())
		};
		keep.into_iter().for_each(|k| s.keep(k));
		remembered.into_iter().for_each(|(name, k)| s.remembered(name, k));
		if let Some((level, tick, arcs)) = arcs {
			s.send_arcs(level, tick, &arcs);
		}
		let (particles, draws, quads) = {
			let mut w = wire();
			(w.particles.take(), w.draws.take(), w.quads.take())
		};
		if let Some((level, tick, rope, qs)) = quads {
			s.send_quads(level, tick, rope, &qs);
		}
		if let Some((level, ps)) = particles {
			s.send_particles(level, &ps);
		}
		if let Some((level, tick, ds)) = draws {
			ds.iter().for_each(|d| s.send_draws(level, tick, d));
		}
		let (out, gone, level, foes, hits, assign, spawns, fx, machines) = {
			let mut w = wire();
			(w.out.take(), std::mem::take(&mut w.gone), w.level.take(), w.enemies.take(), std::mem::take(&mut w.hits), std::mem::take(&mut w.assign), std::mem::take(&mut w.spawns), w.fx.take(), w.machines.take())
		};
		if let Some(h) = out {
			s.send_state(&h);
		}
		gone.into_iter().for_each(|k| s.report(k));
		std::mem::take(&mut wire().back).into_iter().for_each(|k| s.back(k));
		level.into_iter().for_each(|l| s.travel(l));
		let went = wire().went.take();
		went.into_iter().for_each(|l| s.went(l));
		spawns.into_iter().for_each(|(local, level, class, pos, lasting, solo)| s.report_spawn(local, level, class, pos, lasting, solo, Instant::now()));
		if let Some((level, events)) = fx {
			s.send_fx(level, &events);
		}
		if let Some((level, tick, poses)) = foes {
			s.send_enemies(level, tick, &poses);
		}
		if let Some((level, tick, poses)) = machines {
			s.send_machines(level, tick, &poses);
		}
		hits.into_iter().for_each(|(k, damage, mask)| s.send_hit(k, damage, mask));
		assign.into_iter().for_each(|(k, owner)| s.assign(k, owner));
		let t1 = Instant::now();
		let events = s.poll(t1);
		let t2 = Instant::now();
		// Said here, as it happens (the game may be loading and not read the events for a while), with the time of day: a drop
		// on one PC is matched with the other's log.
		for e in &events {
			let st = s.stats();
			let read = format!("(read {}, undecodable {} (last {:?}), from strangers {})", st.read, st.undecodable, st.last_undecodable, st.from_strangers);
			match e {
				Event::Lost => eprintln!("mp: at {} the host was given up, silent for {:.1} s (0: it said this game is not in) {read}", clock(), s.silent_for(t2).as_secs_f32()),
				Event::Left { id } => eprintln!("mp: at {} player {id} left or went silent {read}", clock()),
				Event::Joined { id, .. } | Event::Connected { id } => eprintln!("mp: at {} player {id} in {read}", clock()),
				Event::Stranger(a) => eprintln!("mp: at {} packets from {a}, which is not a player here: told it is not in {read}", clock()),
				_ => {}
			}
		}
		let refused = events.iter().any(|e| matches!(e, Event::Rejected(_)));
		ever_in |= events.iter().any(|e| matches!(e, Event::Connected { .. }));
		{
			let mut w = wire();
			w.events.extend(events);
			if w.events.len() > WIRE_KEEP {
				// Nobody read them for a long while: the hero states are stale, the joins and leaves still count.
				w.events.retain(|e| !matches!(e, Event::State { .. } | Event::Enemies { .. } | Event::Machines { .. } | Event::Fx { .. }));
			}
			if w.roster.as_slice() != s.roster() {
				w.roster = s.roster().to_vec();
			}
		}
		let t3 = Instant::now();
		if t3 - t0 >= HELD_UP {
			let secs = |a: Instant, b: Instant| (b - a).as_secs_f32();
			eprintln!("mp: the network thread was held up {:.1} s (handing over and sending {:.1} s, reading {:.1} s, handing back {:.1} s)", secs(t0, t3), secs(t0, t1), secs(t1, t2), secs(t2, t3));
		}
		if s.is_over() {
			let Some((host, name, hz)) = again.as_ref().filter(|_| !refused) else { return };
			if !ever_in {
				eprintln!("mp: no answer from {host} in a minute: is a game hosting there, and do that PC's firewall (and its router, over the internet) let UDP port {} in? Trying again", host.port());
				notice::status(Some(format!("No answer from {host}. Is your friend's game hosting? Is Tailscale on for both of you? Still trying ...")), true);
			}
			std::thread::sleep(REJOIN_EVERY);
			if let Ok(n) = Session::join(*host, name, *hz, Instant::now()) {
				s = n;
			}
		}
		let before = Instant::now();
		std::thread::sleep(Duration::from_millis(2));
		let slept = before.elapsed();
		if slept >= HELD_UP {
			eprintln!("mp: the network thread did not run for {:.1} s (this machine was too busy or stalled)", slept.as_secs_f32());
		}
	}
}

/// The time of day (UTC) as HH:MM:SS.mmm, for the network lines of two PCs' logs to be put side by side.
fn clock() -> String {
	let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis()) % 86_400_000;
	format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

/// The session RAC_MP asks for, and the host's address when it is a join.
pub(super) fn open(spec: &str, name: &str, hz: u8) -> Result<(Session, Option<SocketAddr>), String> {
	let spec = spec.trim();
	if spec == "host" || spec.starts_with("host:") {
		// host, host:PORT (every network), or host:ADDR:PORT (one address: 127.0.0.1 keeps it to this PC).
		let bind: SocketAddr = match spec.strip_prefix("host:") {
			None => ([0, 0, 0, 0], rac_mp::DEFAULT_PORT).into(),
			Some(p) if !p.contains(':') => ([0, 0, 0, 0], p.parse::<u16>().map_err(|_| format!("bad port {p:?}"))?).into(),
			Some(a) => a.parse().map_err(|_| format!("bad address {a:?}"))?,
		};
		return Session::host(bind, name, hz).map(|s| (s, None)).map_err(|e| e.to_string());
	}
	if let Some(at) = spec.strip_prefix("join:") {
		let at = if at.contains(':') { at.to_string() } else { format!("{at}:{}", rac_mp::DEFAULT_PORT) };
		// The host listens on IPv4 (0.0.0.0): a name with both kinds of address (Tailscale's MagicDNS names have both) is joined at
		// its IPv4 one, whichever the system lists first.
		let mut addrs: Vec<SocketAddr> = at.to_socket_addrs().map_err(|e| e.to_string())?.collect();
		addrs.sort_by_key(|a| !a.is_ipv4());
		let addr = addrs.first().copied().ok_or_else(|| format!("no address for {at:?}"))?;
		return Session::join(addr, name, hz, Instant::now()).map(|s| (s, Some(addr))).map_err(|e| e.to_string());
	}
	Err("expected host, host:PORT, host:ADDR:PORT, join:ADDR, join:ADDR:PORT, mirror or mirror:FIELDS".into())
}
