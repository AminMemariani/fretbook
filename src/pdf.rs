//! Printing: lays the score out on paper-sized pages and writes a vector PDF
//! (lines, curves and real embedded fonts), ready for any printer.

use crate::gp::Song;
use crate::score::{self, PaintCtx, ViewOpts};
use eframe::egui::{self, epaint::ColorMode, Color32, Pos2, Rect, Shape};
use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Paper {
    A4,
    Letter,
}

impl Paper {
    pub fn size(self) -> (f32, f32) {
        match self {
            Paper::A4 => (595.28, 841.89),
            Paper::Letter => (612.0, 792.0),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Paper::A4 => "A4",
            Paper::Letter => "US Letter",
        }
    }
}

const MARGIN: f32 = 32.0;
/// Width of the score layout in screen units; scaled down to fit the page.
const LAYOUT_W: f32 = 820.0;

struct FontSlot {
    data: Vec<u8>,
    base: String,
    used: BTreeMap<u16, u16>, // glyph id -> advance in 1/1000 em
}

struct Fonts {
    slots: Vec<FontSlot>,
    /// family key -> slot index
    by_key: BTreeMap<String, usize>,
    fallback: Option<usize>,
}

fn read_font(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

impl Fonts {
    fn new(cjk: bool) -> Fonts {
        let mut f = Fonts { slots: Vec::new(), by_key: BTreeMap::new(), fallback: None };
        let sans = ["/System/Library/Fonts/Supplemental/Arial.ttf", "/Library/Fonts/Arial.ttf"];
        let serif = ["/System/Library/Fonts/Supplemental/Times New Roman.ttf"];
        let serif_b = ["/System/Library/Fonts/Supplemental/Times New Roman Bold.ttf"];
        for (key, paths, base) in [("sans", &sans[..], "Arial"), ("serif", &serif[..], "TimesNewRoman"), ("serif-bold", &serif_b[..], "TimesNewRoman-Bold")] {
            if let Some(data) = read_font(paths) {
                f.by_key.insert(key.into(), f.slots.len());
                f.slots.push(FontSlot { data, base: base.into(), used: BTreeMap::new() });
            }
        }
        if cjk {
            if let Some(data) = read_font(&["/System/Library/Fonts/Supplemental/Arial Unicode.ttf", "/Library/Fonts/Arial Unicode.ttf"]) {
                f.fallback = Some(f.slots.len());
                f.slots.push(FontSlot { data, base: "ArialUnicodeMS".into(), used: BTreeMap::new() });
            }
        }
        f
    }

    /// (slot, glyph id) for a character in a family.
    fn glyph(&mut self, key: &str, ch: char) -> Option<(usize, u16)> {
        let primary = self.by_key.get(key).or_else(|| self.by_key.get("sans")).copied();
        for slot in primary.into_iter().chain(self.fallback) {
            let s = &mut self.slots[slot];
            let Ok(face) = ttf_parser::Face::parse(&s.data, 0) else { continue };
            if let Some(g) = face.glyph_index(ch) {
                let upm = face.units_per_em().max(1) as f32;
                let adv = face.glyph_hor_advance(g).unwrap_or(0) as f32 * 1000.0 / upm;
                s.used.insert(g.0, adv.round() as u16);
                return Some((slot, g.0));
            }
        }
        None
    }
}

fn color_ops(c: Color32, fill: bool) -> String {
    let (r, g, b) = (c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0);
    format!("{r:.3} {g:.3} {b:.3} {}\n", if fill { "rg" } else { "RG" })
}

fn solid(c: &ColorMode) -> Color32 {
    match c {
        ColorMode::Solid(c) => *c,
        _ => Color32::BLACK,
    }
}

/// Turns painted shapes into PDF drawing commands. Coordinates: score units,
/// y down; the page transform is applied by the caller.
fn emit(shape: &Shape, out: &mut String, fonts: &mut Fonts) {
    match shape {
        Shape::Vec(v) => {
            for s in v {
                emit(s, out, fonts);
            }
        }
        Shape::LineSegment { points, stroke } => {
            if stroke.width <= 0.0 || stroke.color.a() == 0 {
                return;
            }
            out.push_str(&color_ops(stroke.color, false));
            let _ = writeln!(out, "{:.2} w {:.2} {:.2} m {:.2} {:.2} l S", stroke.width, points[0].x, points[0].y, points[1].x, points[1].y);
        }
        Shape::Path(p) => {
            if p.points.len() < 2 {
                return;
            }
            let mut path = String::new();
            let _ = write!(path, "{:.2} {:.2} m ", p.points[0].x, p.points[0].y);
            for q in &p.points[1..] {
                let _ = write!(path, "{:.2} {:.2} l ", q.x, q.y);
            }
            if p.closed {
                path.push_str("h ");
            }
            let has_fill = p.fill.a() > 0;
            let has_stroke = p.stroke.width > 0.0 && solid(&p.stroke.color).a() > 0;
            if has_fill {
                out.push_str(&color_ops(p.fill, true));
            }
            if has_stroke {
                out.push_str(&color_ops(solid(&p.stroke.color), false));
                let _ = write!(out, "{:.2} w ", p.stroke.width);
            }
            out.push_str(&path);
            out.push_str(match (has_fill, has_stroke) {
                (true, true) => "B\n",
                (true, false) => "f\n",
                (false, true) => "S\n",
                _ => "n\n",
            });
        }
        Shape::CubicBezier(b) => {
            let st = &b.stroke;
            if st.width <= 0.0 {
                return;
            }
            out.push_str(&color_ops(solid(&st.color), false));
            let p = &b.points;
            let _ = writeln!(
                out,
                "{:.2} w {:.2} {:.2} m {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c S",
                st.width, p[0].x, p[0].y, p[1].x, p[1].y, p[2].x, p[2].y, p[3].x, p[3].y
            );
        }
        Shape::Circle(c) => {
            let k = 0.5523 * c.radius;
            let (x, y, r) = (c.center.x, c.center.y, c.radius);
            let mut path = String::new();
            let _ = write!(
                path,
                "{:.2} {:.2} m {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c {:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c h ",
                x + r, y,
                x + r, y + k, x + k, y + r, x, y + r,
                x - k, y + r, x - r, y + k, x - r, y,
                x - r, y - k, x - k, y - r, x, y - r,
                x + k, y - r, x + r, y - k, x + r, y
            );
            let has_fill = c.fill.a() > 0;
            let has_stroke = c.stroke.width > 0.0 && c.stroke.color.a() > 0;
            if has_fill {
                out.push_str(&color_ops(c.fill, true));
            }
            if has_stroke {
                out.push_str(&color_ops(c.stroke.color, false));
                let _ = write!(out, "{:.2} w ", c.stroke.width);
            }
            out.push_str(&path);
            out.push_str(match (has_fill, has_stroke) {
                (true, true) => "B\n",
                (true, false) => "f\n",
                (false, true) => "S\n",
                _ => "n\n",
            });
        }
        Shape::Rect(r) => {
            let rc = r.rect;
            if r.fill.a() > 0 {
                out.push_str(&color_ops(r.fill, true));
                let _ = writeln!(out, "{:.2} {:.2} {:.2} {:.2} re f", rc.min.x, rc.min.y, rc.width(), rc.height());
            }
            if r.stroke.width > 0.0 && r.stroke.color.a() > 0 {
                out.push_str(&color_ops(r.stroke.color, false));
                let _ = writeln!(out, "{:.2} w {:.2} {:.2} {:.2} {:.2} re S", r.stroke.width, rc.min.x, rc.min.y, rc.width(), rc.height());
            }
        }
        Shape::Text(t) => {
            let Some(sec) = t.galley.job.sections.first() else { return };
            let fmt = &sec.format;
            let size = fmt.font_id.size;
            let key = match &fmt.font_id.family {
                egui::FontFamily::Name(n) if n.as_ref() == "score-bold" => "serif-bold",
                egui::FontFamily::Name(n) if n.as_ref() == "score" => "serif",
                _ => "sans",
            };
            let mut color = t.override_text_color.unwrap_or(fmt.color);
            if color == Color32::PLACEHOLDER {
                color = t.fallback_color;
            }
            out.push_str(&color_ops(color, true));
            out.push_str("BT\n");
            let mut cur_font = usize::MAX;
            for row in &t.galley.rows {
                for g in &row.row.glyphs {
                    if g.chr.is_whitespace() {
                        continue;
                    }
                    let Some((slot, gid)) = fonts.glyph(key, g.chr) else { continue };
                    if slot != cur_font {
                        let _ = writeln!(out, "/F{slot} 1 Tf");
                        cur_font = slot;
                    }
                    let x = t.pos.x + row.pos.x + g.pos.x;
                    let y = t.pos.y + row.pos.y + g.pos.y;
                    // flip the text back upright inside the y-down page transform
                    let _ = writeln!(out, "{size:.2} 0 0 {:.2} {x:.2} {y:.2} Tm <{gid:04X}> Tj", -size);
                }
            }
            out.push_str("ET\n");
        }
        _ => {}
    }
}

/// Collects what a paint closure draws, without showing it on screen.
fn capture(ctx: &egui::Context, f: impl FnOnce(&egui::Painter)) -> Vec<Shape> {
    let layer = egui::LayerId::new(egui::Order::Background, egui::Id::unique("print-capture"));
    let big = Rect::from_min_max(Pos2::new(-10000.0, -10000.0), Pos2::new(100000.0, 100000.0));
    let painter = egui::Painter::new(ctx.clone(), layer, big);
    f(&painter);
    let list = ctx.graphics_mut(|g| std::mem::take(g.entry(layer)));
    list.all_entries().map(|c| c.shape.clone()).collect()
}

pub struct PrintJob<'a> {
    pub song: &'a Song,
    pub tracks: Vec<usize>,
    pub notation: bool,
    pub tab: bool,
    pub paper: Paper,
    pub file_name: String,
}

