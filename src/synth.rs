//! A small built-in synthesizer and sequencer. Plucked strings use the
//! Karplus-Strong method; drums and other instruments are simple synthesis.

use crate::timing::{NoteEvent, Sound, Timeline};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

pub enum Cmd {
    Load(Arc<Vec<NoteEvent>>, Arc<Vec<(f64, bool)>>, f64),
    Play(f64),
    Stop,
    Seek(f64),
    Speed(f32),
    Loop(Option<(f64, f64)>),
    TrackGain(Vec<f32>),
    Metronome(bool),
    CountIn(bool),
    Preview(NoteEvent),
    Sampler(Box<Sampler>),
    UseSampler(bool),
}

pub struct Player {
    tx: Sender<Cmd>,
    pub position: Arc<AtomicU64>,
    pub playing: Arc<AtomicBool>,
    _stream: Option<cpal::Stream>,
    pub error: Option<String>,
}

impl Player {
    pub fn new() -> Player {
        let (tx, rx) = channel();
        let position = Arc::new(AtomicU64::new(0f64.to_bits()));
        let playing = Arc::new(AtomicBool::new(false));
        let mut error = None;
        let stream = match start_stream(rx, position.clone(), playing.clone()) {
            Ok((s, sr)) => {
                let tx2 = tx.clone();
                std::thread::spawn(move || {
                    if let Some(sm) = Sampler::load(sr) {
                        let _ = tx2.send(Cmd::Sampler(Box::new(sm)));
                    }
                });
                Some(s)
            }
            Err(e) => {
                error = Some(e);
                None
            }
        };
        Player { tx, position, playing, _stream: stream, error }
    }
    pub fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }
    pub fn position(&self) -> f64 {
        f64::from_bits(self.position.load(Ordering::Relaxed))
    }
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }
    pub fn load(&self, tl: &Timeline) {
        self.send(Cmd::Load(Arc::new(tl.events.clone()), Arc::new(tl.clicks.clone()), tl.total));
    }
}

/// Renders a timeline without a sound device (used for checks). Returns stereo samples.
pub fn render_offline(tl: &Timeline, secs: f64, sr: f32, sampled: bool) -> Vec<f32> {
    let (tx, rx) = channel();
    let mut e = Engine::new(sr, rx, Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)));
    if sampled {
        if let Some(sm) = Sampler::load(sr) {
            tx.send(Cmd::Sampler(Box::new(sm))).ok();
        }
    }
    tx.send(Cmd::Load(Arc::new(tl.events.clone()), Arc::new(tl.clicks.clone()), tl.total)).ok();
    tx.send(Cmd::Play(0.0)).ok();
    let frames = (secs * sr as f64) as usize;
    let mut out = vec![0.0f32; frames * 2];
    for chunk in out.chunks_mut(1024) {
        e.render(chunk, 2);
    }
    out
}

fn start_stream(rx: Receiver<Cmd>, position: Arc<AtomicU64>, playing: Arc<AtomicBool>) -> Result<(cpal::Stream, f32), String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no sound output found")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let mut config: cpal::StreamConfig = supported.config();
    config.buffer_size = cpal::BufferSize::Default;
    let channels = config.channels as usize;
    let sr = config.sample_rate as f32;
    let mut engine = Engine::new(sr, rx, position, playing);
    let stream = device
        .build_output_stream::<f32, _, _>(
            config,
            move |data: &mut [f32], _| engine.render(data, channels),
            |e| eprintln!("audio error: {e}"),
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, sr))
}

// ----------------------------------------------------------------------------

const BUF: usize = 4096;
const VOICES: usize = 72;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Pluck,
    Osc,
    Drum,
    Click,
}

struct Voice {
    active: bool,
    kind: Kind,
    sound: Sound,
    track: usize,
    string: u8,
    t: f32,
    dur: f32,
    release: f32,
    pitch: f32,
    vel: f32,
    gain_l: f32,
    gain_r: f32,
    buf: Vec<f32>,
    w: usize,
    loss: f32,
    lp: f32,
    lp_coef: f32,
    phase: f32,
    phase2: f32,
    drum: u8,
    noise: u32,
    hp: f32,
    bp1: f32,
    bp2: f32,
    ev: Option<NoteEvent>,
    env: f32,
    drive: f32,
}

