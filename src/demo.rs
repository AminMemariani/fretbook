//! A short original piece shown when no file is open.

use crate::gp::*;

fn note(string: u8, fret: i32) -> Note {
    Note { string, fret, velocity: 95, ..Default::default() }
}

fn beat(value: u8, notes: Vec<Note>) -> Beat {
    Beat { duration: Duration { value, ..Default::default() }, notes, ..Default::default() }
}

pub fn demo_song() -> Song {
    let mut song = Song {
        title: "Morning Etude".into(),
        subtitle: "a sample piece — open your own files from the library".into(),
        artist: "Fretbook".into(),
        tempo: 96,
        ..Default::default()
    };
    // chord shapes: (bass string, bass fret, then strings 3,2,1 frets)
    let shapes: [(u8, i32, [i32; 3], &str); 8] = [
        (5, 0, [2, 1, 0], "Am"),
        (5, 0, [2, 1, 0], "Am"),
        (4, 0, [2, 3, 1], "Dm"),
        (6, 0, [1, 0, 0], "E"),
        (5, 3, [0, 1, 0], "C"),
        (6, 3, [0, 0, 3], "G"),
        (6, 0, [1, 0, 0], "E"),
        (5, 0, [2, 1, 0], "Am"),
    ];
    let mut guitar = Track {
        name: "Classical Guitar".into(),
        strings: vec![64, 59, 55, 50, 45, 40],
        frets: 19,
        color: [220, 60, 60],
        channel: Channel { instrument: 24, volume: 104, balance: 64, ..Default::default() },
        ..Default::default()
    };
    let mut bass = Track {
        name: "Bass".into(),
        strings: vec![43, 38, 33, 28],
        frets: 20,
        color: [60, 100, 220],
        channel: Channel { instrument: 33, volume: 90, balance: 64, ..Default::default() },
        ..Default::default()
    };
    let mut start = QUARTER;
    for (i, (bs, bf, top, name)) in shapes.iter().enumerate() {
        let mut h = MeasureHeader { numerator: 4, denominator: 4, start, ..Default::default() };
        if i == 0 {
            h.time_changed = true;
            h.repeat_open = true;
            h.marker = Some(("Intro".into(), [200, 40, 40]));
        }
        if i == 3 {
            h.repeat_close = 1;
        }
        if i == 4 {
            h.marker = Some(("Theme".into(), [200, 40, 40]));
        }
        let mut beats = Vec::new();
        let pattern: [(u8, i32); 8] =
            [(*bs, *bf), (3, top[0]), (2, top[1]), (1, top[2]), (2, top[1]), (3, top[0]), (2, top[1]), (1, top[2])];
        for (k, (s, f)) in pattern.iter().enumerate() {
            let mut b = beat(8, vec![note(*s, *f)]);
            if k == 0 {
                b.chord = Some(Chord { name: name.to_string(), ..Default::default() });
                b.notes[0].effects.let_ring = true;
            }
            if i == 7 && k == 7 {
                b.notes[0].effects.vibrato = true;
            }
            beats.push(b);
        }
        if i == 6 {
            // a little hammer-on figure
            beats[1].notes[0].effects.hammer = true;
            beats[2] = beat(8, vec![note(3, 2)]);
        }
        if i == 7 {
            beats.truncate(4);
            let mut last = beat(2, vec![note(5, 0), note(3, 2), note(2, 1), note(1, 0)]);
            last.effects.stroke_down = 16;
            beats.push(last);
        }
        let mut pos = start;
        for b in &mut beats {
            b.start = pos;
            pos += b.duration.ticks();
        }
        guitar.measures.push(Measure { beats });

        // bass: root on beats 1 and 3
        let root = match *name {
            "Am" => (3, 0),
            "Dm" => (2, 0),
            "E" => (4, 0),
            "C" => (3, 3),
            "G" => (4, 3),
            _ => (3, 0),
        };
        let mut bb = vec![beat(2, vec![note(root.0, root.1)]), beat(4, vec![note(root.0, root.1)]), beat(4, vec![note(root.0, root.1 + 2)])];
        if i == 7 {
            bb = vec![beat(1, vec![note(root.0, root.1)])];
        }
        let mut pos = start;
        for b in &mut bb {
            b.start = pos;
            pos += b.duration.ticks();
        }
        bass.measures.push(Measure { beats: bb });
        start += h.length();
        song.headers.push(h);
    }
    song.tracks.push(guitar);
    song.tracks.push(bass);
    song
}