/// Number of pages a job will fill.
pub fn page_breaks(job: &PrintJob) -> (score::Layout, Vec<(f32, f32, bool)>, f32) {
    let (pw, ph) = job.paper.size();
    let k = (pw - 2.0 * MARGIN) / LAYOUT_W;
    let opts = ViewOpts { zoom: 1.0, notation: job.notation, tab: job.tab, tracks: job.tracks.clone(), width: LAYOUT_W };
    let layout = score::layout(job.song, &opts);
    let usable = (ph - 2.0 * MARGIN - 12.0) / k;
    // pages as (start y, end y, has header) in layout units
    let mut pages = Vec::new();
    let mut start = 0.0f32;
    let mut end = layout.header_h;
    let mut header = true;
    for sys in &layout.systems {
        // the last 10 units of a system are spacing, which may run into the margin
        if sys.y + sys.h - 10.0 - start > usable && end > start + 1.0 {
            pages.push((start, end, header));
            start = sys.y - 6.0;
            header = false;
        }
        end = sys.y + sys.h;
    }
    pages.push((start, end, header));
    (layout, pages, k)
}

pub fn make_pdf(ctx: &egui::Context, job: &PrintJob) -> Vec<u8> {
    let was_dark = score::is_dark();
    score::set_dark(false);
    let (pw, ph) = job.paper.size();
    let (layout, pages, k) = page_breaks(job);
    let cjk = [&job.song.title, &job.song.artist, &job.song.subtitle]
        .iter()
        .any(|t| t.chars().any(|c| c as u32 >= 0x2E80))
        || job.song.tracks.iter().any(|t| t.name.chars().any(|c| c as u32 >= 0x2E80));
    let mut fonts = Fonts::new(cjk);
    let n = pages.len();
    let footer_title = if job.song.title.is_empty() { job.file_name.clone() } else { job.song.title.clone() };
    let mut contents = Vec::new();
    for (i, &(start, end, header)) in pages.iter().enumerate() {
        let shapes = capture(ctx, |p| {
            let pc = PaintCtx {
                song: job.song,
                origin: Pos2::new(0.0, -start),
                clip: Rect::from_min_max(Pos2::new(-1000.0, if header { -1000.0 } else { 0.5 }), Pos2::new(10000.0, end - start - 0.5)),
                cursor: None,
                selection: None,
                playing_bar: None,
                header,
            };
            score::paint(p, &layout, &pc);
            let usable = (ph - 2.0 * MARGIN) / k;
            p.text(
                Pos2::new(LAYOUT_W / 2.0, usable),
                egui::Align2::CENTER_BOTTOM,
                format!("{footer_title}  —  page {} of {n}", i + 1),
                egui::FontId::new(10.0, egui::FontFamily::Name("score".into())),
                Color32::from_gray(110),
            );
        });
        let mut c = String::new();
        // y-down coordinates in layout units -> PDF points
        let _ = writeln!(c, "1 J 1 j\n{k:.5} 0 0 {:.5} {MARGIN:.2} {:.2} cm", -k, ph - MARGIN);
        for s in &shapes {
            emit(s, &mut c, &mut fonts);
        }
        contents.push(c);
    }
    score::set_dark(was_dark);
    write_pdf(pw, ph, &contents, &fonts, &footer_title)
}