impl Voice {
    fn new() -> Voice {
        Voice {
            active: false,
            kind: Kind::Pluck,
            sound: Sound::Nylon,
            track: 0,
            string: 0,
            t: 0.0,
            dur: 0.0,
            release: 0.0,
            pitch: 60.0,
            vel: 1.0,
            gain_l: 0.7,
            gain_r: 0.7,
            buf: vec![0.0; BUF],
            w: 0,
            loss: 0.996,
            lp: 0.0,
            lp_coef: 0.5,
            phase: 0.0,
            phase2: 0.0,
            drum: 0,
            noise: 12345,
            hp: 0.0,
            bp1: 0.0,
            bp2: 0.0,
            ev: None,
            env: 1.0,
            drive: 0.0,
        }
    }
    fn rnd(&mut self) -> f32 {
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        (self.noise as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

fn midi_hz(p: f32) -> f32 {
    440.0 * 2f32.powf((p - 69.0) / 12.0)
}

struct Comb {
    buf: Vec<f32>,
    i: usize,
    fb: f32,
    damp: f32,
    store: f32,
}
impl Comb {
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.buf[self.i];
        self.store = y * (1.0 - self.damp) + self.store * self.damp;
        self.buf[self.i] = x + self.store * self.fb;
        self.i = (self.i + 1) % self.buf.len();
        y
    }
}
struct Allpass {
    buf: Vec<f32>,
    i: usize,
}
impl Allpass {
    fn tick(&mut self, x: f32) -> f32 {
        let b = self.buf[self.i];
        let y = -x + b;
        self.buf[self.i] = x + b * 0.5;
        self.i = (self.i + 1) % self.buf.len();
        y
    }
}

struct Engine {
    sr: f32,
    rx: Receiver<Cmd>,
    position: Arc<AtomicU64>,
    playing_flag: Arc<AtomicBool>,
    events: Arc<Vec<NoteEvent>>,
    clicks: Arc<Vec<(f64, bool)>>,
    total: f64,
    playing: bool,
    pos: f64,
    next: usize,
    next_click: usize,
    speed: f64,
    looping: Option<(f64, f64)>,
    gains: Vec<f32>,
    metronome: bool,
    count_in: bool,
    count_in_left: f64,
    count_in_beat: f64,
    count_in_clicks: u32,
    master: f32,
    voices: Vec<Voice>,
    seed: u32,
    combs: Vec<Comb>,
    allpasses: Vec<Allpass>,
    sampler: Option<Box<Sampler>>,
    use_sampler: bool,
    wall: f64,
}

impl Engine {
    fn new(sr: f32, rx: Receiver<Cmd>, position: Arc<AtomicU64>, playing_flag: Arc<AtomicBool>) -> Engine {
        let scale = sr / 44100.0;
        let combs = [1116, 1188, 1277, 1356]
            .iter()
            .map(|&l| Comb { buf: vec![0.0; (l as f32 * scale) as usize], i: 0, fb: 0.78, damp: 0.3, store: 0.0 })
            .collect();
        let allpasses = [556, 441].iter().map(|&l| Allpass { buf: vec![0.0; (l as f32 * scale) as usize], i: 0 }).collect();
        Engine {
            sr,
            rx,
            position,
            playing_flag,
            events: Arc::new(Vec::new()),
            clicks: Arc::new(Vec::new()),
            total: 0.0,
            playing: false,
            pos: 0.0,
            next: 0,
            next_click: 0,
            speed: 1.0,
            looping: None,
            gains: Vec::new(),
            metronome: false,
            count_in: false,
            count_in_left: 0.0,
            count_in_beat: 0.5,
            count_in_clicks: 0,
            master: 0.8,
            voices: (0..VOICES).map(|_| Voice::new()).collect(),
            seed: 987654321,
            combs,
            allpasses,
            sampler: None,
            use_sampler: true,
            wall: 0.0,
        }
    }

    fn seek(&mut self, t: f64) {
        self.pos = t.max(0.0);
        self.next = self.events.partition_point(|e| e.time < self.pos - 1e-6);
        self.next_click = self.clicks.partition_point(|c| c.0 < self.pos - 1e-6);
        self.position.store(self.pos.to_bits(), Ordering::Relaxed);
    }

    fn release_all(&mut self) {
        if let Some(sm) = &mut self.sampler {
            sm.all_off();
        }
        for v in &mut self.voices {
            if v.active && v.kind != Kind::Click {
                v.dur = v.dur.min(v.t);
            }
        }
    }

    fn commands(&mut self) {
        while let Ok(c) = self.rx.try_recv() {
            match c {
                Cmd::Load(ev, clicks, total) => {
                    self.events = ev;
                    self.clicks = clicks;
                    self.total = total;
                    self.playing = false;
                    self.release_all();
                    self.seek(0.0);
                }
                Cmd::Play(t) => {
                    self.release_all();
                    self.seek(t);
                    self.playing = true;
                    if self.count_in {
                        // one bar of four clicks at the tempo of the first beat
                        let beat = self
                            .clicks
                            .get(self.next_click)
                            .and_then(|c| self.clicks.get(self.next_click + 1).map(|d| d.0 - c.0))
                            .unwrap_or(0.5)
                            .clamp(0.15, 2.0);
                        self.count_in_beat = beat;
                        self.count_in_clicks = 4;
                        self.count_in_left = 0.0;
                    }
                }
                Cmd::Stop => {
                    self.playing = false;
                    self.count_in_clicks = 0;
                    self.release_all();
                }
                Cmd::Seek(t) => {
                    self.release_all();
                    self.seek(t);
                }
                Cmd::Speed(s) => self.speed = s.clamp(0.1, 4.0) as f64,
                Cmd::Loop(l) => self.looping = l,
                Cmd::TrackGain(g) => self.gains = g,
                Cmd::Metronome(m) => self.metronome = m,
                Cmd::CountIn(c) => self.count_in = c,
                Cmd::Preview(e) => self.start_voice(&e),
                Cmd::Sampler(sm) => self.sampler = Some(sm),
                Cmd::UseSampler(u) => {
                    self.release_all();
                    self.use_sampler = u;
                }
            }
        }
        self.playing_flag.store(self.playing, Ordering::Relaxed);
    }

    fn free_voice(&mut self, track: usize, string: u8, drum: bool) -> usize {
        // Same string on the same track: a guitar string can only sound one note.
        if !drum {
            if let Some(i) = self
                .voices
                .iter()
                .position(|v| v.active && v.kind != Kind::Click && v.kind != Kind::Drum && v.track == track && v.string == string)
            {
                let v = &mut self.voices[i];
                v.dur = v.dur.min(v.t);
            }
        }
        if let Some(i) = self.voices.iter().position(|v| !v.active) {
            return i;
        }
        // steal the quietest / oldest
        let mut best = 0;
        let mut score = f32::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            let s = v.env * v.vel - v.t * 0.01;
            if s < score {
                score = s;
                best = i;
            }
        }
        best
    }

    fn click(&mut self, accent: bool) {
        if self.use_sampler {
            if let Some(sm) = &mut self.sampler {
                sm.click(accent, self.wall);
                return;
            }
        }
        let i = self.free_voice(usize::MAX, 0, true);
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let v = &mut self.voices[i];
        v.active = true;
        v.kind = Kind::Click;
        v.t = 0.0;
        v.dur = 0.03;
        v.release = 0.01;
        v.pitch = if accent { 96.0 } else { 89.0 };
        v.vel = if accent { 0.5 } else { 0.35 };
        v.gain_l = 0.7;
        v.gain_r = 0.7;
        v.phase = 0.0;
        v.env = 1.0;
        v.ev = None;
    }

    fn start_voice(&mut self, e: &NoteEvent) {
        let gain = self.gains.get(e.track).copied().unwrap_or(1.0);
        if gain <= 0.0 {
            return;
        }
        if self.use_sampler {
            if let Some(sm) = &mut self.sampler {
                sm.note_on(e, gain, self.wall, self.speed);
                return;
            }
        }
        let drum = e.sound == Sound::Drums;
        let i = self.free_voice(e.track, e.string, drum);
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let seed = self.seed | 1;
        let sr = self.sr;
        let v = &mut self.voices[i];
        v.active = true;
        v.sound = e.sound;
        v.track = e.track;
        v.string = e.string;
        v.t = 0.0;
        v.dur = e.dur as f32;
        v.pitch = e.pitch;
        v.vel = e.velocity * gain * (0.35 + 0.65 * e.volume);
        let pan = e.pan.clamp(-1.0, 1.0);
        v.gain_l = ((1.0 - pan) * 0.5).sqrt();
        v.gain_r = ((1.0 + pan) * 0.5).sqrt();
        v.noise = seed;
        v.env = 1.0;
        v.ev = Some(e.clone());
        v.lp = 0.0;
        v.hp = 0.0;
        v.bp1 = 0.0;
        v.bp2 = 0.0;
        v.phase = 0.0;
        v.phase2 = 0.0;
        v.drive = 0.0;
        match e.sound {
            Sound::Drums => {
                v.kind = Kind::Drum;
                v.drum = e.pitch as u8;
                v.release = 0.0;
                v.dur = 3.0;
            }
            Sound::Organ | Sound::Strings | Sound::Brass | Sound::Lead => {
                v.kind = Kind::Osc;
                v.release = if e.sound == Sound::Strings { 0.25 } else { 0.08 };
            }
            _ => {
                v.kind = Kind::Pluck;
                let f = midi_hz(e.pitch);
                // sustain time grows for low notes
                let t60: f32 = match e.sound {
                    Sound::Bass => 3.0,
                    Sound::Piano => 3.5,
                    Sound::Nylon => 2.6,
                    Sound::Overdrive => 4.0,
                    _ => 3.0,
                } * (220.0 / f).powf(0.35).clamp(0.4, 2.0);
                let t60 = if e.palm_mute { 0.18 } else if e.dead { 0.04 } else { t60 };
                v.loss = 10f32.powf(-3.0 / (t60 * f));
                v.lp_coef = match e.sound {
                    Sound::Bass => 0.35,
                    Sound::Nylon => 0.45,
                    Sound::Steel => 0.75,
                    Sound::Piano => 0.6,
                    _ => 0.65,
                };
                if e.palm_mute {
                    v.lp_coef *= 0.6;
                }
                if e.hammer {
                    v.lp_coef *= 0.8;
                }
                v.drive = if e.sound == Sound::Overdrive { 6.0 } else { 0.0 };
                v.release = if e.sound == Sound::Bass { 0.06 } else { 0.08 };
                // excite the string with filtered noise
                let len = ((sr / f) as usize + 2).min(BUF - 2);
                let bright = (0.25 + 0.6 * e.velocity).min(0.95) * v.lp_coef.max(0.4);
                let mut s = 0.0f32;
                for k in 0..BUF {
                    v.buf[k] = 0.0;
                    let _ = k;
                }
                for k in 0..len {
                    let n = v.rnd();
                    s += (n - s) * bright;
                    v.buf[k] = s;
                }
                if e.dead {
                    for k in 0..len {
                        v.buf[k] *= 0.6;
                    }
                }
                v.w = len;
            }
        }
    }

    fn render(&mut self, data: &mut [f32], channels: usize) {
        self.commands();
        let frames = data.len() / channels.max(1);
        let dt = 1.0 / self.sr as f64;
        for f in 0..frames {
            if self.playing {
                if self.count_in_clicks > 0 {
                    self.count_in_left -= dt * self.speed;
                    if self.count_in_left <= 0.0 {
                        let accent = self.count_in_clicks == 4;
                        self.click(accent);
                        self.count_in_clicks -= 1;
                        self.count_in_left += self.count_in_beat;
                    }
                } else if self.count_in_left > 0.0 {
                    self.count_in_left -= dt * self.speed;
                } else {
                    while self.next < self.events.len() && self.events[self.next].time <= self.pos {
                        let e = self.events.clone();
                        self.start_voice(&e[self.next]);
                        self.next += 1;
                    }
                    while self.next_click < self.clicks.len() && self.clicks[self.next_click].0 <= self.pos {
                        let accent = self.clicks[self.next_click].1;
                        if self.metronome {
                            self.click(accent);
                        }
                        self.next_click += 1;
                    }
                    self.pos += dt * self.speed;
                    if let Some((a, b)) = self.looping {
                        if self.pos >= b {
                            self.release_all();
                            self.seek(a);
                        }
                    }
                    if self.pos >= self.total + 0.5 {
                        self.playing = false;
                        self.playing_flag.store(false, Ordering::Relaxed);
                    }
                }
            }
            self.wall += dt;
            let (mut l, mut r) = (0.0f32, 0.0f32);
            if let Some(sm) = &mut self.sampler {
                let (sl, sr) = sm.next_frame(self.wall, self.speed);
                l += sl;
                r += sr;
            }
            for v in self.voices.iter_mut() {
                if !v.active {
                    continue;
                }
                let s = voice_sample(v, self.sr, self.speed as f32);
                l += s * v.gain_l;
                r += s * v.gain_r;
            }
            // light room reverb
            let input = (l + r) * 0.5 * 0.12;
            let mut rev = 0.0;
            for c in self.combs.iter_mut() {
                rev += c.tick(input);
            }
            for a in self.allpasses.iter_mut() {
                rev = a.tick(rev);
            }
            l = soft(l * self.master * 0.5 + rev * self.master + 0.0);
            r = soft(r * self.master * 0.5 + rev * self.master + 0.0);
            let base = f * channels;
            for c in 0..channels {
                data[base + c] = if c % 2 == 0 { l } else { r };
            }
        }
        if self.playing {
            self.position.store(self.pos.to_bits(), Ordering::Relaxed);
        }
    }
}

fn soft(x: f32) -> f32 {
    if x.abs() < 0.6 {
        x
    } else {
        x.signum() * (0.6 + (1.0 - 0.6) * ((x.abs() - 0.6) / 0.4).tanh())
    }
}

fn voice_sample(v: &mut Voice, sr: f32, speed: f32) -> f32 {
    let dt = speed / sr;
    let wall = 1.0 / sr;
    v.t += dt;
    // release envelope after the note's written end
    let mut env = 1.0;
    if v.t > v.dur {
        let rt = v.release.max(0.005);
        env = 1.0 - ((v.t - v.dur) / speed.max(0.1) * speed / rt).min(1.0);
        if env <= 0.0 {
            v.active = false;
            return 0.0;
        }
    }
    let _ = wall;
    match v.kind {
        Kind::Click => {
            v.phase += midi_hz(v.pitch) / sr;
            let e = (-v.t * 120.0).exp();
            if v.t > 0.05 {
                v.active = false;
            }
            (v.phase * std::f32::consts::TAU).sin() * e * v.vel
        }
        Kind::Drum => drum_sample(v, sr),
        Kind::Pluck => {
            let mut pitch = v.pitch;
            if let Some(e) = &v.ev {
                let frac = (v.t / v.dur.max(0.01)).min(1.0);
                if let Some(b) = &e.bend {
                    pitch += b.semitones_at(frac);
                }
                if e.slide_to != 0.0 && frac > 0.6 {
                    pitch += e.slide_to * ((frac - 0.6) / 0.4);
                }
                if e.vibrato {
                    pitch += 0.25 * (v.t * 5.5 * std::f32::consts::TAU).sin();
                }
            }
            let delay = (sr / midi_hz(pitch) - 0.5).clamp(2.0, (BUF - 3) as f32);
            let rd = v.w as f32 - delay;
            let rd = if rd < 0.0 { rd + BUF as f32 } else { rd };
            let i0 = rd as usize % BUF;
            let fr = rd - rd.floor();
            let i1 = (i0 + 1) % BUF;
            let im = (i0 + BUF - 1) % BUF;
            let a = v.buf[i0] * (1.0 - fr) + v.buf[i1] * fr;
            let b = v.buf[im] * (1.0 - fr) + v.buf[i0] * fr;
            // two-tap average with brightness control
            let y = (a * (0.5 + 0.5 * v.lp_coef) + b * (0.5 - 0.5 * v.lp_coef)) * v.loss;
            v.buf[v.w] = y;
            v.w = (v.w + 1) % BUF;
            v.env = env;
            // body resonance / tone
            v.lp += (y - v.lp) * 0.6;
            let mut out = v.lp * v.vel * 0.9;
            if v.drive > 0.0 {
                out = (out * v.drive).tanh() * 0.35;
            }
            if v.t > 0.5 && v.lp.abs() < 1e-5 && v.buf[(v.w + BUF / 2) % BUF].abs() < 1e-5 {
                v.active = false;
            }
            out * env
        }
        Kind::Osc => {
            let mut pitch = v.pitch;
            if let Some(e) = &v.ev {
                let frac = (v.t / v.dur.max(0.01)).min(1.0);
                if let Some(b) = &e.bend {
                    pitch += b.semitones_at(frac);
                }
                if e.vibrato || v.t > 0.4 {
                    pitch += 0.12 * (v.t * 5.0 * std::f32::consts::TAU).sin();
                }
            }
            let f = midi_hz(pitch);
            v.phase = (v.phase + f / sr).fract();
            v.phase2 = (v.phase2 + f * 1.004 / sr).fract();
            let (raw, attack, cutoff) = match v.sound {
                Sound::Organ => {
                    let p = v.phase * std::f32::consts::TAU;
                    (p.sin() * 0.6 + (2.0 * p).sin() * 0.3 + (4.0 * p).sin() * 0.15, 0.01, 0.9)
                }
                Sound::Strings => ((v.phase * 2.0 - 1.0) * 0.5 + (v.phase2 * 2.0 - 1.0) * 0.5, 0.12, 0.25),
                Sound::Brass => (v.phase * 2.0 - 1.0, 0.04, 0.3),
                _ => (if v.phase < 0.5 { 0.6 } else { -0.6 }, 0.01, 0.35),
            };
            v.lp += (raw - v.lp) * cutoff;
            let a = (v.t / attack).min(1.0);
            v.lp * a * env * v.vel * 0.35
        }
    }
}

fn drum_sample(v: &mut Voice, sr: f32) -> f32 {
    let t = v.t;
    let n = v.rnd();
    let tau = std::f32::consts::TAU;
    let out = match v.drum {
        35 | 36 => {
            let f = 50.0 + 110.0 * (-t * 30.0).exp();
            v.phase += f / sr;
            if t > 0.5 {
                v.active = false;
            }
            (v.phase * tau).sin() * (-t * 9.0).exp() * 1.2 + n * 0.1 * (-t * 200.0).exp()
        }
        38 | 40 => {
            v.phase += 185.0 / sr;
            v.hp += (n - v.hp) * 0.5;
            if t > 0.35 {
                v.active = false;
            }
            (v.phase * tau).sin() * (-t * 25.0).exp() * 0.5 + (n - v.hp * 0.5) * (-t * 14.0).exp() * 0.6
        }
        37 => {
            if t > 0.06 {
                v.active = false;
            }
            v.phase += 900.0 / sr;
            ((v.phase * tau).sin() * 0.5 + n * 0.4) * (-t * 70.0).exp()
        }
        39 => {
            if t > 0.3 {
                v.active = false;
            }
            let burst = if t < 0.03 { ((t * 100.0) as i32 % 1 == 0) as i32 as f32 } else { 1.0 };
            n * burst * (-t * 18.0).exp() * 0.7
        }
        42 | 44 | 46 => {
            let decay = if v.drum == 46 { 5.0 } else { 40.0 };
            v.hp += (n - v.hp) * 0.25;
            if t > 1.0 {
                v.active = false;
            }
            (n - v.hp) * (-t * decay).exp() * 0.55
        }
        49 | 52 | 55 | 57 => {
            v.hp += (n - v.hp) * 0.3;
            if t > 2.5 {
                v.active = false;
            }
            (n - v.hp) * (-t * 2.2).exp() * 0.45
        }
        51 | 53 | 59 => {
            v.hp += (n - v.hp) * 0.3;
            v.phase += 3200.0 / sr;
            v.phase2 += 4700.0 / sr;
            if t > 2.0 {
                v.active = false;
            }
            ((n - v.hp) * 0.25 + ((v.phase * tau).sin() + (v.phase2 * tau).sin()) * 0.08) * (-t * 3.0).exp()
        }
        41 | 43 | 45 | 47 | 48 | 50 => {
            let base = 70.0 + (v.drum as f32 - 41.0) * 14.0;
            let f = base * (1.0 + 0.5 * (-t * 20.0).exp());
            v.phase += f / sr;
            if t > 0.7 {
                v.active = false;
            }
            (v.phase * tau).sin() * (-t * 7.0).exp() * 0.9 + n * 0.05 * (-t * 60.0).exp()
        }
        _ => {
            v.bp1 += (n - v.bp1) * 0.4;
            if t > 0.25 {
                v.active = false;
            }
            v.bp1 * (-t * 25.0).exp() * 0.5
        }
    };
    out * v.vel
}


// ----------------------------------------------------------------------------
// Sampled instruments: a General MIDI sound bank of recorded instruments.

const SOUND_BANK: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/soundbank/GeneralUser-GS.sf2"));
const CHUNK: usize = 64;
const BEND_RANGE: f32 = 12.0;

struct Active {
    id: u64,
    ch: i32,
    key: i32,
    track: usize,
    string: u8,
    start: f64,
    end: f64,
    len: f64,
    bend: Option<Arc<crate::gp::Bend>>,
    vibrato: bool,
    slide_to: f32,
}

pub struct Sampler {
    synth: rustysynth::Synthesizer,
    l: Vec<f32>,
    r: Vec<f32>,
    idx: usize,
    program: [i32; 16],
    volume: [i32; 16],
    pan: [i32; 16],
    expr: [i32; 16],
    bend: [i32; 16],
    owner: [u64; 16],
    active: Vec<Active>,
    next_id: u64,
}

fn channel_for(track: usize, drums: bool) -> i32 {
    if drums {
        return 9;
    }
    let c = (track % 15) as i32;
    if c >= 9 {
        c + 1
    } else {
        c
    }
}

impl Sampler {
    pub fn load(sr: f32) -> Option<Sampler> {
        let mut cur = std::io::Cursor::new(SOUND_BANK);
        let sf = Arc::new(rustysynth::SoundFont::new(&mut cur).ok()?);
        let mut settings = rustysynth::SynthesizerSettings::new(sr as i32);
        settings.block_size = CHUNK;
        settings.maximum_polyphony = 128;
        settings.enable_reverb_and_chorus = true;
        let mut synth = rustysynth::Synthesizer::new(&sf, &settings).ok()?;
        synth.set_master_volume(0.9);
        for ch in 0..16 {
            // pitch bend range of 12 semitones, for string bends and slides
            synth.process_midi_message(ch, 0xB0, 101, 0);
            synth.process_midi_message(ch, 0xB0, 100, 0);
            synth.process_midi_message(ch, 0xB0, 6, BEND_RANGE as i32);
            synth.process_midi_message(ch, 0xB0, 38, 0);
            synth.process_midi_message(ch, 0xB0, 101, 127);
            synth.process_midi_message(ch, 0xB0, 100, 127);
            synth.process_midi_message(ch, 0xB0, 91, 48);
            synth.process_midi_message(ch, 0xB0, 93, 8);
        }
        Some(Sampler {
            synth,
            l: vec![0.0; CHUNK],
            r: vec![0.0; CHUNK],
            idx: CHUNK,
            program: [-1; 16],
            volume: [-1; 16],
            pan: [-1; 16],
            expr: [-1; 16],
            bend: [8192; 16],
            owner: [0; 16],
            active: Vec::with_capacity(256),
            next_id: 1,
        })
    }

