//! The players' weapon beams and Swingshot ropes (`effects`): ours to the others, theirs into the snapshot where their puppets are.

use super::*;

/// Weapon quads sent each other field, at most (8 datagrams), and fields a player's are shown after they came.
const QUADS_MAX: usize = 8 * rac_mp::proto::MAX_QUADS;
const QUADS_LAST: u64 = 6;
/// Weapon quads sent and drawn, for the run log.
pub(super) static QUADS_SENT: AtomicU64 = AtomicU64::new(0);
pub(super) static QUADS_SHOWN: AtomicU64 = AtomicU64::new(0);
/// Their corners in 2 mm steps (as far as 65 m from him), their UVs in 1/1024.
const QUAD_STEP: f32 = 0.002;
const UV_STEP: f32 = 1.0 / 1024.0;

/// retail/worker/field_snap.rs, when a field's snapshot is taken: our player's weapon beams (the quads `queue_b::fill` marks his)
/// and Swingshot rope go to the others, relative to where he is, every other field while another player is here; the other
/// players' come into the snapshot's effects, placed where their puppets are. The game itself is not touched.
pub fn effects(fx: &mut Option<crate::retail::RetailFx>) {
	if !ON.load(Ordering::Relaxed) {
		return;
	}
	let Some(fx) = fx.as_mut() else { return };
	let mut g = lock();
	let Some(mp) = g.as_mut().filter(|mp| mp.level >= 0 && mp.mode == 0) else { return };
	let others = mp.shown.iter().any(|(_, h)| h.level == mp.level && on_stage(h.mode));
	// Ours.
	if let (true, Some(me), true) = (others, fx.hero, mp.fields % SEND_EVERY == 0) {
		let q = |v: f32| (v / QUAD_STEP).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
		let u = |v: f32| (v / UV_STEP).round().clamp(i16::MIN as f32, i16::MAX as f32) as i16;
		let c = |v: f32| (v * 128.0).round().clamp(0.0, 255.0) as u8;
		let quads: Vec<FxQuad> = fx
			.weapon
			.iter()
			.flat_map(|(b, r)| if *b { fx.qb.get(r.clone()) } else { fx.qa.get(r.clone()) }.unwrap_or(&[]).iter())
			.take(QUADS_MAX)
			.map(|w| FxQuad {
				pos: w.pos.map(|p| [q(p[0] - me[0]), q(p[1] - me[1]), q(p[2] - me[2])]),
				uv: w.uv.map(|t| [u(t[0]), u(t[1])]),
				colour: w.colour.map(|k| k.map(c)),
				tex: w.tex as u32,
				additive: w.additive,
			})
			.collect();
		let rope = fx.rope.map(|(_, r)| RopePose { hook: [r.hook[0] - me[0], r.hook[1] - me[1], r.hook[2] - me[2]], phase: r.phase, wave: r.wave, amp: r.amp });
		if !quads.is_empty() || rope.is_some() {
			QUADS_SENT.fetch_add(quads.len() as u64 + rope.is_some() as u64, Ordering::Relaxed);
			wire().quads = Some((mp.level as u8, mp.fields as u32, rope, quads));
		}
	}
	// Theirs, where their puppets are.
	let now = mp.fields;
	mp.quads_in.retain(|_, e| now.saturating_sub(e.0) <= QUADS_LAST);
	for (id, (_, _, rope, quads)) in &mp.quads_in {
		let Some((_, h)) = mp.shown.iter().find(|(i, h)| i == id && h.level == mp.level && on_stage(h.mode)) else { continue };
		let at = h.pos;
		QUADS_SHOWN.fetch_add(quads.len() as u64 + rope.is_some() as u64, Ordering::Relaxed);
		for w in quads {
			fx.qb.push(crate::particles::WorldQuad {
				pos: w.pos.map(|p| [at[0] + p[0] as f32 * QUAD_STEP, at[1] + p[1] as f32 * QUAD_STEP, at[2] + p[2] as f32 * QUAD_STEP]),
				uv: w.uv.map(|t| [t[0] as f32 * UV_STEP, t[1] as f32 * UV_STEP]),
				colour: w.colour.map(|k| k.map(|v| v as f32 / 128.0)),
				tex: w.tex as usize,
				additive: w.additive,
			});
		}
		// His rope, from the hook to his hand (about 1 m above his feet), facing up toward where the local camera looks from.
		if let Some(r) = rope {
			let hook = [at[0] + r.hook[0], at[1] + r.hook[1], at[2] + r.hook[2]];
			let cam = fx.hero.map_or([at[0], at[1], at[2] + 4.0], |m| [m[0], m[1], m[2] + 2.0]);
			let s = crate::retail::queue_b::SwingRope { hook, cam, phase: r.phase, wave: r.wave, amp: r.amp };
			fx.qa.extend(crate::retail::queue_b::rope_quads(&s, [at[0], at[1], at[2] + 1.0]));
		}
	}
}
