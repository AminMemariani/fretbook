//! Song model and a reader for the version 3 and 4 tab file formats (.gp3 / .gp4).
//!
//! Written from public descriptions of the binary layout. All integers are
//! little endian. Text fields are stored in an 8-bit code page, which is guessed
//! per file (Western or Cyrillic).

pub const QUARTER: u32 = 960;

#[derive(Clone, Debug, Default)]
pub struct Song {
    pub version: String,
    pub title: String,
    pub subtitle: String,
    pub artist: String,
    pub album: String,
    pub words: String,
    pub copyright: String,
    pub tab_author: String,
    pub instructions: String,
    pub notice: Vec<String>,
    pub triplet_feel: bool,
    pub lyrics: Vec<(i32, String)>,
    pub lyrics_track: i32,
    pub tempo: i32,
    pub key: i32,
    pub headers: Vec<MeasureHeader>,
    pub tracks: Vec<Track>,
    pub trailing: i64,
}

#[derive(Clone, Debug, Default)]
pub struct MeasureHeader {
    pub start: u32,
    pub numerator: u8,
    pub denominator: u8,
    pub repeat_open: bool,
    /// Number of repeats written on the closing bar (0 = no closing repeat).
    pub repeat_close: i32,
    /// Bit mask of the alternate endings this bar belongs to (bit 0 = ending 1).
    pub alternative: u8,
    pub marker: Option<(String, [u8; 3])>,
    /// Key signature in effect: number of sharps (positive) or flats (negative).
    pub key: i8,
    pub key_minor: bool,
    pub key_changed: bool,
    pub time_changed: bool,
    pub double_bar: bool,
}

