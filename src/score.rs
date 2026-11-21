//! Lays out and draws a song as standard notation and tablature.

use crate::gp::{Beat, Harmonic, NoteKind, Song, Track, QUARTER};
use eframe::egui::{self, epaint::CubicBezierShape, Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, Vec2};

#[derive(Clone, PartialEq)]
pub struct ViewOpts {
    pub zoom: f32,
    pub notation: bool,
    pub tab: bool,
    pub tracks: Vec<usize>,
    pub width: f32,
}

#[derive(Clone, Debug)]
pub struct MLayout {
    pub header: usize,
    pub x: f32,
    pub w: f32,
    pub prefix: f32,
    pub clef: bool,
    pub key: bool,
    pub time: bool,
    pub ticks: Vec<(u32, f32)>,
}

impl MLayout {
    pub fn tick_x(&self, tick: u32) -> f32 {
        if self.ticks.is_empty() {
            return self.x + self.prefix + 8.0;
        }
        match self.ticks.binary_search_by_key(&tick, |t| t.0) {
            Ok(i) => self.x + self.ticks[i].1,
            Err(i) => {
                if i == 0 {
                    self.x + self.ticks[0].1
                } else if i >= self.ticks.len() {
                    self.x + self.ticks[self.ticks.len() - 1].1
                } else {
                    let (t0, x0) = self.ticks[i - 1];
                    let (t1, x1) = self.ticks[i];
                    self.x + x0 + (x1 - x0) * (tick - t0) as f32 / (t1 - t0).max(1) as f32
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Block {
    pub track: usize,
    pub top: f32,
    pub lane_y: f32,
    pub notation: Option<f32>,
    pub tab_fx: f32,
    pub tab: Option<f32>,
    pub rhythm: Option<f32>,
    pub bottom: f32,
}

#[derive(Clone, Debug)]
pub struct System {
    pub y: f32,
    pub h: f32,
    pub measures: Vec<MLayout>,
    pub blocks: Vec<Block>,
}

#[derive(Clone)]
pub struct Layout {
    pub opts: ViewOpts,
    pub header_h: f32,
    pub systems: Vec<System>,
    pub height: f32,
    pub left: f32,
    pub right: f32,
}

static DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Switch the score between a white page and a dark page.
pub fn set_dark(dark: bool) {
    DARK.store(dark, std::sync::atomic::Ordering::Relaxed);
}
pub fn is_dark() -> bool {
    dark()
}
fn dark() -> bool {
    DARK.load(std::sync::atomic::Ordering::Relaxed)
}
pub fn ink() -> Color32 {
    if dark() { Color32::from_rgb(228, 228, 222) } else { Color32::from_rgb(20, 20, 24) }
}
pub fn paper() -> Color32 {
    if dark() { Color32::from_rgb(32, 33, 37) } else { Color32::from_rgb(255, 255, 253) }
}
fn grey() -> Color32 {
    if dark() { Color32::from_rgb(150, 150, 160) } else { Color32::from_rgb(120, 120, 128) }
}
fn line_col() -> Color32 {
    if dark() { Color32::from_rgb(125, 125, 135) } else { Color32::from_rgb(110, 110, 118) }
}
fn accent() -> Color32 {
    if dark() { Color32::from_rgb(240, 110, 95) } else { Color32::from_rgb(190, 40, 40) }
}
fn text_blue() -> Color32 {
    if dark() { Color32::from_rgb(130, 175, 245) } else { Color32::from_rgb(40, 70, 150) }
}

fn spacing(gap: u32, z: f32) -> f32 {
    let g = (gap.max(30)) as f32 / 60.0;
    (10.0 + 7.5 * g.log2()).max(16.0) * z
}

fn show_tab(track: &Track, opts: &ViewOpts) -> bool {
    !track.percussion && (opts.tab || !opts.notation)
}
fn show_notation(track: &Track, opts: &ViewOpts) -> bool {
    opts.notation || track.percussion
}

pub fn layout(song: &Song, opts: &ViewOpts) -> Layout {
    let z = opts.zoom;
    let left = 30.0 * z;
    let right = opts.width - 30.0 * z;
    let avail = (right - left).max(200.0);
    let mut header_h = 40.0 * z;
    if !song.title.is_empty() {
        header_h += 34.0 * z;
    }
    if !song.subtitle.is_empty() {
        header_h += 20.0 * z;
    }
    if !song.artist.is_empty() || !song.album.is_empty() {
        header_h += 20.0 * z;
    }
    header_h += 44.0 * z;

    // natural widths
    let mut items: Vec<MLayout> = Vec::new();
    let mut prev_key = i8::MIN;
    for (hi, h) in song.headers.iter().enumerate() {
        let mut ticks: Vec<u32> = Vec::new();
        for &ti in &opts.tracks {
            if let Some(m) = song.tracks.get(ti).and_then(|t| t.measures.get(hi)) {
                for b in &m.beats {
                    ticks.push(b.start);
                }
            }
        }
        ticks.sort_unstable();
        ticks.dedup();
        let key_changed = h.key != prev_key;
        prev_key = h.key;
        let mut ml = MLayout {
            header: hi,
            x: 0.0,
            w: 0.0,
            prefix: 0.0,
            clef: false,
            key: key_changed && h.key != 0,
            time: h.time_changed,
            ticks: Vec::new(),
        };
        ml.prefix = prefix_width(&ml, h.key, z);
        let mut x = ml.prefix + 10.0 * z;
        if h.repeat_open {
            x += 6.0 * z;
        }
        for (i, &t) in ticks.iter().enumerate() {
            ml.ticks.push((t, x));
            let next = ticks.get(i + 1).copied().unwrap_or(h.end().max(t + 1));
            // the longest beat starting here decides how much room it needs
            let mut gap = next.saturating_sub(t);
            for &ti in &opts.tracks {
                if let Some(m) = song.tracks.get(ti).and_then(|tr| tr.measures.get(hi)) {
                    if let Some(b) = m.beats.iter().find(|b| b.start == t) {
                        gap = gap.min(b.duration.ticks()).max(30);
                        let extra = beat_extra(b, z);
                        x += extra;
                    }
                }
            }
            x += spacing(gap, z);
        }
        if ticks.is_empty() {
            x += 50.0 * z;
        }
        ml.w = x + 4.0 * z;
        items.push(ml);
    }

    // break into systems
    let mut lines: Vec<Vec<MLayout>> = Vec::new();
    let mut cur: Vec<MLayout> = Vec::new();
    let mut cur_w = 0.0;
    for mut ml in items {
        let first = cur.is_empty();
        let key = song.headers[ml.header].key;
        if first {
            ml.clef = true;
            ml.key = key != 0;
            refit_prefix(&mut ml, key, z);
        }
        if !first && cur_w + ml.w > avail {
            lines.push(std::mem::take(&mut cur));
            ml.clef = true;
            ml.key = key != 0;
            refit_prefix(&mut ml, key, z);
            cur_w = 0.0;
        }
        cur_w += ml.w;
        cur.push(ml);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }

    let mut systems = Vec::new();
    let mut y = header_h;
    let nlines = lines.len();
    for (li, mut ms) in lines.into_iter().enumerate() {
        let total: f32 = ms.iter().map(|m| m.w).sum();
        let stretch = if li + 1 < nlines || total > avail * 0.75 { avail / total.max(1.0) } else { 1.0 };
        let mut x = left;
        for m in &mut ms {
            let body = m.w - m.prefix;
            for t in &mut m.ticks {
                t.1 = m.prefix + (t.1 - m.prefix) * stretch;
            }
            m.w = m.prefix + body * stretch;
            m.x = x;
            x += m.w;
        }
        // vertical blocks
        let mut blocks = Vec::new();
        let mut by = y + 26.0 * z;
        for &ti in &opts.tracks {
            let Some(track) = song.tracks.get(ti) else { continue };
            let top = by;
            let lane_y = by;
            by += 14.0 * z;
            let ls = 8.0 * z;
            let notation = if show_notation(track, opts) {
                by += 4.0 * ls;
                let n = by;
                by += 4.0 * ls + 4.5 * ls;
                Some(n)
            } else {
                None
            };
            let (tab_fx, tab, rhythm) = if show_tab(track, opts) {
                let fx = by;
                by += 24.0 * z;
                let t = by;
                by += (track.strings.len().max(1) - 1) as f32 * 13.0 * z;
                let rhythm = if notation.is_none() {
                    let r = by + 6.0 * z;
                    by += 30.0 * z;
                    Some(r)
                } else {
                    by += 8.0 * z;
                    None
                };
                (fx, Some(t), rhythm)
            } else {
                (by, None, None)
            };
            blocks.push(Block { track: ti, top, lane_y, notation, tab_fx, tab, rhythm, bottom: by });
            by += 14.0 * z;
        }
        let h = by - y + 10.0 * z;
        systems.push(System { y, h, measures: ms, blocks });
        y += h;
    }
    Layout { opts: opts.clone(), header_h, systems, height: y + 60.0 * z, left, right }
}

fn beat_extra(b: &Beat, z: f32) -> f32 {
    let mut e = 0.0;
    if b.notes.iter().any(|n| n.effects.grace.is_some()) {
        e += 10.0 * z;
    }
    if b.notes.iter().any(|n| n.fret >= 10) {
        e += 3.0 * z;
    }
    if b.duration.dotted {
        e += 3.0 * z;
    }
    e
}

fn prefix_width(ml: &MLayout, key: i8, z: f32) -> f32 {
    let mut p = 0.0;
    if ml.clef {
        p += 30.0 * z;
    }
    if ml.key {
        p += key.unsigned_abs() as f32 * 7.0 * z + 4.0 * z;
    }
    if ml.time {
        p += 20.0 * z;
    }
    p
}

fn refit_prefix(ml: &mut MLayout, key: i8, z: f32) {
    let newp = prefix_width(ml, key, z);
    let delta = newp - ml.prefix;
    for t in &mut ml.ticks {
        t.1 += delta;
    }
    ml.w += delta;
    ml.prefix = newp;
}

// ----------------------------------------------------------------------------
// Pitch spelling

const SHARP_ORDER: [usize; 7] = [3, 0, 4, 1, 5, 2, 6]; // F C G D A E B (as steps C=0)
const FLAT_ORDER: [usize; 7] = [6, 2, 5, 1, 4, 0, 3]; // B E A D G C F

fn key_alter(key: i8, step: usize) -> i8 {
    if key > 0 {
        if SHARP_ORDER[..key as usize].contains(&step) {
            return 1;
        }
    } else if key < 0 && FLAT_ORDER[..(-key) as usize].contains(&step) {
        return -1;
    }
    0
}

/// (diatonic index, alteration)
fn spell(midi: i32, key: i8) -> (i32, i8) {
    let pc = midi.rem_euclid(12);
    let oct = midi.div_euclid(12) - 1;
    let (step, alt) = if key >= 0 {
        [(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (3, 0), (3, 1), (4, 0), (4, 1), (5, 0), (5, 1), (6, 0)][pc as usize]
    } else {
        [(0, 0), (1, -1), (1, 0), (2, -1), (2, 0), (3, 0), (4, -1), (4, 0), (5, -1), (5, 0), (6, -1), (6, 0)][pc as usize]
    };
    // E# / B# / Cb / Fb in extreme keys
    let (step, alt, oct) = if key >= 6 && pc == 5 {
        (2, 1, oct)
    } else if key >= 7 && pc == 0 {
        (6, 1, oct - 1)
    } else if key <= -6 && pc == 11 {
        (0, -1, oct + 1)
    } else if key <= -7 && pc == 4 {
        (3, -1, oct)
    } else {
        (step, alt, oct)
    };
    (oct * 7 + step as i32, alt as i8)
}

fn drum_position(key: i32) -> (i32, bool) {
    // (diatonic index on a treble staff, cross head)
    match key {
        35 | 36 => (31, false),
        38 | 40 => (35, false),
        37 => (35, true),
        39 => (35, true),
        41 | 43 => (32, false),
        45 => (33, false),
        47 => (34, false),
        48 | 50 => (36, false),
        42 | 44 => (39, true),
        46 => (39, true),
        49 | 57 | 52 | 55 => (40, true),
        51 | 53 | 59 => (38, true),
        _ => (37, true),
    }
}

// ----------------------------------------------------------------------------
// Drawing helpers

fn serif(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("score".into()))
}
fn serif_bold(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("score-bold".into()))
}

fn text(p: &egui::Painter, pos: Pos2, anchor: Align2, s: &str, size: f32, color: Color32) -> Rect {
    p.text(pos, anchor, s, serif(size), color)
}
fn bold(p: &egui::Painter, pos: Pos2, anchor: Align2, s: &str, size: f32, color: Color32) -> Rect {
    p.text(pos, anchor, s, serif_bold(size), color)
}
/// Tab numbers and small playing marks stay in a clean sans-serif.
fn sans(p: &egui::Painter, pos: Pos2, anchor: Align2, s: &str, size: f32, color: Color32) -> Rect {
    p.text(pos, anchor, s, FontId::proportional(size), color)
}

fn note_head(p: &egui::Painter, c: Pos2, ls: f32, hollow: bool, color: Color32) {
    let rx = ls * 0.62;
    let ry = ls * 0.44;
    let ang: f32 = -0.35;
    let (sa, ca) = ang.sin_cos();
    let pts: Vec<Pos2> = (0..20)
        .map(|i| {
            let t = i as f32 / 20.0 * std::f32::consts::TAU;
            let (x, y) = (rx * t.cos(), ry * t.sin());
            Pos2::new(c.x + x * ca - y * sa, c.y + x * sa + y * ca)
        })
        .collect();
    if hollow {
        p.add(Shape::closed_line(pts, Stroke::new(ls * 0.16, color)));
    } else {
        p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
    }
}

fn cross_head(p: &egui::Painter, c: Pos2, ls: f32, color: Color32) {
    let r = ls * 0.4;
    let s = Stroke::new(ls * 0.15, color);
    p.line_segment([c + Vec2::new(-r, -r), c + Vec2::new(r, r)], s);
    p.line_segment([c + Vec2::new(-r, r), c + Vec2::new(r, -r)], s);
}

fn sharp(p: &egui::Painter, c: Pos2, ls: f32, color: Color32) {
    let thin = Stroke::new(ls * 0.1, color);
    let thick = Stroke::new(ls * 0.25, color);
    let dx = ls * 0.22;
    p.line_segment([c + Vec2::new(-dx, -ls * 1.1), c + Vec2::new(-dx, ls * 1.2)], thin);
    p.line_segment([c + Vec2::new(dx, -ls * 1.2), c + Vec2::new(dx, ls * 1.1)], thin);
    for oy in [-0.35f32, 0.35] {
        p.line_segment(
            [c + Vec2::new(-ls * 0.45, ls * (oy + 0.12)), c + Vec2::new(ls * 0.45, ls * (oy - 0.12))],
            thick,
        );
    }
}

fn flat(p: &egui::Painter, c: Pos2, ls: f32, color: Color32) {
    let s = Stroke::new(ls * 0.12, color);
    let x = c.x - ls * 0.25;
    p.line_segment([Pos2::new(x, c.y - ls * 1.6), Pos2::new(x, c.y + ls * 0.5)], s);
    p.add(CubicBezierShape::from_points_stroke(
        [
            Pos2::new(x, c.y - ls * 0.05),
            Pos2::new(x + ls * 0.6, c.y - ls * 0.6),
            Pos2::new(x + ls * 0.9, c.y + ls * 0.05),
            Pos2::new(x, c.y + ls * 0.5),
        ],
        false,
        Color32::TRANSPARENT,
        Stroke::new(ls * 0.2, color),
    ));
}

fn natural(p: &egui::Painter, c: Pos2, ls: f32, color: Color32) {
    let thin = Stroke::new(ls * 0.1, color);
    let thick = Stroke::new(ls * 0.22, color);
    let dx = ls * 0.22;
    p.line_segment([c + Vec2::new(-dx, -ls * 1.2), c + Vec2::new(-dx, ls * 0.45)], thin);
    p.line_segment([c + Vec2::new(dx, -ls * 0.45), c + Vec2::new(dx, ls * 1.2)], thin);
    for oy in [-0.3f32, 0.3] {
        p.line_segment([c + Vec2::new(-dx, ls * (oy + 0.1)), c + Vec2::new(dx, ls * (oy - 0.1))], thick);
    }
}

fn smooth(points: &[(f32, f32)], origin: Pos2, unit: f32) -> Vec<Pos2> {
    let pts: Vec<Pos2> = points.iter().map(|&(x, y)| origin + Vec2::new(x * unit, y * unit)).collect();
    let mut out = Vec::new();
    for i in 0..pts.len().saturating_sub(1) {
        let p0 = if i == 0 { pts[0] } else { pts[i - 1] };
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = if i + 2 < pts.len() { pts[i + 2] } else { pts[i + 1] };
        for k in 0..8 {
            let t = k as f32 / 8.0;
            let t2 = t * t;
            let t3 = t2 * t;
            let f = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * ((2.0 * b) + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            out.push(Pos2::new(f(p0.x, p1.x, p2.x, p3.x), f(p0.y, p1.y, p2.y, p3.y)));
        }
    }
    if let Some(l) = pts.last() {
        out.push(*l);
    }
    out
}

fn treble_clef(p: &egui::Painter, g_line: Pos2, ls: f32, color: Color32) {
    let path = [
        (0.15, 0.05),
        (0.65, -0.35),
        (0.75, 0.45),
        (0.05, 0.95),
        (-0.8, 0.55),
        (-0.9, -0.55),
        (-0.25, -1.55),
        (0.45, -2.45),
        (0.65, -3.5),
        (0.35, -4.35),
        (-0.05, -3.95),
        (-0.2, -2.9),
        (0.0, -1.0),
        (0.2, 1.0),
        (0.33, 2.55),
        (0.05, 3.1),
        (-0.45, 2.95),
    ];
    let pts = smooth(&path, g_line, ls);
    p.add(Shape::line(pts, Stroke::new(ls * 0.2, color)));
    p.circle_filled(g_line + Vec2::new(-0.3 * ls, 2.65 * ls), ls * 0.3, color);
}

fn bass_clef(p: &egui::Painter, f_line: Pos2, ls: f32, color: Color32) {
    let path = [(-0.55, 0.0), (-0.4, -0.55), (0.25, -0.65), (0.75, -0.1), (0.5, 1.0), (-0.6, 2.0)];
    let pts = smooth(&path, f_line, ls);
    p.add(Shape::line(pts, Stroke::new(ls * 0.25, color)));
    p.circle_filled(f_line + Vec2::new(-0.5 * ls, 0.0), ls * 0.3, color);
    p.circle_filled(f_line + Vec2::new(1.15 * ls, -0.5 * ls), ls * 0.13, color);
    p.circle_filled(f_line + Vec2::new(1.15 * ls, 0.5 * ls), ls * 0.13, color);
}

fn rest(p: &egui::Painter, x: f32, mid: f32, ls: f32, value: u8, color: Color32) {
    match value {
        1 => {
            p.rect_filled(Rect::from_min_size(Pos2::new(x - ls * 0.6, mid - ls), Vec2::new(ls * 1.2, ls * 0.5)), 0.0, color);
        }
        2 => {
            p.rect_filled(Rect::from_min_size(Pos2::new(x - ls * 0.6, mid - ls * 0.5), Vec2::new(ls * 1.2, ls * 0.5)), 0.0, color);
        }
        4 => {
            let pts = [
                (-0.2, -1.5),
                (0.35, -0.8),
                (-0.25, -0.2),
                (0.35, 0.5),
                (-0.2, 0.35),
                (0.1, 1.1),
            ];
            let v: Vec<Pos2> = pts.iter().map(|&(a, b)| Pos2::new(x + a * ls, mid + b * ls)).collect();
            p.add(Shape::line(v, Stroke::new(ls * 0.25, color)));
        }
        _ => {
            let flags = match value {
                8 => 1,
                16 => 2,
                32 => 3,
                _ => 4,
            };
            let top = mid - ls * 0.8;
            p.line_segment(
                [Pos2::new(x + ls * 0.45, top), Pos2::new(x - ls * 0.1, top + ls * (1.4 + flags as f32 * 0.6))],
                Stroke::new(ls * 0.13, color),
            );
            for k in 0..flags {
                let fy = top + k as f32 * ls * 0.7;
                let fx = x + ls * 0.45 - k as f32 * ls * 0.25;
                p.circle_filled(Pos2::new(fx - ls * 0.55, fy + ls * 0.15), ls * 0.2, color);
                p.line_segment([Pos2::new(fx - ls * 0.55, fy + ls * 0.3), Pos2::new(fx, fy)], Stroke::new(ls * 0.1, color));
            }
        }
    }
}

fn beams_of(value: u8) -> usize {
    match value {
        8 => 1,
        16 => 2,
        32 => 3,
        64 => 4,
        _ => 0,
    }
}

/// Groups of beat indices that share beams.
fn beam_groups(beats: &[Beat], bar_start: u32, num: u8, den: u8) -> Vec<Vec<usize>> {
    let window = if den == 8 && num % 3 == 0 { QUARTER * 3 / 2 } else if den >= 8 { QUARTER / 2 * 2 } else { QUARTER };
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_win = u32::MAX;
    for (i, b) in beats.iter().enumerate() {
        let w = (b.start.saturating_sub(bar_start)) / window;
        let beamable = b.duration.value >= 8 && !b.is_rest();
        if !beamable || w != cur_win {
            if !cur.is_empty() {
                groups.push(std::mem::take(&mut cur));
            }
        }
        if beamable {
            if cur.is_empty() {
                cur_win = w;
            }
            cur.push(i);
        } else {
            groups.push(vec![i]);
            cur_win = u32::MAX;
        }
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    groups
}

fn bend_label(v: i32) -> String {
    let q = (v.abs() as f32 / 25.0).round() as i32; // quarter tones
    match q {
        0 => String::new(),
        1 => "¼".into(),
        2 => "½".into(),
        3 => "¾".into(),
        4 => "full".into(),
        _ => {
            let full = q / 4;
            let rest = q % 4;
            let frac = ["", "¼", "½", "¾"][rest as usize];
            format!("{full}{frac}")
        }
    }
}

pub struct PaintCtx<'a> {
    pub song: &'a Song,
    pub origin: Pos2,
    pub clip: Rect,
    /// (header, tick start, tick end) of the beat under the play cursor
    pub cursor: Option<(usize, u32, u32)>,
    pub selection: Option<(usize, usize)>,
    pub playing_bar: Option<usize>,
    /// Draw the title block (off for printed pages after the first).
    pub header: bool,
}

pub fn paint(p: &egui::Painter, l: &Layout, c: &PaintCtx) {
    let z = l.opts.zoom;
    let o = c.origin;
    let song = c.song;
    let ls = 8.0 * z;
    let ts = 13.0 * z;

    if c.header {
        // header
        let cx = o.x + (l.left + l.right) / 2.0;
        let mut hy = o.y + 30.0 * z;
        if !song.title.is_empty() {
            bold(p, Pos2::new(cx, hy), Align2::CENTER_TOP, &song.title, 28.0 * z, ink());
            hy += 34.0 * z;
        }
        if !song.subtitle.is_empty() {
            text(p, Pos2::new(cx, hy), Align2::CENTER_TOP, &song.subtitle, 15.0 * z, ink());
            hy += 20.0 * z;
        }
        let mut by_line = Vec::new();
        if !song.artist.is_empty() {
            by_line.push(song.artist.clone());
        }
        if !song.album.is_empty() {
            by_line.push(song.album.clone());
        }
        if !by_line.is_empty() {
            text(p, Pos2::new(cx, hy), Align2::CENTER_TOP, &by_line.join("  —  "), 13.0 * z, grey());
        }
        let info_y = o.y + l.header_h - 40.0 * z;
        let mut credits = Vec::new();
        if !song.words.is_empty() {
            credits.push(format!("Words & music: {}", song.words));
        }
        if !song.tab_author.is_empty() {
            credits.push(format!("Tab: {}", song.tab_author));
        }
        if !credits.is_empty() {
            text(p, Pos2::new(o.x + l.right, info_y - 14.0 * z), Align2::RIGHT_TOP, &credits.join("   "), 11.0 * z, grey());
        }
        // tempo mark
        let tx = o.x + l.left;
        note_head(p, Pos2::new(tx + 4.0 * z, info_y + 8.0 * z), ls * 0.85, false, ink());
        p.line_segment(
            [Pos2::new(tx + 8.0 * z, info_y + 7.0 * z), Pos2::new(tx + 8.0 * z, info_y - 9.0 * z)],
            Stroke::new(1.2 * z, ink()),
        );
        text(p, Pos2::new(tx + 13.0 * z, info_y), Align2::LEFT_TOP, &format!("= {}", song.tempo), 13.0 * z, ink());
        if let Some(&ti) = l.opts.tracks.first() {
            if let Some(t) = song.tracks.get(ti) {
                if !t.percussion {
                    let names: Vec<String> = t.strings.iter().rev().map(|&s| crate::gp::pitch_name(s)).collect();
                    let mut s = format!("Tuning: {}", names.join(" "));
                    if t.capo > 0 {
                        s += &format!("    Capo fret {}", t.capo);
                    }
                    text(p, Pos2::new(tx + 80.0 * z, info_y), Align2::LEFT_TOP, &s, 11.0 * z, grey());
                }
            }
        }
    }

    for sys in &l.systems {
        let sy0 = o.y + sys.y;
        if sy0 > c.clip.max.y || sy0 + sys.h < c.clip.min.y {
            continue;
        }
        paint_system(p, l, c, sys, z, ls, ts);
    }
}

fn paint_system(p: &egui::Painter, l: &Layout, c: &PaintCtx, sys: &System, z: f32, ls: f32, ts: f32) {
    let o = c.origin;
    let song = c.song;
    let x0 = o.x + sys.measures.first().map(|m| m.x).unwrap_or(l.left);
    let x1 = o.x + sys.measures.last().map(|m| m.x + m.w).unwrap_or(l.right);
    let line_stroke = Stroke::new(0.8 * z.max(0.8), line_col());
    let bar_stroke = Stroke::new(1.0 * z.max(0.8), ink());

    let first_block = sys.blocks.first().map(|b| o.y + b.top).unwrap_or(o.y + sys.y);
    let last_block_bottom = |b: &Block| -> f32 {
        if let Some(t) = b.tab {
            o.y + t + (song.tracks[b.track].strings.len().max(1) - 1) as f32 * ts
        } else if let Some(n) = b.notation {
            o.y + n + 4.0 * ls
        } else {
            o.y + b.bottom
        }
    };
    let staff_top = |b: &Block| -> f32 {
        if let Some(n) = b.notation {
            o.y + n
        } else {
            o.y + b.tab.unwrap_or(b.top)
        }
    };

    // selection and cursor highlights
    for m in &sys.measures {
        let mx = o.x + m.x;
        if let Some((a, b)) = c.selection {
            if m.header >= a && m.header <= b {
                for blk in &sys.blocks {
                    let r = Rect::from_min_max(
                        Pos2::new(mx, staff_top(blk) - 4.0 * z),
                        Pos2::new(mx + m.w, last_block_bottom(blk) + 4.0 * z),
                    );
                    p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(60, 120, 230, 40));
                }
            }
        }
        if c.playing_bar == Some(m.header) {
            for blk in &sys.blocks {
                let r = Rect::from_min_max(
                    Pos2::new(mx, staff_top(blk) - 4.0 * z),
                    Pos2::new(mx + m.w, last_block_bottom(blk) + 4.0 * z),
                );
                p.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(255, 200, 60, 28));
            }
        }
        if let Some((hi, t0, t1)) = c.cursor {
            if hi == m.header {
                let bx0 = o.x + m.tick_x(t0) - 7.0 * z;
                let bx1 = (o.x + m.tick_x(t1) - 7.0 * z).max(bx0 + 14.0 * z).min(mx + m.w);
                for blk in &sys.blocks {
                    let r = Rect::from_min_max(
                        Pos2::new(bx0, staff_top(blk) - 6.0 * z),
                        Pos2::new(bx1, last_block_bottom(blk) + 6.0 * z),
                    );
                    p.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(230, 60, 40, 45));
                    p.rect_stroke(r, 2.0, Stroke::new(1.0, Color32::from_rgba_unmultiplied(230, 60, 40, 150)), egui::StrokeKind::Inside);
                }
            }
        }
    }

    // bar numbers, markers, endings, chord names (top lane of the first track)
    for m in &sys.measures {
        let h = &song.headers[m.header];
        let mx = o.x + m.x;
        let lane = first_block;
        sans(p, Pos2::new(mx + 2.0 * z, lane - 2.0 * z), Align2::LEFT_BOTTOM, &(m.header + 1).to_string(), 9.0 * z, accent());
                if let Some((name, col)) = &h.marker {
            let mut color = Color32::from_rgb(col[0], col[1], col[2]);
            let sum = col[0] as u32 + col[1] as u32 + col[2] as u32;
            if (!dark() && sum > 600) || (dark() && sum < 200) {
                color = accent();
            } else if dark() {
                // lift dark marker colours so they read on the dark page
                color = Color32::from_rgb(col[0].saturating_add(70), col[1].saturating_add(70), col[2].saturating_add(70));
            }
            p.text(
                Pos2::new(mx + 18.0 * z, lane - 4.0 * z),
                Align2::LEFT_BOTTOM,
                name,
                serif_bold(14.0 * z),
                color,
            );
        }
        if h.alternative != 0 {
            let nums: Vec<String> = (0..8).filter(|b| h.alternative & (1 << b) != 0).map(|b| format!("{}", b + 1)).collect();
            let ey = lane + 4.0 * z;
            let s = Stroke::new(1.0 * z, ink());
            p.line_segment([Pos2::new(mx + 2.0, ey + 10.0 * z), Pos2::new(mx + 2.0, ey)], s);
            p.line_segment([Pos2::new(mx + 2.0, ey), Pos2::new(mx + m.w - 4.0 * z, ey)], s);
            text(p, Pos2::new(mx + 5.0 * z, ey + 1.0 * z), Align2::LEFT_TOP, &format!("{}.", nums.join(", ")), 10.0 * z, ink());
        }
    }

    for blk in &sys.blocks {
        let track = &song.tracks[blk.track];
        // staff lines
        if let Some(n) = blk.notation {
            for i in 0..5 {
                let y = o.y + n + i as f32 * ls;
                p.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], line_stroke);
            }
        }
        if let Some(t) = blk.tab {
            for i in 0..track.strings.len() {
                let y = o.y + t + i as f32 * ts;
                p.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], line_stroke);
            }
        }
        // track name on multi-track view
        if l.opts.tracks.len() > 1 {
            let y = staff_top(blk);
            text(p, Pos2::new(x0 + 40.0 * z, o.y + blk.lane_y + 2.0 * z), Align2::LEFT_TOP, &short_name(&track.name), 9.0 * z, grey());
            let _ = y;
        }
        // system bracket
        let top = staff_top(blk);
        let bottom = last_block_bottom(blk);
        p.line_segment([Pos2::new(x0, top), Pos2::new(x0, bottom)], bar_stroke);

        let mut prev_heads: [Option<(Pos2, i32)>; 8] = [None; 8];
        let mut prev_tab: [Option<(Pos2, i32, bool, bool)>; 8] = [None; 8];
        let mut ring_open = false;
        let mut pm_open = false;
        for m in &sys.measures {
            let h = &song.headers[m.header];
            let mx = o.x + m.x;
            let mut px = mx + 4.0 * z;
            // clef / key / time
            if let Some(n) = blk.notation {
                let ny = o.y + n;
                if m.clef {
                    if track.percussion {
                        for dx in [0.0, 4.0] {
                            p.rect_filled(
                                Rect::from_min_size(Pos2::new(px + 8.0 * z + dx * z, ny + ls), Vec2::new(2.0 * z, 2.0 * ls)),
                                0.0,
                                ink(),
                            );
                        }
                    } else if track.is_bass() {
                        bass_clef(p, Pos2::new(px + 12.0 * z, ny + ls), ls, ink());
                    } else {
                        treble_clef(p, Pos2::new(px + 12.0 * z, ny + 3.0 * ls), ls, ink());
                        text(p, Pos2::new(px + 12.0 * z, ny + 4.0 * ls + 2.2 * ls), Align2::CENTER_TOP, "8", 8.0 * z, ink());
                    }
                }
                let mut kx = px + if m.clef { 30.0 * z } else { 0.0 };
                if m.key && !track.percussion {
                    let bass_shift = if track.is_bass() { 14 } else { 0 };
                    let sharps = [38, 35, 39, 36, 33, 37, 34];
                    let flats = [34, 37, 33, 36, 32, 35, 31];
                    for i in 0..h.key.unsigned_abs() as usize {
                        let d = if h.key > 0 { sharps[i] } else { flats[i] } - bass_shift;
                        let base = if track.is_bass() { 18 } else { 30 };
                        let y = ny + 4.0 * ls - (d - base) as f32 * ls / 2.0;
                        if h.key > 0 {
                            sharp(p, Pos2::new(kx + 4.0 * z, y), ls, ink());
                        } else {
                            flat(p, Pos2::new(kx + 4.0 * z, y), ls, ink());
                        }
                        kx += 7.0 * z;
                    }
                    kx += 4.0 * z;
                }
                if m.time {
                    let fs = 2.6 * ls;
                    bold(p, Pos2::new(kx + 9.0 * z, ny + ls), Align2::CENTER_CENTER, &h.numerator.to_string(), fs, ink());
                    bold(p, Pos2::new(kx + 9.0 * z, ny + 3.0 * ls), Align2::CENTER_CENTER, &h.denominator.to_string(), fs, ink());
                }
            }
            if let Some(t) = blk.tab {
                let ty = o.y + t;
                let span = (track.strings.len().max(1) - 1) as f32 * ts;
                if m.clef {
                    let letters = ["T", "A", "B"];
                    for (i, s) in letters.iter().enumerate() {
                        let y = ty + span / 2.0 + (i as f32 - 1.0) * span.min(3.6 * ts) / 3.0;
                        bold(p, Pos2::new(px + 12.0 * z, y), Align2::CENTER_CENTER, s, (span / 4.2).clamp(8.0, 15.0 * z), ink());
                    }
                }
                if m.time {
                    let kx = mx + m.prefix - 12.0 * z;
                    let fs = (span / 2.2).clamp(10.0, 26.0 * z);
                    bold(p, Pos2::new(kx, ty + span * 0.28), Align2::CENTER_CENTER, &h.numerator.to_string(), fs, ink());
                    bold(p, Pos2::new(kx, ty + span * 0.75), Align2::CENTER_CENTER, &h.denominator.to_string(), fs, ink());
                }
            }
            px = mx + m.prefix;

            // bar lines and repeats
            let bx_end = mx + m.w;
            let draw_bar = |x: f32, thick: bool| {
                let w = if thick { 3.0 * z } else { 1.0 * z.max(0.8) };
                p.line_segment([Pos2::new(x, top), Pos2::new(x, bottom)], Stroke::new(w, ink()));
            };
            let dots = |x: f32| {
                if let Some(n) = blk.notation {
                    let ny = o.y + n;
                    p.circle_filled(Pos2::new(x, ny + 1.5 * ls), 1.6 * z, ink());
                    p.circle_filled(Pos2::new(x, ny + 2.5 * ls), 1.6 * z, ink());
                }
                if let Some(t) = blk.tab {
                    let ty = o.y + t;
                    let span = (track.strings.len().max(1) - 1) as f32 * ts;
                    p.circle_filled(Pos2::new(x, ty + span / 2.0 - ts * 0.5), 1.6 * z, ink());
                    p.circle_filled(Pos2::new(x, ty + span / 2.0 + ts * 0.5), 1.6 * z, ink());
                }
            };
            if h.repeat_open {
                let x = px - 2.0 * z;
                draw_bar(x, true);
                draw_bar(x + 4.0 * z, false);
                dots(x + 8.0 * z);
            }
            if h.repeat_close > 0 {
                draw_bar(bx_end - 2.0 * z, true);
                draw_bar(bx_end - 6.0 * z, false);
                dots(bx_end - 10.0 * z);
                if h.repeat_close > 1 {
                    text(p, Pos2::new(bx_end - 4.0 * z, top - 3.0 * z), Align2::RIGHT_BOTTOM, &format!("x{}", h.repeat_close + 1), 10.0 * z, ink());
                }
            } else if h.double_bar {
                draw_bar(bx_end - 3.0 * z, false);
                draw_bar(bx_end, false);
            } else if m.header + 1 == song.headers.len() {
                draw_bar(bx_end - 4.0 * z, false);
                draw_bar(bx_end - 1.0 * z, true);
            } else {
                draw_bar(bx_end, false);
            }

            let Some(measure) = track.measures.get(m.header) else { continue };
            let beats = &measure.beats;
            if beats.is_empty() {
                continue;
            }

            // accidentals in effect within this bar
            let mut altered: std::collections::HashMap<i32, i8> = std::collections::HashMap::new();
            // positions of note heads per beat, for stems
            struct BeatGeo {
                x: f32,
                ys: Vec<f32>,
                value: u8,
            }
            let mut geo: Vec<BeatGeo> = Vec::new();

            for (bi, beat) in beats.iter().enumerate() {
                let bx = o.x + m.tick_x(beat.start);
                let mut g = BeatGeo { x: bx, ys: Vec::new(), value: beat.duration.value };

                // ---- top lane: chord names and text
                let lane_top = o.y + blk.lane_y;
                if let Some(ch) = &beat.chord {
                    if !ch.name.is_empty() {
                        p.text(Pos2::new(bx, lane_top + 12.0 * z), Align2::CENTER_BOTTOM, &ch.name, serif_bold(13.0 * z), ink());
                    }
                }
                if let Some(tx) = &beat.text {
                    let ty = if beat.chord.is_some() { lane_top } else { lane_top + 12.0 * z };
                    p.text(Pos2::new(bx - 4.0 * z, ty), Align2::LEFT_BOTTOM, tx, FontId::new(12.0 * z, egui::FontFamily::Name("score".into())), text_blue());
                }
                if let Some(mix) = &beat.mix {
                    if let Some(tp) = mix.tempo {
                        if !(m.header == 0 && bi == 0 && tp == song.tempo) {
                            let tx = if beat.start == h.start { (bx).max(mx + 18.0 * z + h.marker.as_ref().map(|m| m.0.chars().count() as f32 * 7.5 * z + 12.0 * z).unwrap_or(0.0)) } else { bx };
                            text(p, Pos2::new(tx, lane_top - 3.0 * z), Align2::LEFT_BOTTOM, &format!("tempo {tp}"), 9.0 * z, grey());
                        }
                    }
                }

                // ---- notation
                if let Some(n) = blk.notation {
                    let ny = o.y + n;
                    let bottom_line = ny + 4.0 * ls;
                    let mid = ny + 2.0 * ls;
                    if beat.is_rest() {
                        rest(p, bx, mid, ls, beat.duration.value, ink());
                        if beat.duration.dotted {
                            p.circle_filled(Pos2::new(bx + ls, mid - ls * 0.5), 1.4 * z, ink());
                        }
                    } else {
                        let hollow = beat.duration.value <= 2;
                        let base = if track.is_bass() { 18 } else { 30 };
                        let mut positions: Vec<(i32, i8, bool, &crate::gp::Note)> = Vec::new();
                        for note in &beat.notes {
                            if track.percussion {
                                let (d, x) = drum_position(note.fret);
                                positions.push((d, 0, x, note));
                            } else {
                                let pitch = track.pitch_of(note) + 12;
                                let (d, a) = spell(pitch, h.key);
                                positions.push((d, a, note.kind == NoteKind::Dead, note));
                            }
                        }
                        positions.sort_by_key(|q| q.0);
                        let mut last_d = i32::MIN;
                        let mut shift_next = false;
                        for (d, alt, cross, note) in &positions {
                            let y = bottom_line - (d - base) as f32 * ls / 2.0;
                            // seconds next to each other go side by side
                            let offset = if *d == last_d + 1 && !shift_next {
                                shift_next = true;
                                ls * 1.15
                            } else {
                                shift_next = false;
                                0.0
                            };
                            last_d = *d;
                            let hx = bx + offset;
                            // ledger lines
                            let steps_from_bottom = d - base;
                            if steps_from_bottom <= -2 {
                                let mut s = -2;
                                while s >= steps_from_bottom {
                                    let ly = bottom_line - s as f32 * ls / 2.0;
                                    p.line_segment([Pos2::new(hx - ls, ly), Pos2::new(hx + ls, ly)], Stroke::new(1.0 * z, ink()));
                                    s -= 2;
                                }
                            }
                            if steps_from_bottom >= 10 {
                                let mut s = 10;
                                while s <= steps_from_bottom {
                                    let ly = bottom_line - s as f32 * ls / 2.0;
                                    p.line_segment([Pos2::new(hx - ls, ly), Pos2::new(hx + ls, ly)], Stroke::new(1.0 * z, ink()));
                                    s += 2;
                                }
                            }
                            let color = if note.ghost { grey() } else { ink() };
                            if *cross {
                                cross_head(p, Pos2::new(hx, y), ls, color);
                            } else {
                                note_head(p, Pos2::new(hx, y), ls, hollow, color);
                            }
                            if note.effects.harmonic.is_some() {
                                p.circle_stroke(Pos2::new(hx, y - ls * 1.2), ls * 0.25, Stroke::new(1.0 * z, ink()));
                            }
                            // accidentals
                            if !track.percussion {
                                let current = *altered.get(d).unwrap_or(&key_alter(h.key, d.rem_euclid(7) as usize));
                                let tie = note.kind == NoteKind::Tie;
                                if *alt != current && !tie {
                                    let ax = bx - ls * 1.5;
                                    match alt {
                                        1 => sharp(p, Pos2::new(ax, y), ls * 0.85, ink()),
                                        -1 => flat(p, Pos2::new(ax, y), ls * 0.85, ink()),
                                        _ => natural(p, Pos2::new(ax, y), ls * 0.85, ink()),
                                    }
                                }
                                altered.insert(*d, *alt);
                            }
                            if beat.duration.dotted {
                                let dy = if steps_from_bottom % 2 == 0 { y - ls * 0.5 } else { y };
                                p.circle_filled(Pos2::new(hx + ls * 1.1, dy), 1.4 * z, ink());
                            }
                            // tie from previous beat
                            let s = (note.string as usize).min(7);
                            if note.kind == NoteKind::Tie {
                                if let Some((pp, _)) = prev_heads[s] {
                                    let a = Pos2::new(pp.x + ls * 0.6, pp.y + ls * 0.5);
                                    let b = Pos2::new(hx - ls * 0.6, y + ls * 0.5);
                                    tie_arc(p, a, b, ls, true);
                                } else {
                                    tie_arc(p, Pos2::new(hx - 14.0 * z, y + ls * 0.5), Pos2::new(hx - ls * 0.6, y + ls * 0.5), ls, true);
                                }
                            }
                            prev_heads[s] = Some((Pos2::new(hx, y), *d));
                            g.ys.push(y);
                        }
                        if beat.notes.iter().any(|n| n.accent) {
                            let ay = g.ys.iter().cloned().fold(f32::MAX, f32::min).min(ny) - ls * 1.2;
                            p.line_segment([Pos2::new(bx - ls * 0.6, ay - ls * 0.3), Pos2::new(bx + ls * 0.6, ay)], Stroke::new(1.0 * z, ink()));
                            p.line_segment([Pos2::new(bx + ls * 0.6, ay), Pos2::new(bx - ls * 0.6, ay + ls * 0.3)], Stroke::new(1.0 * z, ink()));
                        }
                        if beat.notes.iter().any(|n| n.effects.staccato) {
                            let sy = g.ys.iter().cloned().fold(f32::MIN, f32::max) + ls * 1.0;
                            p.circle_filled(Pos2::new(bx, sy), 1.4 * z, ink());
                        }
                    }
                }

                // ---- tablature
                if let Some(t) = blk.tab {
                    let ty = o.y + t;
                    let fx_y = o.y + blk.tab_fx;
                    let fs = 11.0 * z;
                    for note in &beat.notes {
                        let si = note.string as usize - 1;
                        let y = ty + si as f32 * ts;
                        let mut label = match note.kind {
                            NoteKind::Dead => "x".to_string(),
                            _ => note.fret.to_string(),
                        };
                        if let Some(hm) = note.effects.harmonic {
                            label = match hm {
                                Harmonic::Natural => format!("<{label}>"),
                                _ => format!("<{label}>"),
                            };
                        }
                        if note.ghost || note.kind == NoteKind::Tie {
                            label = format!("({label})");
                        }
                        let color = if note.kind == NoteKind::Tie { grey() } else { ink() };
                        let galley = p.layout_no_wrap(label, FontId::proportional(fs * 1.05), color);
                        let size = galley.size();
                        let r = Rect::from_center_size(Pos2::new(bx, y), size + Vec2::new(2.0 * z, -2.0 * z));
                        p.rect_filled(r, 0.0, paper());
                        p.galley(Pos2::new(bx - size.x / 2.0, y - size.y / 2.0), galley, color);
                        // grace note
                        if let Some(gr) = &note.effects.grace {
                            let gx = bx - 11.0 * z;
                            let gl = p.layout_no_wrap(gr.fret.to_string(), FontId::proportional(fs * 0.7), ink());
                            let gs = gl.size();
                            let gr_r = Rect::from_center_size(Pos2::new(gx - 2.0 * z, y), gs);
                            p.rect_filled(gr_r, 0.0, paper());
                            p.galley(gr_r.min, gl, ink());
                        }
                        let s = (note.string as usize).min(7);
                        // hammer-on / pull-off and slides from the previous note on this string
                        if let Some((pp, pf, hammer, slide)) = prev_tab[s] {
                            if note.kind != NoteKind::Tie {
                                if hammer && pp.y == y {
                                    let a = Pos2::new(pp.x + 2.0 * z, y - ts * 0.45);
                                    let b = Pos2::new(bx - 2.0 * z, y - ts * 0.45);
                                    tie_arc(p, a, b, ts * 0.6, false);
                                    let lbl = if note.fret >= pf { "H" } else { "P" };
                                    sans(p, Pos2::new((a.x + b.x) / 2.0, y - ts * 0.8), Align2::CENTER_BOTTOM, lbl, 8.0 * z, ink());
                                }
                                if slide && pp.y == y {
                                    let up = note.fret > pf;
                                    let (ya, yb) = if up { (y + 3.0 * z, y - 3.0 * z) } else { (y - 3.0 * z, y + 3.0 * z) };
                                    p.line_segment(
                                        [Pos2::new(pp.x + 7.0 * z, ya), Pos2::new(bx - 7.0 * z, yb)],
                                        Stroke::new(1.0 * z, ink()),
                                    );
                                }
                            }
                        }
                        prev_tab[s] = Some((Pos2::new(bx, y), note.fret, note.effects.hammer, note.effects.slide.is_some()));
                        // bends
                        if let Some(bend) = &note.effects.bend {
                            let v = bend.max_value();
                            let peak = fx_y + 6.0 * z;
                            let start = Pos2::new(bx + 6.0 * z, y - 2.0 * z);
                            let top = Pos2::new(bx + 14.0 * z, peak);
                            p.add(CubicBezierShape::from_points_stroke(
                                [start, Pos2::new(top.x, start.y), Pos2::new(top.x, start.y), top],
                                false,
                                Color32::TRANSPARENT,
                                Stroke::new(1.0 * z, ink()),
                            ));
                            p.add(Shape::convex_polygon(
                                vec![top, top + Vec2::new(-3.0 * z, 5.0 * z), top + Vec2::new(3.0 * z, 5.0 * z)],
                                ink(),
                                Stroke::NONE,
                            ));
                            sans(p, top - Vec2::new(0.0, 1.0 * z), Align2::CENTER_BOTTOM, &bend_label(v), 8.5 * z, ink());
                            if let Some(last) = bend.points.last() {
                                if last.1.abs() < v.abs() / 2 {
                                    let end = Pos2::new(top.x + 10.0 * z, y - 6.0 * z);
                                    p.line_segment([top, end], Stroke::new(1.0 * z, ink()));
                                }
                            }
                        }
                    }
                    // beat level marks in the effect lane
                    let ring = beat.notes.iter().any(|n| n.effects.let_ring);
                    let pm = beat.notes.iter().any(|n| n.effects.palm_mute);
                    if ring && !ring_open {
                        text(p, Pos2::new(bx - 4.0 * z, fx_y + 2.0 * z), Align2::LEFT_TOP, "let ring", 8.5 * z, ink());
                    }
                    if pm && !pm_open {
                        text(p, Pos2::new(bx - 4.0 * z, fx_y + 10.0 * z), Align2::LEFT_TOP, "P.M.", 8.5 * z, ink());
                    }
                    ring_open = ring;
                    pm_open = pm;
                    if beat.notes.iter().any(|n| n.effects.vibrato) || beat.effects.vibrato {
                        let w = 18.0 * z;
                        let pts: Vec<Pos2> = (0..=12)
                            .map(|k| {
                                let xx = bx + k as f32 / 12.0 * w;
                                Pos2::new(xx, fx_y + 4.0 * z + (k as f32 * 1.6).sin() * 2.0 * z)
                            })
                            .collect();
                        p.add(Shape::line(pts, Stroke::new(1.2 * z, ink())));
                    }
                    if beat.notes.iter().any(|n| n.effects.trill.is_some()) {
                        text(p, Pos2::new(bx, fx_y + 2.0 * z), Align2::LEFT_TOP, "tr", 9.0 * z, ink());
                    }
                    if beat.notes.iter().any(|n| matches!(n.effects.harmonic, Some(Harmonic::Artificial | Harmonic::Pinch | Harmonic::Tapped | Harmonic::Semi))) {
                        text(p, Pos2::new(bx, fx_y + 12.0 * z), Align2::LEFT_TOP, "A.H.", 8.0 * z, ink());
                    }
                    if beat.effects.slap > 0 {
                        let s = ["", "T", "S", "P"][beat.effects.slap.min(3) as usize];
                        text(p, Pos2::new(bx, fx_y + 2.0 * z), Align2::CENTER_TOP, s, 9.0 * z, ink());
                    }
                    if let Some(tb) = &beat.effects.tremolo_bar {
                        text(p, Pos2::new(bx, fx_y + 2.0 * z), Align2::LEFT_TOP, &format!("w/bar {}", bend_label(tb.max_value())), 8.0 * z, ink());
                    }
                    if beat.effects.stroke_down > 0 || beat.effects.stroke_up > 0 {
                        let ys: Vec<f32> = beat.notes.iter().map(|n| ty + (n.string as f32 - 1.0) * ts).collect();
                        let (a, b) = (ys.iter().cloned().fold(f32::MAX, f32::min), ys.iter().cloned().fold(f32::MIN, f32::max));
                        let sx = bx - 9.0 * z;
                        p.line_segment([Pos2::new(sx, a - 4.0), Pos2::new(sx, b + 4.0)], Stroke::new(1.0 * z, ink()));
                        // down strums sound low to high strings: the arrow points up on the tab
                        let (tip, dir) = if beat.effects.stroke_up > 0 { (b + 4.0, -1.0) } else { (a - 4.0, 1.0) };
                        p.add(Shape::convex_polygon(
                            vec![Pos2::new(sx, tip), Pos2::new(sx - 3.0 * z, tip - 5.0 * z * dir), Pos2::new(sx + 3.0 * z, tip - 5.0 * z * dir)],
                            ink(),
                            Stroke::NONE,
                        ));
                    }
                    if beat.is_rest() && blk.notation.is_none() {
                        let span = (track.strings.len().max(1) - 1) as f32 * ts;
                        rest(p, bx, ty + span / 2.0, ls, beat.duration.value, ink());
                    }
                    if let Some(tp) = beat.notes.iter().find_map(|n| n.effects.tremolo_picking) {
                        let base = ty - 8.0 * z;
                        for k in 0..beams_of(tp).max(1) {
                            let yy = base - k as f32 * 3.0 * z;
                            p.line_segment([Pos2::new(bx - 4.0 * z, yy + 2.0 * z), Pos2::new(bx + 4.0 * z, yy - 2.0 * z)], Stroke::new(1.4 * z, ink()));
                        }
                    }
                }
                geo.push(g);
            }

            // ---- stems and beams
            let groups = beam_groups(beats, h.start, h.numerator, h.denominator);
            // notation
            if let Some(n) = blk.notation {
                let ny = o.y + n;
                let mid = ny + 2.0 * ls;
                for grp in &groups {
                    let notes: Vec<usize> = grp.iter().copied().filter(|&i| !geo[i].ys.is_empty()).collect();
                    if notes.is_empty() {
                        continue;
                    }
                    let only = notes.len() == 1;
                    if geo[notes[0]].value <= 1 {
                        continue;
                    }
                    let avg: f32 = notes.iter().map(|&i| geo[i].ys.iter().sum::<f32>() / geo[i].ys.len() as f32).sum::<f32>() / notes.len() as f32;
                    let up = avg > mid;
                    let stem_len = 3.4 * ls;
                    let hw = ls * 0.58;
                    let sx = |i: usize| if up { geo[i].x + hw } else { geo[i].x - hw };
                    let tip = |i: usize| {
                        let g = &geo[i];
                        if up {
                            g.ys.iter().cloned().fold(f32::MAX, f32::min) - stem_len
                        } else {
                            g.ys.iter().cloned().fold(f32::MIN, f32::max) + stem_len
                        }
                    };
                    let root = |i: usize| {
                        let g = &geo[i];
                        if up {
                            g.ys.iter().cloned().fold(f32::MIN, f32::max)
                        } else {
                            g.ys.iter().cloned().fold(f32::MAX, f32::min)
                        }
                    };
                    let first = notes[0];
                    let last = *notes.last().unwrap();
                    let (xa, xb) = (sx(first), sx(last));
                    let mut ya = tip(first);
                    let mut yb = tip(last);
                    if !only {
                        let slope = ((yb - ya) / (xb - xa).max(1.0)).clamp(-0.2, 0.2);
                        yb = ya + slope * (xb - xa);
                        // keep every stem long enough
                        let mut shift: f32 = 0.0;
                        for &i in &notes {
                            let by = ya + slope * (sx(i) - xa);
                            let need = tip(i);
                            if up {
                                shift = shift.max(by - need);
                            } else {
                                shift = shift.min(by - need);
                            }
                        }
                        ya -= shift;
                        yb -= shift;
                        // pull extreme beams back toward the staff
                        let center = (ya + yb) / 2.0;
                        if up && center > mid {
                            let d = center - mid;
                            ya -= d;
                            yb -= d;
                        }
                        if !up && center < mid {
                            let d = mid - center;
                            ya += d;
                            yb += d;
                        }
                    }
                    let beam_y = |x: f32| if only { ya } else { ya + (yb - ya) * (x - xa) / (xb - xa).max(1.0) };
                    for &i in &notes {
                        let x = sx(i);
                        p.line_segment([Pos2::new(x, root(i)), Pos2::new(x, beam_y(x))], Stroke::new(1.0 * z, ink()));
                    }
                    draw_beams(p, &notes.iter().map(|&i| (sx(i), geo[i].value)).collect::<Vec<_>>(), &beam_y, if up { 1.0 } else { -1.0 }, ls, z);
                    let b0 = &beats[notes[0]];
                    if b0.duration.enters > 1 && notes.len() > 1 {
                        let cxp = (xa + xb) / 2.0;
                        let cy = beam_y(cxp) + if up { -ls * 0.8 } else { ls * 0.8 };
                        text(p, Pos2::new(cxp, cy), Align2::CENTER_CENTER, &b0.duration.enters.to_string(), 9.0 * z, ink());
                    }
                }
            }
            // rhythm under the tab
            if let Some(r) = blk.rhythm {
                let ry = o.y + r;
                let rb = ry + 20.0 * z;
                for grp in &groups {
                    let notes: Vec<usize> = grp.iter().copied().filter(|&i| !beats[i].is_rest()).collect();
                    if notes.is_empty() {
                        continue;
                    }
                    for &i in &notes {
                        let v = geo[i].value;
                        if v == 1 {
                            continue;
                        }
                        let x = geo[i].x;
                        let top_y = if v == 2 { ry + 8.0 * z } else { ry };
                        p.line_segment([Pos2::new(x, top_y), Pos2::new(x, rb)], Stroke::new(1.0 * z, ink()));
                        if beats[i].duration.dotted {
                            p.circle_filled(Pos2::new(x + 4.0 * z, rb - 2.0 * z), 1.4 * z, ink());
                        }
                    }
                    let pts: Vec<(f32, u8)> = notes.iter().map(|&i| (geo[i].x, geo[i].value)).collect();
                    draw_beams(p, &pts, &|_x| rb, -1.0, ls, z);
                    let b0 = &beats[notes[0]];
                    if b0.duration.enters > 1 {
                        let cxp = (geo[notes[0]].x + geo[*notes.last().unwrap()].x) / 2.0;
                        text(p, Pos2::new(cxp, rb + 2.0 * z), Align2::CENTER_TOP, &b0.duration.enters.to_string(), 9.0 * z, ink());
                    }
                }
            }
        }
    }
    // connect tracks with a line at the start
    if let (Some(a), Some(b)) = (sys.blocks.first(), sys.blocks.last()) {
        if sys.blocks.len() > 1 {
            p.line_segment([Pos2::new(x0, staff_top(a)), Pos2::new(x0, last_block_bottom(b))], Stroke::new(2.0 * z, ink()));
        }
    }
}