    fn cc(&mut self, ch: i32, cc: i32, v: i32) {
        self.synth.process_midi_message(ch, 0xB0, cc, v);
    }

    fn set_bend(&mut self, ch: i32, semis: f32) {
        let v = (8192.0 + semis / BEND_RANGE * 8192.0).round().clamp(0.0, 16383.0) as i32;
        if self.bend[ch as usize] != v {
            self.bend[ch as usize] = v;
            self.synth.process_midi_message(ch, 0xE0, v & 0x7F, v >> 7);
        }
    }

    pub fn all_off(&mut self) {
        self.synth.note_off_all(false);
        self.active.clear();
        for ch in 0..16 {
            self.set_bend(ch, 0.0);
            self.owner[ch as usize] = 0;
        }
    }

    pub fn click(&mut self, accent: bool, wall: f64) {
        let key = if accent { 76 } else { 77 };
        self.synth.note_on(9, key, if accent { 110 } else { 85 });
        self.active.push(Active {
            id: 0,
            ch: 9,
            key,
            track: usize::MAX,
            string: 0,
            start: wall,
            end: wall + 0.08,
            len: 0.08,
            bend: None,
            vibrato: false,
            slide_to: 0.0,
        });
    }

    pub fn note_on(&mut self, e: &NoteEvent, gain: f32, wall: f64, speed: f64) {
        let drums = e.sound == Sound::Drums;
        let ch = channel_for(e.track, drums);
        let c = ch as usize;
        let key = e.pitch.round() as i32;
        if !(0..128).contains(&key) {
            return;
        }
        if !drums {
            let guitar = (24..=31).contains(&e.program);
            let prog = if e.harmonic && guitar { 31 } else { e.program.clamp(0, 127) };
            if self.program[c] != prog {
                self.program[c] = prog;
                self.synth.process_midi_message(ch, 0xC0, prog, 0);
            }
            // a string sounds one note at a time
            if let Some(i) = self.active.iter().position(|a| a.track == e.track && a.string == e.string) {
                let a = self.active.swap_remove(i);
                self.synth.note_off(a.ch, a.key);
            }
        }
        let vol = (e.volume * 127.0).round().clamp(0.0, 127.0) as i32;
        if self.volume[c] != vol {
            self.volume[c] = vol;
            self.cc(ch, 7, vol);
        }
        let pan = (64.0 + e.pan * 63.0).round().clamp(0.0, 127.0) as i32;
        if self.pan[c] != pan {
            self.pan[c] = pan;
            self.cc(ch, 10, pan);
        }
        let expr = (gain * 110.0).round().clamp(0.0, 127.0) as i32;
        if self.expr[c] != expr {
            self.expr[c] = expr;
            self.cc(ch, 11, expr);
        }
        let mut vel = e.velocity;
        let mut dur = e.dur;
        if e.palm_mute {
            vel *= 0.85;
            dur = dur.min(0.16);
        }
        if e.dead {
            vel *= 0.6;
            dur = dur.min(0.05);
        }
        let shaped = e.bend.is_some() || e.vibrato || e.slide_to != 0.0;
        let id = self.next_id;
        self.next_id += 1;
        if !drums {
            if shaped {
                self.owner[c] = id;
                let start = e.bend.as_ref().map(|b| b.semitones_at(0.0)).unwrap_or(0.0);
                self.set_bend(ch, start);
            } else {
                let owner_alive = self.active.iter().any(|a| a.id == self.owner[c] && (a.start - wall).abs() < 0.03);
                if !owner_alive {
                    self.owner[c] = 0;
                    self.set_bend(ch, 0.0);
                }
            }
        }
        let v = (vel * 127.0).round().clamp(1.0, 127.0) as i32;
        self.synth.note_on(ch, key, v);
        let len = (dur / speed.max(0.05)).max(0.02);
        self.active.push(Active {
            id,
            ch,
            key,
            track: e.track,
            string: e.string,
            start: wall,
            end: wall + len,
            len,
            bend: e.bend.clone(),
            vibrato: e.vibrato,
            slide_to: e.slide_to,
        });
    }

