mod app;
mod demo;
mod gp;
#[cfg(target_os = "macos")]
mod macmenu;
mod midi;
mod pdf;
mod score;
mod synth;
mod timing;

use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    // Developer check: read every file listed in a text file and report.
    if args.len() > 2 && args[1] == "--scan" {
        scan(&args[2]);
        return Ok(());
    }
    if args.len() > 3 && args[1] == "--render" {
        let song = gp::read_song_file(std::path::Path::new(&args[2])).expect("read");
        let tl = timing::build(&song);
        let secs = tl.total.min(30.0);
        let t = std::time::Instant::now();
        let sampled = std::env::var("FRETBOOK_SIMPLE").is_err();
        let out = synth::render_offline(&tl, secs, 44100.0, sampled);
        let el = t.elapsed().as_secs_f64();
        let peak = out.iter().fold(0f32, |a, b| a.max(b.abs()));
        let nan = out.iter().any(|x| !x.is_finite());
        let rms = (out.iter().map(|x| x * x).sum::<f32>() / out.len().max(1) as f32).sqrt();
        println!("rendered {secs:.1}s in {el:.2}s  peak {peak:.3} rms {rms:.4} nan {nan}");
        let samples: Vec<i16> = out.iter().map(|x| (x.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
        let mut wav = Vec::new();
        let data_len = (samples.len() * 2) as u32;
        wav.extend(b"RIFF");
        wav.extend((36 + data_len).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(2u16.to_le_bytes());
        wav.extend(44100u32.to_le_bytes());
        wav.extend((44100u32 * 4).to_le_bytes());
        wav.extend(4u16.to_le_bytes());
        wav.extend(16u16.to_le_bytes());
        wav.extend(b"data");
        wav.extend(data_len.to_le_bytes());
        for s in samples {
            wav.extend(s.to_le_bytes());
        }
        std::fs::write(&args[3], wav).ok();
        return Ok(());
    }
    let mut screenshot: Option<PathBuf> = None;
    let mut open: Option<PathBuf> = None;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--screenshot" && i + 1 < args.len() {
            screenshot = Some(PathBuf::from(&args[i + 1]));
            i += 2;
            continue;
        }
        if !args[i].starts_with("-psn") && !args[i].starts_with("--") {
            open = Some(PathBuf::from(&args[i]));
        }
        i += 1;
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Fretbook")
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([800.0, 500.0]);
    let shot = screenshot.is_some();
    if shot {
        viewport = viewport.with_active(false);
        #[cfg(target_os = "windows")]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    if shot {
        options.run_and_return = true;
        #[cfg(target_os = "macos")]
        {
            options.event_loop_builder = Some(Box::new(|b| {
                use winit::platform::macos::EventLoopBuilderExtMacOS;
                b.with_activate_ignoring_other_apps(false);
            }));
        }
    }
    eframe::run_native(
        "Fretbook",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, open, screenshot)))),
    )
}

fn scan(list: &str) {
    let mut ok = 0;
    let mut bad = 0;
    let list = std::fs::read_to_string(list).unwrap_or_default();
    for line in list.lines() {
        let p = std::path::Path::new(line);
        match gp::read_song_file(p) {
            Ok(s) => {
                ok += 1;
                // also exercise the timeline and layout code
                let tl = timing::build(&s);
                let opts = score::ViewOpts { zoom: 1.0, notation: true, tab: true, tracks: (0..s.tracks.len()).collect(), width: 1000.0 };
                let l = score::layout(&s, &opts);
                println!(
                    "OK {} | {} | {} | tracks {} bars {} events {} secs {:.0} systems {} trail {}",
                    line,
                    s.title,
                    s.artist,
                    s.tracks.len(),
                    s.headers.len(),
                    tl.events.len(),
                    tl.total,
                    l.systems.len(),
                    s.trailing
                );
            }
            Err(e) => {
                bad += 1;
                println!("ERR {} : {}", line, e);
            }
        }
    }
    println!("ok {ok} bad {bad}");
}