fn short_name(s: &str) -> String {
    let s: String = s.chars().take(30).collect();
    s
}

fn tie_arc(p: &egui::Painter, a: Pos2, b: Pos2, ls: f32, below: bool) {
    let d = if below { 1.0 } else { -1.0 };
    let h = (ls * 0.6).min((b.x - a.x).abs() * 0.3 + 2.0) * d;
    p.add(CubicBezierShape::from_points_stroke(
        [a, Pos2::new(a.x + (b.x - a.x) * 0.25, a.y + h), Pos2::new(a.x + (b.x - a.x) * 0.75, b.y + h), b],
        false,
        Color32::TRANSPARENT,
        Stroke::new(1.0, ink()),
    ));
}

/// `dir` = 1 when beams hang below stem tips toward the notes (stems up), -1 otherwise.
fn draw_beams(p: &egui::Painter, pts: &[(f32, u8)], beam_y: &dyn Fn(f32) -> f32, dir: f32, ls: f32, z: f32) {
    let thick = ls * 0.45;
    let gap = ls * 0.75;
    if pts.len() == 1 {
        let (x, v) = pts[0];
        let n = beams_of(v);
        for k in 0..n {
            let y = beam_y(x) + dir * k as f32 * gap;
            let pts = vec![
                Pos2::new(x, y),
                Pos2::new(x + ls * 0.5, y + dir * ls * 0.9),
                Pos2::new(x + ls * 0.9, y + dir * ls * 2.0),
            ];
            p.add(Shape::line(pts, Stroke::new(1.4 * z, ink())));
        }
        return;
    }
    let max_beams = pts.iter().map(|q| beams_of(q.1)).max().unwrap_or(0);
    for level in 0..max_beams {
        let off = dir * level as f32 * gap;
        let mut i = 0;
        while i < pts.len() {
            if beams_of(pts[i].1) <= level {
                i += 1;
                continue;
            }
            let mut j = i;
            while j + 1 < pts.len() && beams_of(pts[j + 1].1) > level {
                j += 1;
            }
            let (xa, xb) = if i == j {
                // stub toward the neighbour
                let x = pts[i].0;
                if i + 1 < pts.len() {
                    (x, x + ls * 1.0)
                } else {
                    (x - ls * 1.0, x)
                }
            } else {
                (pts[i].0, pts[j].0)
            };
            let ya = beam_y(xa) + off;
            let yb = beam_y(xb) + off;
            let quad = vec![
                Pos2::new(xa, ya),
                Pos2::new(xb, yb),
                Pos2::new(xb, yb + dir * thick),
                Pos2::new(xa, ya + dir * thick),
            ];
            p.add(Shape::convex_polygon(quad, ink(), Stroke::NONE));
            i = j + 1;
        }
    }
}

