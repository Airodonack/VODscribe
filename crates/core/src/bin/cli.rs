//! Developer CLI for exercising the pipeline without the desktop app.
//! Data lives in $VODSCRIBE_DATA (default: ./vodscribe-data).

use std::sync::atomic::AtomicBool;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use vodscribe_core::{Paths, chat, db::Db, fetch, model, peaks, pipeline, tools};

fn main() -> Result<()> {
    let paths = Paths::new(std::env::var("VODSCRIBE_DATA").unwrap_or_else(|_| "vodscribe-data".into()));
    paths.ensure()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cancel = AtomicBool::new(false);
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["setup"] => {
            if tools::find_yt_dlp(&paths.bin()).is_none() {
                let (p, _) = tools::install_yt_dlp(&paths.bin(), &cancel, |_| {})?;
                println!("installed {}", p.display());
            }
            model::install(&paths.models(), &model::CPU_MODEL, &cancel, |p| {
                eprint!("\rmodel {:.0}%", p.done as f64 / p.total.unwrap_or(1) as f64 * 100.0)
            })?;
            println!("\nready");
        }
        ["add" | "add-file", source] => {
            let yt_dlp = tools::find_yt_dlp(&paths.bin());
            let ffmpeg = tools::find_ffmpeg(&paths.bin()).context("ffmpeg not found")?;
            let mut db = Db::open(&paths.db())?;
            let vod = if args[0] == "add-file" {
                // A file on this computer, transcribed where it is.
                let info = vodscribe_core::local::probe(&ffmpeg, std::path::Path::new(source))?;
                let (vod, _) = db.add_vod(&info)?;
                db.set_source_path(vod.id, &info.url)?;
                db.vod(vod.id)?.context("just added")?
            } else {
                let yt_dlp = yt_dlp.as_ref().context("run `setup` first")?;
                let info =
                    fetch::probe(yt_dlp, source, &pipeline::info_path(&paths.work(), 0), &AtomicBool::new(false))?;
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
            let tools = pipeline::Tools { yt_dlp: yt_dlp.as_ref(), ffmpeg: &ffmpeg };
            let mut report = |s, f: f64| eprint!("\r{s:?} {:5.1}%  {:6.1}s   ", f * 100.0, t.elapsed().as_secs_f64());
            let stretch = db.request(vod.id, 0.0, None)?;
            let audio =
                pipeline::audio_for(&mut db, &vod, &stretch, &tools, &paths.work(), None, &cancel, &mut report)?
                    .audio();
            let stretch = db.stretch(stretch.id)?.context("just asked for")?;
            pipeline::transcribe(
                &mut db,
                &vod,
                &stretch,
                &ffmpeg,
                &audio,
                &transcription,
                &cancel,
                &mut report,
                &mut |n| eprintln!("\n{n}"),
            )?;
            pipeline::remove_work_files(&paths.work(), vod.id);
            eprintln!();
        }
        ["import-annotations", file, rest @ ..] if rest.len() <= 1 => {
            // An annotations file (and its chat: the one given, or the one beside it).
            let chat = rest.first().map(std::path::Path::new);
            let (annotations, chat) = vodscribe_core::annotations::read(std::path::Path::new(file), chat)?;
            let db = Db::open(&paths.db())?;
            let (vod, created) = db.import_annotations(&annotations, chat.as_deref())?;
            println!("#{} {} ({})", vod.id, vod.info.title, if created { "added" } else { "updated" });
        }
        ["probe", url] => {
            let yt_dlp = tools::find_yt_dlp(&paths.bin()).context("run `setup` first")?;
            let probe_path = paths.work().join("probe.json");
            let info = fetch::probe(&yt_dlp, url, &probe_path, &AtomicBool::new(false));
            let _ = std::fs::remove_file(&probe_path);
            let info = info?;
            println!("{}", serde_json::to_string_pretty(&info)?);
        }
        ["chat", url] => {
            let yt_dlp = tools::find_yt_dlp(&paths.bin()).context("yt-dlp not found; run setup")?;
            let info = fetch::probe(&yt_dlp, url, &paths.work().join("cli.info.json"), &AtomicBool::new(false))?;
            let started = Instant::now();
            let mut store = chat::MemoryChat::default();
            let found = chat::fetch(&info, Some(&yt_dlp), &paths.work(), 0, &mut store, &cancel, |f| {
                eprint!("\rchat {:.0}%", f * 100.0)
            })?;
            eprintln!();
            if !found {
                bail!("no chat replay for this VOD");
            }
            let mut messages = store.messages;
            messages.sort_by(|a, b| a.time.total_cmp(&b.time));
            for m in messages.iter().take(5).chain(messages.iter().rev().take(5).rev()) {
                println!("{:>9.1}  {}: {}", m.time, m.author, m.text());
            }
            println!("{} messages in {:.1?}", messages.len(), started.elapsed());
            let mut kinds = std::collections::BTreeMap::new();
            for m in &messages {
                *kinds.entry(m.kind.as_str()).or_insert(0) += 1;
            }
            println!("{kinds:?}");
            let starts: Vec<f64> = messages.iter().filter_map(chat::ChatMessage::start).collect();
            let by_chat = chat::vod_start(&starts).map(vodscribe_core::db::utc_time);
            println!("started {by_chat:?} by the chat ({} messages), {:?} by the site", starts.len(), info.started_at);
            for m in messages.iter().filter(|m| m.kind != chat::Kind::Message).take(8) {
                println!("{:>9.1}  {:?} {:?}  {}: {}", m.time, m.kind, m.amount, m.author, m.text());
            }
        }
        ["list"] => {
            for v in Db::open(&paths.db())?.vods()? {
                println!("#{} [{:?}] {} — {}", v.id, v.status, v.info.title, v.info.url);
            }
        }
        ["search", query @ ..] => {
            let r = Db::open(&paths.db())?.search(&query.join(" "), &Default::default(), 50)?;
            for h in r.hits {
                let text: String =
                    h.parts.iter().map(|p| if p.hit { format!("[{}]", p.text) } else { p.text.clone() }).collect();
                println!("#{} {:>8.1}s  {text}", h.vod_id, h.time);
            }
        }
        ["peaks", input, out] => {
            // A file's waveform as the app draws it (the site's demo ships these).
            let ffmpeg = tools::find_ffmpeg(&paths.bin()).context("ffmpeg not found")?;
            let levels = peaks::read(&ffmpeg, input.as_ref(), &cancel)?;
            std::fs::write(out, &levels)?;
            println!("{} levels ({:.0} s)", levels.len(), levels.len() as f64 / peaks::RATE as f64);
        }
        ["transcript", id] => {
            for s in Db::open(&paths.db())?.segments(id.parse()?)? {
                println!("{:>8.1}  {}", s.start, s.text);
            }
        }
        _ => bail!(
            "usage: vodscribe-cli setup | add <url> | probe <url> | chat <url> | list | search <phrase> | transcript <id> | peaks <file> <out> | import-annotations <file.json> [<chat.json>]"
        ),
    }
    Ok(())
}
