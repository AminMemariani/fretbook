//! Turns a song into a timeline: the order bars are played in (repeats and
//! alternate endings), the time of every tick, and the list of sounding notes.

use crate::gp::{self, Harmonic, NoteKind, Song, QUARTER};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Played {
    pub header: usize,
    pub start_sec: f64,
    pub end_sec: f64,
    /// Tempo at the bar start, then (tick offset, bpm) changes inside it.
    pub tempo0: f64,
    pub changes: Vec<(u32, f64)>,
}

impl Played {
    pub fn tick_to_sec(&self, song: &Song, tick: u32) -> f64 {
        let h = &song.headers[self.header];
        let off = tick.saturating_sub(h.start);
        let mut t = self.start_sec;
        let mut last = 0u32;
        let mut bpm = self.tempo0;
        for &(ct, b) in &self.changes {
            if ct >= off {
                break;
            }
            t += (ct - last) as f64 / QUARTER as f64 * 60.0 / bpm;
            last = ct;
            bpm = b;
        }
        t + (off - last) as f64 / QUARTER as f64 * 60.0 / bpm
    }
    pub fn sec_to_tick(&self, song: &Song, sec: f64) -> u32 {
        let h = &song.headers[self.header];
        let mut t = self.start_sec;
        let mut last = 0u32;
        let mut bpm = self.tempo0;
        for &(ct, b) in &self.changes {
            let seg = (ct - last) as f64 / QUARTER as f64 * 60.0 / bpm;
            if t + seg > sec {
                break;
            }
            t += seg;
            last = ct;
            bpm = b;
        }
        let ticks = ((sec - t).max(0.0) * bpm / 60.0 * QUARTER as f64) as u32;
        h.start + last + ticks
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sound {
    Nylon,
    Steel,
    Electric,
    Overdrive,
    Bass,
    Piano,
    Organ,
    Strings,
    Brass,
    Lead,
    Drums,
}

pub fn sound_for(program: i32, percussion: bool) -> Sound {
    if percussion {
        return Sound::Drums;
    }
    match program {
        0..=15 => Sound::Piano,
        16..=23 => Sound::Organ,
        24 => Sound::Nylon,
        25 | 31 => Sound::Steel,
        26..=28 => Sound::Electric,
        29 | 30 => Sound::Overdrive,
        32..=39 => Sound::Bass,
        40..=55 => Sound::Strings,
        56..=71 => Sound::Brass,
        72..=79 => Sound::Lead,
        80..=103 => Sound::Lead,
        104..=111 => Sound::Steel,
        _ => Sound::Piano,
    }
}

#[derive(Clone, Debug)]
pub struct NoteEvent {
    pub time: f64,
    pub dur: f64,
    pub track: usize,
    pub string: u8,
    pub pitch: f32,
    pub velocity: f32,
    pub sound: Sound,
    pub palm_mute: bool,
    pub dead: bool,
    pub vibrato: bool,
    pub hammer: bool,
    pub bend: Option<Arc<gp::Bend>>,
    /// Semitones to glide to by the end of the note.
    pub slide_to: f32,
    pub slide: bool,
    pub pan: f32,
    pub volume: f32,
    /// MIDI instrument number, for the sampled sound bank.
    pub program: i32,
    pub harmonic: bool,
}

pub struct Timeline {
    pub played: Vec<Played>,
    pub events: Vec<NoteEvent>,
    /// (time, accented) metronome clicks.
    pub clicks: Vec<(f64, bool)>,
    pub total: f64,
}

impl Timeline {
    /// Index into `played` for a time.
    pub fn played_at(&self, sec: f64) -> Option<usize> {
        if self.played.is_empty() {
            return None;
        }
        let i = self.played.partition_point(|p| p.start_sec <= sec);
        Some(i.saturating_sub(1).min(self.played.len() - 1))
    }
    /// (bar, tick) under a time.
    pub fn locate(&self, song: &Song, sec: f64) -> Option<(usize, u32)> {
        let p = &self.played[self.played_at(sec)?];
        Some((p.header, p.sec_to_tick(song, sec)))
    }
    /// First time a bar/tick is reached.
    pub fn time_of(&self, song: &Song, header: usize, tick: u32) -> f64 {
        self.played
            .iter()
            .find(|p| p.header == header)
            .map(|p| p.tick_to_sec(song, tick))
            .unwrap_or(0.0)
    }
    pub fn bar_end_time(&self, header: usize) -> f64 {
        self.played.iter().find(|p| p.header == header).map(|p| p.end_sec).unwrap_or(self.total)
    }
}

/// The bar order with repeats and alternate endings unfolded.
pub fn play_order(song: &Song) -> Vec<usize> {
    let n = song.headers.len();
    let mut order = Vec::new();
    let mut i = 0usize;
    let mut repeat_start = 0usize;
    let mut pass = 1u32;
    let mut jumped = false;
    let mut count = vec![0i32; n];
    while i < n && order.len() < 20000 {
        let h = &song.headers[i];
        if h.repeat_open && !jumped {
            repeat_start = i;
            pass = 1;
        }
        jumped = false;
        if h.alternative != 0 && (pass > 8 || h.alternative & (1u8 << (pass - 1)) == 0) {
            i += 1;
            continue;
        }
        order.push(i);
        if h.repeat_close > 0 {
            if count[i] < h.repeat_close {
                count[i] += 1;
                pass += 1;
                i = repeat_start;
                jumped = true;
                continue;
            }
            count[i] = 0;
            pass = 1;
            repeat_start = i + 1;
        }
        i += 1;
    }
    order
}

fn harmonic_pitch(open: i32, fret: i32, h: Harmonic) -> i32 {
    match h {
        Harmonic::Natural => match fret {
            12 => open + 12,
            7 | 19 => open + 19,
            5 | 24 => open + 24,
            4 | 9 | 16 => open + 28,
            3 => open + 31,
            _ => open + fret + 12,
        },
        _ => open + fret + 12,
    }
}

pub fn build(song: &Song) -> Timeline {
    let order = play_order(song);
    // Tempo changes by bar: (tick offset, bpm)
    let mut bar_changes: Vec<Vec<(u32, f64)>> = vec![Vec::new(); song.headers.len()];
    for t in &song.tracks {
        for (mi, m) in t.measures.iter().enumerate() {
            for b in &m.beats {
                if let Some(mix) = &b.mix {
                    if let Some(tempo) = mix.tempo {
                        if tempo > 0 {
                            let off = b.start.saturating_sub(song.headers[mi].start);
                            bar_changes[mi].push((off, tempo as f64));
                        }
                    }
                }
            }
        }
    }
    for c in &mut bar_changes {
        c.sort_by_key(|x| x.0);
        c.dedup_by_key(|x| x.0);
    }

    let mut played = Vec::with_capacity(order.len());
    let mut sec = 0.0f64;
    let mut bpm = (song.tempo.clamp(20, 400)) as f64;
    for &hi in &order {
        let h = &song.headers[hi];
        let mut p = Played { header: hi, start_sec: sec, end_sec: 0.0, tempo0: bpm, changes: Vec::new() };
        // a change exactly at the bar start replaces the starting tempo
        for &(off, b) in &bar_changes[hi] {
            if off == 0 {
                p.tempo0 = b;
            } else if off < h.length() {
                p.changes.push((off, b));
            }
        }
        p.end_sec = p.tick_to_sec(song, h.end());
        bpm = p.changes.last().map(|c| c.1).unwrap_or(p.tempo0);
        sec = p.end_sec;
        played.push(p);
    }

    let mut clicks = Vec::new();
    for p in &played {
        let h = &song.headers[p.header];
        let step = QUARTER * 4 / h.denominator.max(1) as u32;
        let compound = h.denominator == 8 && h.numerator % 3 == 0 && h.numerator > 3;
        for k in 0..h.numerator as u32 {
            if compound && k % 3 != 0 {
                continue;
            }
            clicks.push((p.tick_to_sec(song, h.start + k * step), k == 0));
        }
    }

    let mut events: Vec<NoteEvent> = Vec::new();
    for (ti, track) in song.tracks.iter().enumerate() {
        let mut program = track.channel.instrument;
        let mut volume = track.channel.volume as f32 / 127.0;
        let mut pan = (track.channel.balance as f32 - 64.0) / 64.0;
        // last event per string, for ties and slides
        let mut last: [Option<usize>; 8] = [None; 8];
        for p in &played {
            let Some(measure) = track.measures.get(p.header) else { continue };
            let h = &song.headers[p.header];
            for beat in &measure.beats {
                if let Some(mix) = &beat.mix {
                    if let Some(i) = mix.instrument {
                        program = i;
                    }
                    if let Some(v) = mix.volume {
                        volume = v as f32 / 127.0;
                    }
                    if let Some(b) = mix.balance {
                        pan = (b as f32 - 64.0) / 64.0;
                    }
                }
                if beat.is_rest() {
                    continue;
                }
                let start_tick = beat.start.min(h.end());
                let end_tick = (beat.start + beat.duration.ticks()).min(h.end().max(start_tick + 1));
                let t0 = p.tick_to_sec(song, start_tick);
                let t1 = p.tick_to_sec(song, end_tick).max(t0 + 0.01);
                let beat_dur = t1 - t0;
                let sound = sound_for(program, track.percussion);
                let swing = song.triplet_feel && beat.duration.value == 8 && beat.duration.enters == 1 && !beat.duration.dotted;
                let (t0, beat_dur) = if swing {
                    let in_quarter = (beat.start - h.start) % QUARTER;
                    if in_quarter == 0 {
                        (t0, beat_dur * 4.0 / 3.0)
                    } else {
                        (t0 + beat_dur / 3.0, beat_dur * 2.0 / 3.0)
                    }
                } else {
                    (t0, beat_dur)
                };
                let n_notes = beat.notes.len();
                let stroke = if beat.effects.stroke_down > 0 || beat.effects.stroke_up > 0 {
                    (beat_dur * 0.5).min(0.12) / n_notes.max(1) as f64
                } else {
                    0.0
                };
                let mut sorted: Vec<&gp::Note> = beat.notes.iter().collect();
                if beat.effects.stroke_up > 0 {
                    sorted.sort_by_key(|n| n.string);
                } else {
                    sorted.sort_by_key(|n| std::cmp::Reverse(n.string));
                }
                for (k, note) in sorted.into_iter().enumerate() {
                    let s = (note.string as usize).min(7);
                    if note.kind == NoteKind::Tie {
                        if let Some(li) = last[s] {
                            let e = &mut events[li];
                            e.dur = (t0 + beat_dur - e.time).max(e.dur);
                            continue;
                        }
                    }
                    let open = track.strings.get(s.saturating_sub(1)).copied().unwrap_or(40) + track.capo;
                    let mut pitch = if track.percussion { note.fret } else { open + note.fret };
                    if let Some(hm) = note.effects.harmonic {
                        if !track.percussion {
                            pitch = harmonic_pitch(open, note.fret, hm);
                        }
                    }
                    let mut vel = note.velocity as f32 / 127.0;
                    if note.ghost {
                        vel *= 0.55;
                    }
                    if note.accent {
                        vel = (vel * 1.25).min(1.0);
                    }
                    let mut dur = beat_dur;
                    if note.effects.staccato {
                        dur *= 0.5;
                    }
                    if note.effects.let_ring {
                        dur = dur.max(beat_dur * 4.0).max(2.0);
                    }
                    let dead = note.kind == NoteKind::Dead;
                    if dead {
                        dur = dur.min(0.08);
                    }
                    let time = t0 + stroke * k as f64;
                    if let Some(g) = &note.effects.grace {
                        let gd = (60.0 / p.tempo0) * 4.0 / g.duration.max(1) as f64;
                        events.push(NoteEvent {
                            time: (time - gd).max(0.0),
                            dur: gd,
                            track: ti,
                            string: note.string,
                            pitch: (open + g.fret) as f32,
                            velocity: g.velocity as f32 / 127.0,
                            sound,
                            palm_mute: false,
                            dead: g.fret < 0,
                            vibrato: false,
                            hammer: false,
                            bend: None,
                            slide_to: 0.0,
                            slide: false,
                            pan,
                            volume,
                            program,
                            harmonic: false,
                        });
                    }
                    let bend = note
                        .effects
                        .bend
                        .clone()
                        .or_else(|| beat.effects.tremolo_bar.clone())
                        .map(Arc::new);
                    let base = NoteEvent {
                        time,
                        dur,
                        track: ti,
                        string: note.string,
                        pitch: pitch as f32,
                        velocity: vel,
                        sound,
                        palm_mute: note.effects.palm_mute,
                        dead,
                        vibrato: note.effects.vibrato || beat.effects.vibrato,
                        hammer: note.effects.hammer,
                        bend,
                        slide_to: 0.0,
                        slide: note.effects.slide.is_some(),
                        pan,
                        volume,
                        program,
                        harmonic: note.effects.harmonic.is_some(),
                    };
                    // the note before a hammer-on/pull-off: mark this one soft
                    let prev_hammer = last[s].map(|li| events[li].hammer).unwrap_or(false);
                    let mut base = base;
                    if prev_hammer {
                        base.velocity *= 0.7;
                    }
                    if let Some(speed) = note.effects.tremolo_picking {
                        let step = (60.0 / p.tempo0) * 4.0 / speed as f64;
                        let mut t = time;
                        while t < time + dur - step * 0.5 {
                            let mut e = base.clone();
                            e.time = t;
                            e.dur = step;
                            events.push(e);
                            t += step;
                        }
                        last[s] = Some(events.len() - 1);
                        continue;
                    }
                    if let Some((tf, speed)) = note.effects.trill {
                        let step = (60.0 / p.tempo0) * 4.0 / speed as f64;
                        let mut t = time;
                        let mut alt = false;
                        while t < time + dur - step * 0.5 {
                            let mut e = base.clone();
                            e.time = t;
                            e.dur = step;
                            if alt {
                                e.pitch = (open + tf) as f32;
                                e.velocity *= 0.8;
                            }
                            events.push(e);
                            alt = !alt;
                            t += step;
                        }
                        last[s] = Some(events.len() - 1);
                        continue;
                    }
                    if let Some(li) = last[s] {
                        if events[li].slide && events[li].slide_to == 0.0 && time - (events[li].time + events[li].dur) < 0.05 {
                            events[li].slide_to = base.pitch - events[li].pitch;
                        }
                    }
                    events.push(base);
                    last[s] = Some(events.len() - 1);
                }
            }
        }
    }
    events.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));
    let total = played.last().map(|p| p.end_sec).unwrap_or(0.0);
    Timeline { played, events, clicks, total }
}