/// Which bar and beat lies under a point (relative to the score's origin).
pub fn hit(l: &Layout, song: &Song, pos: Pos2) -> Option<(usize, u32, usize)> {
    for sys in &l.systems {
        if pos.y < sys.y || pos.y > sys.y + sys.h {
            continue;
        }
        // which track block
        let blk = sys
            .blocks
            .iter()
            .min_by(|a, b| {
                let da = (pos.y - (a.top + a.bottom) / 2.0).abs();
                let db = (pos.y - (b.top + b.bottom) / 2.0).abs();
                da.partial_cmp(&db).unwrap()
            })
            .map(|b| b.track)
            .unwrap_or(0);
        for m in &sys.measures {
            if pos.x >= m.x && pos.x < m.x + m.w {
                let mut best = song.headers[m.header].start;
                let mut bd = f32::MAX;
                for &(t, x) in &m.ticks {
                    let d = (m.x + x - pos.x).abs();
                    if d < bd {
                        bd = d;
                        best = t;
                    }
                }
                return Some((m.header, best, blk));
            }
        }
    }
    None
}

/// Vertical position of a bar (top of its system), for following playback.
pub fn bar_y(l: &Layout, header: usize) -> Option<(f32, f32)> {
    l.systems
        .iter()
        .find(|s| s.measures.iter().any(|m| m.header == header))
        .map(|s| (s.y, s.h))
}
