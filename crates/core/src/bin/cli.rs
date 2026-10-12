//! Developer CLI for exercising the pipeline without the desktop app.
//! Data lives in $VODSCRIBE_DATA (default: ./vodscribe-data).

use std::sync::atomic::AtomicBool;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use vodscribe_core::{Paths, db::Db, fetch, model, pipeline, tools};

fn main() -> Result<()> {
    let paths = Paths::new(std::env::var("VODSCRIBE_DATA").unwrap_or_else(|_| "vodscribe-data".into()));
    paths.ensure()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cancel = AtomicBool::new(false);
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["setup"] => {
            if tools::find_yt_dlp(&paths.bin()).is_none() {
                let (p, _) = tools::install_yt_dlp(&paths.bin(), &cancel, |_, _, _| {})?;
                println!("installed {}", p.display());
            }
            model::install(&paths.models(), &model::CPU_MODEL, &cancel, |n, total| {
                eprint!("\rmodel {:.0}%", n as f64 / total as f64 * 100.0)
            })?;
            println!("\nready");
        }
        ["add" | "add-file", source] => {
            let yt_dlp = tools::find_yt_dlp(&paths.bin()).context("run `setup` first")?;
            let ffmpeg = tools::find_ffmpeg(&paths.bin()).context("ffmpeg not found")?;
            let mut db = Db::open(&paths.db())?;
            let vod = if args[0] == "add-file" {
                // A file on this computer, transcribed where it is.
                let info = vodscribe_core::local::probe(&ffmpeg, std::path::Path::new(source))?;
                let (vod, _) = db.add_vod(&info)?;
                db.set_source_path(vod.id, &info.url)?;
                db.vod(vod.id)?.context("just added")?
            } else {
                let info = fetch::probe(&yt_dlp, source, &pipeline::info_path(&paths.work(), 0))?;
                let (vod, _) = db.add_vod(&info)?;
                std::fs::rename(pipeline::info_path(&paths.work(), 0), pipeline::info_path(&paths.work(), vod.id))?;
                vod
            };
            println!("#{} {} ({:?}s)", vod.id, vod.info.title, vod.info.duration);
            let t = Instant::now();
            let transcription = pipeline::Transcription {
                worker: pipeline::WorkerSpec {
                    program: tools::sidecar("vodscribe-worker")?,
                    model_id: model::CPU_MODEL.id,
                    model_dir: paths.model_dir(),
                    provider: "cpu",
                    batch: 1,
                    lib_dirs: Vec::new(),
                },
                fallback: None,
            };
            let tools = pipeline::Tools { yt_dlp: &yt_dlp, ffmpeg: &ffmpeg };
            let mut report = |s, f: f64| eprint!("\r{s:?} {:5.1}%  {:6.1}s   ", f * 100.0, t.elapsed().as_secs_f64());
            let audio = pipeline::download(&mut db, &vod, &tools, &paths.work(), &cancel, &mut report)?;
            pipeline::transcribe(&mut db, &vod, &ffmpeg, &audio, &transcription, &cancel, &mut report, &mut |n| {
                eprintln!("\n{n}")
            })?;
            pipeline::remove_work_files(&paths.work(), vod.id);
            eprintln!();
        }
        ["probe", url] => {
            let yt_dlp = tools::find_yt_dlp(&paths.bin()).context("run `setup` first")?;
            let probe_path = paths.work().join("probe.json");
            let info = fetch::probe(&yt_dlp, url, &probe_path);
            let _ = std::fs::remove_file(&probe_path);
            let info = info?;
            println!("{}", serde_json::to_string_pretty(&info)?);
        }
        ["list"] => {
            for v in Db::open(&paths.db())?.vods()? {
                println!("#{} [{:?}] {} — {}", v.id, v.status, v.info.title, v.info.url);
            }
        }
        ["search", query @ ..] => {
            let r = Db::open(&paths.db())?.search(&query.join(" "), None, 50)?;
            for h in r.hits {
                let text: String = h.parts.iter().map(|p| if p.hit { format!("[{}]", p.text) } else { p.text.clone() }).collect();
                println!("#{} {:>8.1}s  {text}", h.vod_id, h.time);
            }
        }
        ["transcript", id] => {
            for s in Db::open(&paths.db())?.segments(id.parse()?)? {
                println!("{:>8.1}  {}", s.start, s.text);
            }
        }
        _ => bail!("usage: vodscribe-cli setup | add <url> | probe <url> | list | search <phrase> | transcript <id>"),
    }
    Ok(())
}