fn pdf_escape(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii() && !c.is_control())
        .flat_map(|c| match c {
            '(' | ')' | '\\' => vec!['\\', c],
            _ => vec![c],
        })
        .collect()
}

fn write_pdf(pw: f32, ph: f32, pages: &[String], fonts: &Fonts, title: &str) -> Vec<u8> {
    let mut objs: Vec<Vec<u8>> = Vec::new(); // object n is objs[n-1]
    let reserve = |objs: &mut Vec<Vec<u8>>| {
        objs.push(Vec::new());
        objs.len()
    };
    let catalog = reserve(&mut objs);
    let pages_id = reserve(&mut objs);
    let info = reserve(&mut objs);

    // fonts actually used
    let mut font_refs = String::new();
    for (i, f) in fonts.slots.iter().enumerate() {
        if f.used.is_empty() {
            continue;
        }
        let face = ttf_parser::Face::parse(&f.data, 0).ok();
        let upm = face.as_ref().map(|x| x.units_per_em() as f32).unwrap_or(1000.0);
        let sc = |v: f32| (v * 1000.0 / upm).round() as i32;
        let (asc, desc, bbox) = match &face {
            Some(fc) => {
                let b = fc.global_bounding_box();
                (sc(fc.ascender() as f32), sc(fc.descender() as f32), [sc(b.x_min as f32), sc(b.y_min as f32), sc(b.x_max as f32), sc(b.y_max as f32)])
            }
            None => (800, -200, [0, -200, 1000, 900]),
        };
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&f.data, 6);
        let file_id = reserve(&mut objs);
        let mut file = format!("<< /Length {} /Length1 {} /Filter /FlateDecode >>\nstream\n", compressed.len(), f.data.len()).into_bytes();
        file.extend(&compressed);
        file.extend(b"\nendstream");
        objs[file_id - 1] = file;
        let desc_id = reserve(&mut objs);
        objs[desc_id - 1] = format!(
            "<< /Type /FontDescriptor /FontName /{} /Flags 32 /FontBBox [{} {} {} {}] /ItalicAngle 0 /Ascent {asc} /Descent {desc} /CapHeight {asc} /StemV 80 /FontFile2 {file_id} 0 R >>",
            f.base, bbox[0], bbox[1], bbox[2], bbox[3]
        )
        .into_bytes();
        let mut w = String::new();
        for (g, adv) in &f.used {
            let _ = write!(w, "{g} [{adv}] ");
        }
        let cid_id = reserve(&mut objs);
        objs[cid_id - 1] = format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {desc_id} 0 R /DW 1000 /W [{w}] /CIDToGIDMap /Identity >>",
            f.base
        )
        .into_bytes();
        let font_id = reserve(&mut objs);
        objs[font_id - 1] = format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /{} /Encoding /Identity-H /DescendantFonts [{cid_id} 0 R] >>",
            f.base
        )
        .into_bytes();
        let _ = write!(font_refs, "/F{i} {font_id} 0 R ");
    }

    let mut kids = String::new();
    for content in pages {
        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(content.as_bytes(), 6);
        let cid = reserve(&mut objs);
        let mut obj = format!("<< /Length {} /Filter /FlateDecode >>\nstream\n", compressed.len()).into_bytes();
        obj.extend(&compressed);
        obj.extend(b"\nendstream");
        objs[cid - 1] = obj;
        let pid = reserve(&mut objs);
        objs[pid - 1] = format!(
            "<< /Type /Page /Parent {pages_id} 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] /Resources << /Font << {font_refs}>> >> /Contents {cid} 0 R >>"
        )
        .into_bytes();
        let _ = write!(kids, "{pid} 0 R ");
    }
    objs[catalog - 1] = format!("<< /Type /Catalog /Pages {pages_id} 0 R >>").into_bytes();
    objs[pages_id - 1] = format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()).into_bytes();
    objs[info - 1] = format!("<< /Title ({}) /Producer (Fretbook) >>", pdf_escape(title)).into_bytes();

    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend(o);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for off in offsets {
        out.extend(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root {catalog} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

// ----------------------------------------------------------------------------
// Printers (macOS / CUPS)

pub fn printers() -> (Vec<String>, Option<String>) {
    let list = std::process::Command::new("lpstat")
        .arg("-p")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let names: Vec<String> = list
        .lines()
        .filter_map(|l| l.strip_prefix("printer ").and_then(|r| r.split_whitespace().next()).map(|s| s.to_string()))
        .collect();
    let default = std::process::Command::new("lpstat")
        .arg("-d")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).split(':').nth(1).map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty() && names.contains(s));
    (names, default)
}

pub fn send_to_printer(printer: &str, copies: u32, paper: Paper, file: &std::path::Path) -> Result<(), String> {
    let media = match paper {
        Paper::A4 => "A4",
        Paper::Letter => "Letter",
    };
    let out = std::process::Command::new("lp")
        .arg("-d")
        .arg(printer)
        .arg("-n")
        .arg(copies.max(1).to_string())
        .arg("-o")
        .arg(format!("media={media}"))
        .arg("-o")
        .arg("fit-to-page")
        .arg(file)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}