impl MeasureHeader {
    pub fn length(&self) -> u32 {
        self.numerator as u32 * (QUARTER * 4 / self.denominator.max(1) as u32)
    }
    pub fn end(&self) -> u32 {
        self.start + self.length()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Channel {
    pub instrument: i32,
    pub volume: u8,
    pub balance: u8,
    #[allow(dead_code)] // read from the file, not used by the app yet
    pub chorus: u8,
    #[allow(dead_code)] // read from the file, not used by the app yet
    pub reverb: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Track {
    pub name: String,
    pub percussion: bool,
    #[allow(dead_code)] // read from the file, not used by the app yet
    pub twelve_string: bool,
    #[allow(dead_code)] // read from the file, not used by the app yet
    pub banjo: bool,
    /// MIDI pitch of each string, string 1 (highest) first.
    pub strings: Vec<i32>,
    pub frets: i32,
    pub capo: i32,
    pub color: [u8; 3],
    #[allow(dead_code)] // read from the file, not used by the app yet
    pub port: i32,
    #[allow(dead_code)] // read from the file, not used by the app yet
    pub channel_index: i32,
    pub channel: Channel,
    pub measures: Vec<Measure>,
}

#[derive(Clone, Debug, Default)]
pub struct Measure {
    pub beats: Vec<Beat>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Duration {
    /// 1 = whole, 2 = half, 4 = quarter, 8 = eighth ...
    pub value: u8,
    pub dotted: bool,
    pub enters: u8,
    pub times: u8,
}

impl Default for Duration {
    fn default() -> Self {
        Duration { value: 4, dotted: false, enters: 1, times: 1 }
    }
}

impl Duration {
    pub fn ticks(&self) -> u32 {
        let mut t = QUARTER * 4 / self.value.max(1) as u32;
        if self.dotted {
            t = t * 3 / 2;
        }
        if self.enters > 1 {
            t = t * self.times as u32 / self.enters as u32;
        }
        t
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum BeatStatus {
    #[default]
    Normal,
    Empty,
    Rest,
}

#[derive(Clone, Debug, Default)]
pub struct Beat {
    pub start: u32,
    pub duration: Duration,
    pub status: BeatStatus,
    pub notes: Vec<Note>,
    pub chord: Option<Chord>,
    pub text: Option<String>,
    pub effects: BeatEffects,
    pub mix: Option<MixChange>,
}

impl Beat {
    pub fn is_rest(&self) -> bool {
        self.status != BeatStatus::Normal || self.notes.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Chord {
    pub name: String,
    pub first_fret: i32,
    /// Fret per string (string 1 first), -1 = not played.
    pub frets: Vec<i32>,
}

#[derive(Clone, Debug, Default)]
pub struct MixChange {
    pub instrument: Option<i32>,
    pub volume: Option<u8>,
    pub balance: Option<u8>,
    pub tempo: Option<i32>,
}

#[derive(Clone, Debug, Default)]
pub struct BeatEffects {
    pub vibrato: bool,
    pub fade_in: bool,
    /// 1 tap, 2 slap, 3 pop
    pub slap: u8,
    pub tremolo_bar: Option<Bend>,
    /// Stroke speed as a duration value (0 = none).
    pub stroke_down: u8,
    pub stroke_up: u8,
    pub pick_stroke: i8,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum NoteKind {
    #[default]
    Normal,
    Tie,
    Dead,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Harmonic {
    Natural,
    Artificial,
    Tapped,
    Pinch,
    Semi,
}

#[derive(Clone, Debug, Default)]
pub struct Bend {
    /// (position 0..=60 across the note, value in 1/100 tone: 100 = full step)
    pub points: Vec<(i32, i32)>,
}

impl Bend {
    pub fn max_value(&self) -> i32 {
        self.points.iter().map(|p| p.1).fold(0, |a, b| if b.abs() > a.abs() { b } else { a })
    }
    /// Pitch offset in semitones at a fraction 0..1 of the note.
    pub fn semitones_at(&self, frac: f32) -> f32 {
        let pos = frac * 60.0;
        if self.points.is_empty() {
            return 0.0;
        }
        let mut prev = self.points[0];
        if pos <= prev.0 as f32 {
            return prev.1 as f32 / 50.0;
        }
        for &p in &self.points[1..] {
            if pos <= p.0 as f32 {
                let span = (p.0 - prev.0).max(1) as f32;
                let t = (pos - prev.0 as f32) / span;
                let v = prev.1 as f32 + (p.1 - prev.1) as f32 * t;
                return v / 50.0;
            }
            prev = p;
        }
        prev.1 as f32 / 50.0
    }
}

#[derive(Clone, Debug, Default)]
pub struct Grace {
    pub fret: i32,
    pub velocity: u8,
    pub duration: u8,
    #[allow(dead_code)] // read from the file, not used by the app yet
    pub transition: i8,
}

#[derive(Clone, Debug, Default)]
pub struct NoteEffects {
    pub bend: Option<Bend>,
    pub grace: Option<Grace>,
    pub hammer: bool,
    pub let_ring: bool,
    pub slide: Option<i8>,
    pub harmonic: Option<Harmonic>,
    pub palm_mute: bool,
    pub staccato: bool,
    pub vibrato: bool,
    /// Tremolo picking speed as a duration value (8, 16, 32).
    pub tremolo_picking: Option<u8>,
    /// (fret, speed as duration value)
    pub trill: Option<(i32, u8)>,
}

#[derive(Clone, Debug, Default)]
pub struct Note {
    pub string: u8,
    pub fret: i32,
    pub kind: NoteKind,
    pub velocity: u8,
    pub ghost: bool,
    pub accent: bool,
    pub left_finger: i8,
    pub right_finger: i8,
    pub effects: NoteEffects,
}

impl Track {
    pub fn pitch_of(&self, note: &Note) -> i32 {
        let s = (note.string as usize).saturating_sub(1).min(self.strings.len().saturating_sub(1));
        let base = self.strings.get(s).copied().unwrap_or(40);
        if self.percussion {
            note.fret
        } else {
            base + note.fret + self.capo
        }
    }
    pub fn is_bass(&self) -> bool {
        !self.percussion && self.strings.iter().copied().min().unwrap_or(40) < 36
    }
}

// ----------------------------------------------------------------------------
// Reader

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    raw_texts: Vec<Vec<u8>>,
}

type R<T> = Result<T, String>;

impl<'a> Reader<'a> {
    fn need(&self, n: usize) -> R<()> {
        if self.pos + n > self.data.len() {
            Err(format!("file ends early (at byte {} of {})", self.pos, self.data.len()))
        } else {
            Ok(())
        }
    }
    fn u8(&mut self) -> R<u8> {
        self.need(1)?;
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }
    fn i8(&mut self) -> R<i8> {
        Ok(self.u8()? as i8)
    }
    fn bool(&mut self) -> R<bool> {
        Ok(self.u8()? != 0)
    }
    fn i32(&mut self) -> R<i32> {
        self.need(4)?;
        let b = &self.data[self.pos..self.pos + 4];
        self.pos += 4;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn skip(&mut self, n: usize) -> R<()> {
        self.need(n)?;
        self.pos += n;
        Ok(())
    }
    fn bytes(&mut self, n: usize) -> R<Vec<u8>> {
        self.need(n)?;
        let v = self.data[self.pos..self.pos + n].to_vec();
        self.pos += n;
        Ok(v)
    }
    /// Keeps the raw bytes; the text is decoded once the code page is known.
    fn text(&mut self, raw: Vec<u8>) -> String {
        let idx = self.raw_texts.len();
        self.raw_texts.push(raw);
        format!("\u{0}{idx}")
    }
    /// One length byte, then `size` bytes of storage.
    fn byte_size_string(&mut self, size: usize) -> R<String> {
        let len = self.u8()? as usize;
        let raw = self.bytes(size)?;
        let raw = raw[..len.min(size)].to_vec();
        Ok(self.text(raw))
    }
    /// A 32-bit length, then the bytes.
    fn int_size_string(&mut self) -> R<String> {
        let len = self.i32()?;
        if !(0..=1 << 20).contains(&len) {
            return Err(format!("bad text length {len} at byte {}", self.pos));
        }
        let raw = self.bytes(len as usize)?;
        Ok(self.text(raw))
    }
    /// A 32-bit storage size, then a length byte and the text.
    fn int_byte_size_string(&mut self) -> R<String> {
        let size = self.i32()?;
        if !(0..=1 << 20).contains(&size) {
            return Err(format!("bad text size {size} at byte {}", self.pos));
        }
        if size == 0 {
            return Ok(String::new());
        }
        let len = self.u8()? as usize;
        let raw = self.bytes(size as usize - 1)?;
        let raw = raw[..len.min(raw.len())].to_vec();
        Ok(self.text(raw))
    }
    fn color(&mut self) -> R<[u8; 3]> {
        let r = self.u8()?;
        let g = self.u8()?;
        let b = self.u8()?;
        self.skip(1)?;
        Ok([r, g, b])
    }
}

fn unpack_velocity(dynamic: i8) -> u8 {
    (15 + 16 * dynamic as i32 - 16).clamp(1, 127) as u8
}

fn tuplet_times(enters: i32) -> (u8, u8) {
    match enters {
        3 => (3, 2),
        5 => (5, 4),
        6 => (6, 4),
        7 => (7, 4),
        9 => (9, 8),
        10 => (10, 8),
        11 => (11, 8),
        12 => (12, 8),
        13 => (13, 8),
        _ => (1, 1),
    }
}

pub fn read_song_file(path: &std::path::Path) -> R<Song> {
    let data = std::fs::read(path).map_err(|e| format!("could not read the file: {e}"))?;
    read_song(&data)
}

pub fn read_song(data: &[u8]) -> R<Song> {
    let mut r = Reader { data, pos: 0, raw_texts: Vec::new() };
    let vlen = r.u8()? as usize;
    let vraw = r.bytes(30)?;
    let version = String::from_utf8_lossy(&vraw[..vlen.min(30)]).to_string();
    let major = if version.contains("v3.") || version.contains("L3.") {
        3
    } else if version.contains("v4.") || version.contains("L4.") || version.contains("CLIPBOARD") {
        4
    } else if version.contains("v5.") {
        return Err("this is a version 5 file, which this version of the app can't read yet".into());
    } else if version.contains("v1.") || version.contains("v2.") {
        return Err("this is a very old version 1/2 file, which isn't supported yet".into());
    } else {
        return Err("this doesn't look like a tab file of version 3 or 4".into());
    };

    let mut song = Song { version: version.clone(), ..Default::default() };
    if version.contains("CLIPBOARD") {
        r.skip(16)?;
    }
    song.title = r.int_byte_size_string()?;
    song.subtitle = r.int_byte_size_string()?;
    song.artist = r.int_byte_size_string()?;
    song.album = r.int_byte_size_string()?;
    song.words = r.int_byte_size_string()?;
    song.copyright = r.int_byte_size_string()?;
    song.tab_author = r.int_byte_size_string()?;
    song.instructions = r.int_byte_size_string()?;
    let notice_count = r.i32()?;
    if !(0..10000).contains(&notice_count) {
        return Err("damaged song notes".into());
    }
    for _ in 0..notice_count {
        let s = r.int_byte_size_string()?;
        song.notice.push(s);
    }
    song.triplet_feel = r.bool()?;
    if major == 4 {
        song.lyrics_track = r.i32()?;
        for _ in 0..5 {
            let start = r.i32()?;
            let text = r.int_size_string()?;
            song.lyrics.push((start, text));
        }
    }
    song.tempo = r.i32()?;
    song.key = r.i32()?;
    if major == 4 {
        r.i8()?; // octave
    }
    // 64 MIDI channels (4 ports x 16)
    let mut channels = Vec::with_capacity(64);
    for _ in 0..64 {
        let instrument = r.i32()?;
        let conv = |v: i8| -> u8 { (((v as i32) << 3) - 1).clamp(-1, 127).max(-1) as u8 + 0 };
        let vol = r.i8()?;
        let bal = r.i8()?;
        let cho = r.i8()?;
        let rev = r.i8()?;
        r.i8()?; // phaser
        r.i8()?; // tremolo
        r.skip(2)?;
        let to127 = |v: i8| -> u8 { (((v as i32) * 8) - 1).clamp(0, 127) as u8 };
        let _ = conv;
        channels.push(Channel {
            instrument,
            volume: to127(vol),
            balance: to127(bal),
            chorus: to127(cho),
            reverb: to127(rev),
        });
    }
    let measure_count = r.i32()?;
    let track_count = r.i32()?;
    if !(0..=10000).contains(&measure_count) || !(0..=128).contains(&track_count) {
        return Err(format!("damaged file ({measure_count} bars, {track_count} tracks)"));
    }

    // Measure headers
    let mut num = 4u8;
    let mut den = 4u8;
    let mut key = song.key.clamp(-7, 7) as i8;
    let mut key_minor = false;
    let mut start = QUARTER;
    for i in 0..measure_count as usize {
        let flags = r.u8()?;
        let mut h = MeasureHeader::default();
        if flags & 0x01 != 0 {
            num = r.i8()?.max(1) as u8;
            h.time_changed = true;
        }
        if flags & 0x02 != 0 {
            den = r.i8()?.max(1) as u8;
            h.time_changed = true;
        }
        if i == 0 {
            h.time_changed = true;
        }
        h.repeat_open = flags & 0x04 != 0;
        if flags & 0x08 != 0 {
            h.repeat_close = r.i8()? as i32;
        }
        if flags & 0x10 != 0 {
            let value = r.u8()?;
            let mut existing = 0u8;
            for prev in song.headers.iter().rev() {
                if prev.repeat_open {
                    break;
                }
                existing |= prev.alternative;
            }
            let mask = if value >= 8 { 0xFF } else { ((1u16 << value) - 1) as u8 };
            h.alternative = mask ^ (mask & existing);
            if h.alternative == 0 {
                h.alternative = 1u8.checked_shl(value.saturating_sub(1) as u32).unwrap_or(0);
            }
        }
        if flags & 0x20 != 0 {
            let name = r.int_byte_size_string()?;
            let color = r.color()?;
            h.marker = Some((name, color));
        }
        if flags & 0x40 != 0 {
            key = r.i8()?.clamp(-7, 7);
            key_minor = r.i8()? != 0;
            h.key_changed = true;
        }
        h.double_bar = flags & 0x80 != 0;
        h.numerator = num;
        h.denominator = den;
        h.key = key;
        h.key_minor = key_minor;
        h.start = start;
        start += h.length();
        song.headers.push(h);
    }

    // Tracks
    for _ in 0..track_count {
        let flags = r.u8()?;
        let name = r.byte_size_string(40)?;
        let string_count = r.i32()?;
        if !(1..=7).contains(&string_count) {
            return Err(format!("damaged track ({string_count} strings)"));
        }
        let mut tuning = Vec::new();
        for s in 0..7 {
            let t = r.i32()?;
            if s < string_count {
                tuning.push(t);
            }
        }
        let port = r.i32()?;
        let channel = r.i32()?;
        let _effect_channel = r.i32()?;
        let frets = r.i32()?;
        let capo = r.i32()?;
        let color = r.color()?;
        let ci = ((port - 1).clamp(0, 3) * 16 + (channel - 1).clamp(0, 15)) as usize;
        let mut ch = channels[ci].clone();
        let percussion = flags & 0x01 != 0 || channel == 10;
        if percussion {
            ch.instrument = 0;
        }
        song.tracks.push(Track {
            name,
            percussion,
            twelve_string: flags & 0x02 != 0,
            banjo: flags & 0x04 != 0,
            strings: tuning,
            frets,
            capo: capo.clamp(0, 30),
            color,
            port,
            channel_index: ci as i32,
            channel: ch,
            measures: Vec::new(),
        });
    }

    // Measures: for every bar, every track
    for m in 0..measure_count as usize {
        let mstart = song.headers[m].start;
        for t in 0..track_count as usize {
            let beat_count = r.i32()?;
            if !(0..=1024).contains(&beat_count) {
                return Err(format!("damaged bar {} ({} beats)", m + 1, beat_count));
            }
            let mut measure = Measure::default();
            let mut pos = mstart;
            for _ in 0..beat_count {
                let mut beat = read_beat(&mut r, major, &song.tracks[t])?;
                beat.start = pos;
                pos += beat.duration.ticks();
                measure.beats.push(beat);
            }
            song.tracks[t].measures.push(measure);
        }
    }

    // Tied notes take the fret of the note they continue.
    for track in &mut song.tracks {
        let mut last: Vec<i32> = vec![0; 8];
        for measure in &mut track.measures {
            for beat in &mut measure.beats {
                for note in &mut beat.notes {
                    let s = (note.string as usize).min(7);
                    if note.kind == NoteKind::Tie {
                        note.fret = last[s];
                    } else {
                        last[s] = note.fret;
                    }
                }
            }
        }
    }

    decode_texts(&mut song, &r.raw_texts);
    song.trailing = data.len() as i64 - r.pos as i64;
    Ok(song)
}

fn read_bend(r: &mut Reader) -> R<Bend> {
    let _kind = r.i8()?;
    let _value = r.i32()?;
    let count = r.i32()?;
    if !(0..=100).contains(&count) {
        return Err("damaged bend".into());
    }
    let mut points = Vec::new();
    for _ in 0..count {
        let pos = r.i32()?;
        let val = r.i32()?;
        let _vib = r.bool()?;
        points.push((pos.clamp(0, 60), val));
    }
    Ok(Bend { points })
}

fn read_beat(r: &mut Reader, major: u8, track: &Track) -> R<Beat> {
    let flags = r.u8()?;
    let mut beat = Beat::default();
    if flags & 0x40 != 0 {
        let status = r.u8()?;
        beat.status = if status == 0 { BeatStatus::Empty } else { BeatStatus::Rest };
    }
    let dv = r.i8()?.clamp(-2, 6);
    let mut duration = Duration { value: 1u8 << (dv + 2), dotted: flags & 0x01 != 0, enters: 1, times: 1 };
    if flags & 0x20 != 0 {
        let enters = r.i32()?;
        let (e, t) = tuplet_times(enters);
        duration.enters = e;
        duration.times = t;
    }
    beat.duration = duration;
    if flags & 0x02 != 0 {
        beat.chord = Some(read_chord(r, major, track.strings.len())?);
    }
    if flags & 0x04 != 0 {
        beat.text = Some(r.int_byte_size_string()?);
    }
    let mut harmonic_all: Option<Harmonic> = None;
    let mut vibrato_all = false;
    if flags & 0x08 != 0 {
        if major == 3 {
            let f1 = r.u8()?;
            vibrato_all = f1 & 0x01 != 0;
            beat.effects.vibrato = f1 & 0x02 != 0;
            beat.effects.fade_in = f1 & 0x10 != 0;
            if f1 & 0x20 != 0 {
                let kind = r.u8()?;
                let value = r.i32()?;
                if kind == 0 {
                    beat.effects.tremolo_bar = Some(Bend { points: vec![(0, 0), (30, value), (60, 0)] });
                } else {
                    beat.effects.slap = kind;
                }
            }
            if f1 & 0x40 != 0 {
                beat.effects.stroke_down = stroke_value(r.i8()?);
                beat.effects.stroke_up = stroke_value(r.i8()?);
            }
            if f1 & 0x04 != 0 {
                harmonic_all = Some(Harmonic::Natural);
            }
            if f1 & 0x08 != 0 {
                harmonic_all = Some(Harmonic::Artificial);
            }
        } else {
            let f1 = r.u8()?;
            let f2 = r.u8()?;
            vibrato_all = f1 & 0x01 != 0;
            beat.effects.vibrato = f1 & 0x02 != 0;
            beat.effects.fade_in = f1 & 0x10 != 0;
            if f1 & 0x20 != 0 {
                beat.effects.slap = r.u8()?;
            }
            if f2 & 0x04 != 0 {
                beat.effects.tremolo_bar = Some(read_bend(r)?);
            }
            if f1 & 0x40 != 0 {
                beat.effects.stroke_down = stroke_value(r.i8()?);
                beat.effects.stroke_up = stroke_value(r.i8()?);
            }
            if f2 & 0x02 != 0 {
                beat.effects.pick_stroke = r.i8()?;
            }
        }
    }
    if flags & 0x10 != 0 {
        beat.mix = Some(read_mix(r, major)?);
    }
    let string_flags = r.u8()?;
    for s in 1..=7u8 {
        if string_flags & (1 << (7 - s)) != 0 && (s as usize) <= track.strings.len() {
            let mut note = read_note(r, major, s)?;
            if note.effects.harmonic.is_none() {
                note.effects.harmonic = harmonic_all;
            }
            note.effects.vibrato |= vibrato_all;
            beat.notes.push(note);
        }
    }
    Ok(beat)
}

fn stroke_value(v: i8) -> u8 {
    match v {
        1 => 128,
        2 => 64,
        3 => 32,
        4 => 16,
        5 => 8,
        6 => 4,
        _ => 0,
    }
}

fn read_chord(r: &mut Reader, major: u8, string_count: usize) -> R<Chord> {
    let mut chord = Chord { frets: vec![-1; string_count], ..Default::default() };
    let new_format = r.u8()? & 0x01 != 0;
    if !new_format {
        chord.name = r.int_byte_size_string()?;
        chord.first_fret = r.i32()?;
        if chord.first_fret != 0 {
            for i in 0..6 {
                let f = r.i32()?;
                if i < string_count {
                    chord.frets[i] = f;
                }
            }
        }
    } else if major == 3 {
        r.skip(25)?;
        chord.name = r.byte_size_string(34)?;
        chord.first_fret = r.i32()?;
        for i in 0..6 {
            let f = r.i32()?;
            if i < string_count {
                chord.frets[i] = f;
            }
        }
        r.skip(36)?;
    } else {
        r.skip(16)?;
        chord.name = r.byte_size_string(21)?;
        r.skip(4)?;
        chord.first_fret = r.i32()?;
        for i in 0..7 {
            let f = r.i32()?;
            if i < string_count {
                chord.frets[i] = f;
            }
        }
        r.skip(32)?;
    }
    Ok(chord)
}

fn read_mix(r: &mut Reader, major: u8) -> R<MixChange> {
    let mut mix = MixChange::default();
    let instrument = r.i8()?;
    let volume = r.i8()?;
    let balance = r.i8()?;
    let chorus = r.i8()?;
    let reverb = r.i8()?;
    let phaser = r.i8()?;
    let tremolo = r.i8()?;
    let tempo = r.i32()?;
    if instrument >= 0 {
        mix.instrument = Some(instrument as i32);
    }
    for v in [volume, balance, chorus, reverb, phaser, tremolo] {
        if v >= 0 {
            r.i8()?;
        }
    }
    if tempo >= 0 {
        r.i8()?;
        mix.tempo = Some(tempo);
    }
    if volume >= 0 {
        mix.volume = Some(((volume as i32) * 8 - 1).clamp(0, 127) as u8);
    }
    if balance >= 0 {
        mix.balance = Some(((balance as i32) * 8 - 1).clamp(0, 127) as u8);
    }
    if major == 4 {
        r.u8()?; // which values apply to every track
    }
    Ok(mix)
}

fn read_note(r: &mut Reader, major: u8, string: u8) -> R<Note> {
    let flags = r.u8()?;
    let mut note = Note { string, velocity: unpack_velocity(6), ..Default::default() };
    note.ghost = flags & 0x04 != 0;
    note.accent = major == 4 && flags & 0x40 != 0;
    if flags & 0x20 != 0 {
        note.kind = match r.u8()? {
            2 => NoteKind::Tie,
            3 => NoteKind::Dead,
            _ => NoteKind::Normal,
        };
    }
    if flags & 0x01 != 0 {
        r.skip(2)?;
    }
    if flags & 0x10 != 0 {
        note.velocity = unpack_velocity(r.i8()?);
    }
    if flags & 0x20 != 0 {
        note.fret = (r.i8()? as i32).clamp(0, 99);
    }
    if flags & 0x80 != 0 {
        note.left_finger = r.i8()?;
        note.right_finger = r.i8()?;
    }
    if flags & 0x08 != 0 {
        if major == 3 {
            let f = r.u8()?;
            note.effects.hammer = f & 0x02 != 0;
            note.effects.let_ring = f & 0x08 != 0;
            if f & 0x01 != 0 {
                note.effects.bend = Some(read_bend(r)?);
            }
            if f & 0x10 != 0 {
                note.effects.grace = Some(read_grace(r)?);
            }
            if f & 0x04 != 0 {
                note.effects.slide = Some(1);
            }
        } else {
            let f1 = r.u8()?;
            let f2 = r.u8()?;
            note.effects.hammer = f1 & 0x02 != 0;
            note.effects.let_ring = f1 & 0x08 != 0;
            if f1 & 0x01 != 0 {
                note.effects.bend = Some(read_bend(r)?);
            }
            if f1 & 0x10 != 0 {
                note.effects.grace = Some(read_grace(r)?);
            }
            if f2 & 0x04 != 0 {
                let v = r.u8()?;
                note.effects.tremolo_picking = Some(match v {
                    1 => 8,
                    2 => 16,
                    _ => 32,
                });
            }
            if f2 & 0x08 != 0 {
                note.effects.slide = Some(r.i8()?);
            }
            if f2 & 0x10 != 0 {
                let h = r.i8()?;
                note.effects.harmonic = Some(match h {
                    1 => Harmonic::Natural,
                    3 => Harmonic::Tapped,
                    4 => Harmonic::Pinch,
                    5 => Harmonic::Semi,
                    _ => Harmonic::Artificial,
                });
            }
            if f2 & 0x20 != 0 {
                let fret = r.i8()? as i32;
                let period = r.i8()?;
                note.effects.trill = Some((fret, match period {
                    1 => 16,
                    2 => 32,
                    _ => 64,
                }));
            }
            note.effects.vibrato = f2 & 0x40 != 0;
            note.effects.palm_mute = f2 & 0x02 != 0;
            note.effects.staccato = f2 & 0x01 != 0;
        }
    }
    Ok(note)
}

fn read_grace(r: &mut Reader) -> R<Grace> {
    let fret = r.u8()? as i32;
    let dynamic = r.u8()?;
    let transition = r.i8()?;
    let dur = r.u8()?;
    Ok(Grace {
        fret: fret.min(99),
        velocity: unpack_velocity(dynamic as i8),
        transition,
        duration: match dur {
            1 => 32,
            2 => 24,
            _ => 16,
        },
    })
}

// ----------------------------------------------------------------------------
// Text decoding

fn looks_cyrillic(texts: &[Vec<u8>]) -> bool {
    let mut cyr_words = 0;
    let mut latin_accent_words = 0;
    for t in texts {
        for word in t.split(|b| !(b.is_ascii_alphabetic() || *b >= 0x80)) {
            let high = word.iter().filter(|b| **b >= 0xC0).count();
            if high == 0 {
                continue;
            }
            if word.len() >= 3 && high * 10 >= word.len() * 7 {
                cyr_words += 1;
            } else {
                latin_accent_words += 1;
            }
        }
    }
    cyr_words > 0 && cyr_words >= latin_accent_words
}

fn decode_texts(song: &mut Song, raws: &[Vec<u8>]) {
    let all_utf8 = raws.iter().all(|r| std::str::from_utf8(r).is_ok());
    let cyrillic = looks_cyrillic(raws);
    let long_runs = raws.iter().any(|r| {
        let mut run = 0;
        r.iter().any(|b| {
            if *b >= 0x80 { run += 1 } else { run = 0 }
            run >= 4
        })
    });
    let gbk = !cyrillic
        && long_runs
        && raws.iter().all(|r| !encoding_rs::GBK.decode_without_bom_handling(r).1);
    let decoded: Vec<String> = raws
        .iter()
        .map(|raw| {
            let s = if all_utf8 {
                String::from_utf8_lossy(raw).to_string()
            } else if gbk {
                encoding_rs::GBK.decode(raw).0.to_string()
            } else if cyrillic {
                encoding_rs::WINDOWS_1251.decode(raw).0.to_string()
            } else {
                encoding_rs::WINDOWS_1252.decode(raw).0.to_string()
            };
            s.replace('\0', "").trim_end().to_string()
        })
        .collect();
    let fix = |s: &mut String| {
        if let Some(rest) = s.strip_prefix('\u{0}') {
            if let Ok(i) = rest.parse::<usize>() {
                *s = decoded.get(i).cloned().unwrap_or_default();
            }
        }
    };
    fix(&mut song.title);
    fix(&mut song.subtitle);
    fix(&mut song.artist);
    fix(&mut song.album);
    fix(&mut song.words);
    fix(&mut song.copyright);
    fix(&mut song.tab_author);
    fix(&mut song.instructions);
    for n in &mut song.notice {
        fix(n);
    }
    for l in &mut song.lyrics {
        fix(&mut l.1);
    }
    for h in &mut song.headers {
        if let Some((name, _)) = &mut h.marker {
            fix(name);
        }
    }
    for t in &mut song.tracks {
        fix(&mut t.name);
        for m in &mut t.measures {
            for b in &mut m.beats {
                if let Some(text) = &mut b.text {
                    fix(text);
                }
                if let Some(c) = &mut b.chord {
                    fix(&mut c.name);
                }
            }
        }
    }
}

pub const NOTE_NAMES_SHARP: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

pub fn pitch_name(p: i32) -> String {
    NOTE_NAMES_SHARP[p.rem_euclid(12) as usize].to_string()
}

pub fn instrument_name(program: i32) -> &'static str {
    const NAMES: [&str; 128] = [
        "Acoustic Grand Piano", "Bright Acoustic Piano", "Electric Grand Piano", "Honky-tonk Piano",
        "Electric Piano 1", "Electric Piano 2", "Harpsichord", "Clavinet", "Celesta", "Glockenspiel",
        "Music Box", "Vibraphone", "Marimba", "Xylophone", "Tubular Bells", "Dulcimer", "Drawbar Organ",
        "Percussive Organ", "Rock Organ", "Church Organ", "Reed Organ", "Accordion", "Harmonica",
        "Tango Accordion", "Nylon Guitar", "Steel Guitar", "Jazz Guitar", "Clean Guitar",
        "Muted Guitar", "Overdrive Guitar", "Distortion Guitar", "Guitar Harmonics", "Acoustic Bass",
        "Fingered Bass", "Picked Bass", "Fretless Bass", "Slap Bass 1", "Slap Bass 2", "Synth Bass 1",
        "Synth Bass 2", "Violin", "Viola", "Cello", "Contrabass", "Tremolo Strings", "Pizzicato Strings",
        "Orchestral Harp", "Timpani", "String Ensemble 1", "String Ensemble 2", "Synth Strings 1",
        "Synth Strings 2", "Choir Aahs", "Voice Oohs", "Synth Voice", "Orchestra Hit", "Trumpet",
        "Trombone", "Tuba", "Muted Trumpet", "French Horn", "Brass Section", "Synth Brass 1",
        "Synth Brass 2", "Soprano Sax", "Alto Sax", "Tenor Sax", "Baritone Sax", "Oboe", "English Horn",
        "Bassoon", "Clarinet", "Piccolo", "Flute", "Recorder", "Pan Flute", "Blown Bottle", "Shakuhachi",
        "Whistle", "Ocarina", "Square Lead", "Saw Lead", "Calliope Lead", "Chiff Lead", "Charang Lead",
        "Voice Lead", "Fifths Lead", "Bass + Lead", "New Age Pad", "Warm Pad", "Polysynth Pad",
        "Choir Pad", "Bowed Pad", "Metallic Pad", "Halo Pad", "Sweep Pad", "Rain", "Soundtrack",
        "Crystal", "Atmosphere", "Brightness", "Goblins", "Echoes", "Sci-fi", "Sitar", "Banjo",
        "Shamisen", "Koto", "Kalimba", "Bagpipe", "Fiddle", "Shanai", "Tinkle Bell", "Agogo",
        "Steel Drums", "Woodblock", "Taiko Drum", "Melodic Tom", "Synth Drum", "Reverse Cymbal",
        "Fret Noise", "Breath Noise", "Seashore", "Bird Tweet", "Telephone Ring", "Helicopter",
        "Applause", "Gunshot",
    ];
    NAMES.get(program.clamp(0, 127) as usize).copied().unwrap_or("Instrument")
}