    /// Note-offs and pitch movement, once per small block.
    fn update(&mut self, wall: f64) {
        let mut i = 0;
        while i < self.active.len() {
            if self.active[i].end <= wall {
                let a = self.active.swap_remove(i);
                self.synth.note_off(a.ch, a.key);
            } else {
                i += 1;
            }
        }
        for c in 0..16usize {
            let id = self.owner[c];
            if id == 0 {
                continue;
            }
            let Some(a) = self.active.iter().find(|a| a.id == id) else { continue };
            let t = (wall - a.start) as f32;
            let frac = (t / a.len as f32).clamp(0.0, 1.0);
            let mut semis = 0.0;
            if let Some(b) = &a.bend {
                semis += b.semitones_at(frac);
            }
            if a.slide_to != 0.0 && frac > 0.6 {
                semis += a.slide_to * ((frac - 0.6) / 0.4);
            }
            if a.vibrato && t > 0.08 {
                semis += 0.22 * (t * 5.5 * std::f32::consts::TAU).sin();
            }
            self.set_bend(c as i32, semis);
        }
    }

    fn next_frame(&mut self, wall: f64, _speed: f64) -> (f32, f32) {
        if self.idx >= CHUNK {
            self.update(wall);
            let (l, r) = (&mut self.l, &mut self.r);
            self.synth.render(l, r);
            self.idx = 0;
        }
        let out = (self.l[self.idx], self.r[self.idx]);
        self.idx += 1;
        // the main mix halves its input; make up for it here
        (out.0 * 1.7, out.1 * 1.7)
    }
}
