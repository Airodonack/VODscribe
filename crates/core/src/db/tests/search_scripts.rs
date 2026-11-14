//! Search finds words in every script, as the library's index keeps them.

use super::*;

/// What is said and written in Korean, Greek, Japanese or with an emoji is
/// found by typing it as it was written, in the transcript and in chat
/// (search.rs: "run ... against what was said and what chat wrote";
/// transcript.rs `terms`: "lowercase, accents off Latin letters, and every
/// other script as written"). Accents on Latin letters still don't count.
#[test]
fn words_in_every_script_are_found_as_written() {
    let mut db = Db::open(Path::new(":memory:")).unwrap();
    let vod = db.add_vod(&info("a")).unwrap().0.id;
    let said = ["오늘 한국어 방송.", "Τα ελληνικά μου.", "これ ガンダム です.", "the 🦅 landed.", "Le café ferme."];
    let mut seg = Segmenter::new();
    let mut lines = Vec::new();
    for (i, text) in said.iter().enumerate() {
        lines.extend(seg.push(words(text, i as f64 * 10.0)));
    }
    lines.extend(seg.finish());
    db.insert_segments(vod, &lines).unwrap();
    let chat: Vec<_> = said.iter().enumerate().map(|(i, text)| message(i as f64 * 10.0 + 5.0, "v", text)).collect();
    db.add_chat(vod, &chat, &[]).unwrap();

    let found = |query: &str, speech: bool| {
        let filter = SearchFilter { speech, chat: !speech, ..Default::default() };
        db.search(query, &filter, 10).unwrap().hits.iter().map(|h| h.time).collect::<Vec<_>>()
    };
    let mut missed = Vec::new();
    let typed = [("한국어", 0.0), ("ελληνικά", 10.0), ("ガンダム", 20.0), ("🦅", 30.0), ("cafe", 40.0)];
    for (query, at) in typed {
        // Each is the second word of its line, and chat wrote it 5 s in.
        for (speech, time, in_) in [(true, at + 1.0, "speech"), (false, at + 5.0, "chat")] {
            let hits = found(query, speech);
            if hits != [time] {
                missed.push(format!("{query} in {in_}: {hits:?}"));
            }
        }
        let layer: Vec<f64> = db.chat_matches(vod, &[query.into()]).unwrap().into_iter().map(|(_, t)| t).collect();
        if layer != [at + 5.0] {
            missed.push(format!("{query} on the chat layer: {layer:?}"));
        }
    }
    assert!(missed.is_empty(), "{missed:#?}");
}
