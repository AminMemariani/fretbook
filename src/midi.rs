//! Writes a Standard MIDI File (type 1) from a song's timeline.

use crate::gp::{Song, QUARTER};
use crate::timing::Timeline;

fn vlq(mut v: u32, out: &mut Vec<u8>) {
    let mut bytes = vec![(v & 0x7F) as u8];
    v >>= 7;
    while v > 0 {
        bytes.push(((v & 0x7F) as u8) | 0x80);
        v >>= 7;
    }
    bytes.reverse();
    out.extend(bytes);
}

fn chunk(events: &mut Vec<(u32, Vec<u8>)>) -> Vec<u8> {
    events.sort_by_key(|e| e.0);
    let mut data = Vec::new();
    let mut last = 0;
    for (t, bytes) in events.iter() {
        vlq(t - last, &mut data);
        data.extend(bytes);
        last = *t;
    }
    data.extend([0x00, 0xFF, 0x2F, 0x00]);
    let mut out = b"MTrk".to_vec();
    out.extend((data.len() as u32).to_be_bytes());
    out.extend(data);
    out
}

pub fn export(song: &Song, tl: &Timeline) -> Vec<u8> {
    // played-order tick offsets
    let mut cum = Vec::with_capacity(tl.played.len());
    let mut acc = 0u32;
    for p in &tl.played {
        cum.push(acc);
        acc += song.headers[p.header].length();
    }
    let sec_to_tick = |sec: f64| -> u32 {
        match tl.played_at(sec) {
            Some(i) => {
                let p = &tl.played[i];
                cum[i] + p.sec_to_tick(song, sec).saturating_sub(song.headers[p.header].start)
            }
            None => 0,
        }
    };

    let mut tracks = Vec::new();
    // tempo and time signatures
    let mut meta: Vec<(u32, Vec<u8>)> = Vec::new();
    for (i, p) in tl.played.iter().enumerate() {
        let mut tempos = vec![(0u32, p.tempo0)];
        tempos.extend(p.changes.iter().copied());
        for (off, bpm) in tempos {
            let us = (60_000_000.0 / bpm.max(1.0)) as u32;
            meta.push((cum[i] + off, vec![0xFF, 0x51, 0x03, (us >> 16) as u8, (us >> 8) as u8, us as u8]));
        }
        let h = &song.headers[p.header];
        if i == 0 || h.time_changed {
            let dd = (h.denominator.max(1) as f32).log2() as u8;
            meta.push((cum[i], vec![0xFF, 0x58, 0x04, h.numerator, dd, 24, 8]));
        }
    }
    let title = song.title.as_bytes();
    let mut name = vec![0xFF, 0x03];
    vlq(title.len() as u32, &mut name);
    name.extend(title);
    meta.push((0, name));
    tracks.push(chunk(&mut meta));

    let mut next_channel = 0u8;
    for (ti, track) in song.tracks.iter().enumerate() {
        let ch = if track.percussion {
            9
        } else {
            let c = next_channel;
            next_channel = (next_channel + 1) % 16;
            if next_channel == 9 {
                next_channel = 10;
            }
            if c == 9 {
                10
            } else {
                c
            }
        };
        let mut ev: Vec<(u32, Vec<u8>)> = Vec::new();
        let tn = track.name.as_bytes();
        let mut name = vec![0xFF, 0x03];
        vlq(tn.len() as u32, &mut name);
        name.extend(tn);
        ev.push((0, name));
        if !track.percussion {
            ev.push((0, vec![0xC0 | ch, track.channel.instrument.clamp(0, 127) as u8]));
        }
        ev.push((0, vec![0xB0 | ch, 7, track.channel.volume.min(127)]));
        ev.push((0, vec![0xB0 | ch, 10, track.channel.balance.min(127)]));
        for e in tl.events.iter().filter(|e| e.track == ti) {
            let t0 = sec_to_tick(e.time);
            let t1 = sec_to_tick(e.time + e.dur).max(t0 + QUARTER / 32);
            let key = (e.pitch.round() as i32).clamp(0, 127) as u8;
            let vel = ((e.velocity * 127.0) as i32).clamp(1, 127) as u8;
            ev.push((t0, vec![0x90 | ch, key, vel]));
            ev.push((t1, vec![0x80 | ch, key, 0]));
        }
        tracks.push(chunk(&mut ev));
    }

    let mut out = b"MThd".to_vec();
    out.extend(6u32.to_be_bytes());
    out.extend(1u16.to_be_bytes());
    out.extend((tracks.len() as u16).to_be_bytes());
    out.extend((QUARTER as u16).to_be_bytes());
    for t in tracks {
        out.extend(t);
    }
    out
}
