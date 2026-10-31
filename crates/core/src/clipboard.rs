//! What Ctrl+C copies and Ctrl+V puts back: copies, not references. A copy
//! (`Copied`) is what a clip or a stretch was at the moment it was copied,
//! as undo keeps a clip (`EditState`: its title, pieces and marks, no ids),
//! so changing or deleting the original afterwards changes nothing pasted.
//! Pasted on a clip bar, each copy is a new clip there; on an edit's
//! timeline, it goes in at a moment of it (`Paste::Timeline`), a copied clip
//! as a group (one band, as a clip dropped there), a copied stretch as
//! pieces.

use anyhow::{Context, Result, bail};
use rusqlite::OptionalExtension;

use crate::db::Db;
use crate::edits::{EditState, ItemState, MarkAt, Place, Put, SLIVER, spans, struck_flicks};
use crate::time::Flicks;

/// What a copy goes in a timeline as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopiedKind {
    /// A clip (a pill of a clip bar): one band, a group named for it.
    Clip,
    /// A stretch of a timeline: its pieces, as they are.
    Stretch,
}

/// A clip or a stretch as it was when copied.
#[derive(Debug, Clone, PartialEq)]
pub struct Copied {
    pub kind: CopiedKind,
    pub state: EditState,
}

/// Where `Db::paste` puts copies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Paste {
    /// On listed clip `bar`'s clip bar (a VOD's own clip, or an edit), each a
    /// clip of its own, after the others.
    Bar(i64),
    /// In clip `clip`'s timeline (an edit's, or a snippet open in the
    /// editor) at `at` seconds of it; with `replace`, that stretch of it
    /// taken out first and the copies put where it began.
    Timeline { clip: i64, at: f64, replace: Option<(f64, f64)> },
}

/// What a paste did.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Pasted {
    /// The clips made on the bar, in order, each with the index in the
    /// copies of the one it's of.
    pub clips: Vec<(i64, usize)>,
    /// On a timeline: the stretch of it the copies now play.
    pub span: Option<(f64, f64)>,
    /// How many pieces were left out, their VOD gone since the copy.
    pub dropped: usize,
    /// How many copies weren't pasted: pieces of another VOD, for a VOD's own clip bar.
    pub refused: usize,
}

impl Db {
    /// `item` (what's selected or picked, as a drop names it) copied as it
    /// is now: a clip whole (`edit_state`), a stretch of a VOD or of an
    /// edit's timeline as the pieces that play there, with the chapters and
    /// highlights inside it.
    pub fn copy_of(&self, item: &Put) -> Result<Copied> {
        if let Put::Clip { id } = item {
            let state = self.edit_state(*id)?.context("That clip is gone.")?;
            return Ok(Copied { kind: CopiedKind::Clip, state });
        }
        let (title, parts) = self.put_parts(item)?;
        let mut items = Vec::with_capacity(parts.len());
        for p in &parts {
            let (start, end) = self.within_vod(p.vod_id, p.start, p.end)?;
            items.push(ItemState {
                parent: None,
                vod_id: Some(p.vod_id),
                start: Some(start),
                end: Some(end),
                title: p.title.clone(),
                struck: struck_flicks(&p.struck),
                origin: None,
            });
        }
        let marks = match item {
            Put::Span { clip, from, to } => self.marks_inside(*clip, &self.edit_pieces(*clip)?, *from, *to)?,
            _ => {
                let mut marks = Vec::new();
                for i in &items {
                    let (Some(vod), Some(start), Some(end)) = (i.vod_id, i.start, i.end) else { continue };
                    marks.extend(self.vod_marks_in(vod, start, end)?);
                }
                marks
            }
        };
        // A highlight goes in a timeline as a clip's pill does: a band named for it.
        let kind = if matches!(item, Put::Highlight { .. }) { CopiedKind::Clip } else { CopiedKind::Stretch };
        Ok(Copied { kind, state: EditState { title, items, marks } })
    }

    /// VOD `vod_id`'s chapters and highlights from `start` to `end`, as
    /// `copy_vod_marks` would copy them: read, not written.
    pub fn vod_marks_in(&self, vod_id: i64, start: Flicks, end: Flicks) -> Result<Vec<MarkAt>> {
        let Some(own) = self.vod_clip(vod_id)? else { return Ok(Vec::new()) };
        Ok(self
            .clip_marks(own)?
            .into_iter()
            .map(|(_, m)| m)
            .filter(|m| m.kind != "cut" && m.at >= start && m.at < end)
            .collect())
    }

    /// Whether VOD `vod_id` is gone for a paste: no row, or taken out of the
    /// library with nothing using it (its file erased, its row kept only
    /// till the next start): a piece of it pasted would be a hole.
    fn gone_for_paste(&self, vod_id: i64) -> Result<bool> {
        Ok(self.vod(vod_id)?.is_none() || (self.vod_clip(vod_id)?.is_none() && self.uses(vod_id)? <= 0))
    }

    /// `state` less its pieces whose VOD is gone (`gone_for_paste`), and
    /// the groups left empty, with its marks on those VODs; and how many
    /// pieces went.
    fn present(&self, state: &EditState) -> Result<(EditState, usize)> {
        let mut keep = Vec::with_capacity(state.items.len());
        for item in &state.items {
            keep.push(match item.vod_id {
                Some(vod) => !self.gone_for_paste(vod)?,
                None => false,
            });
        }
        let dropped = state.items.iter().zip(&keep).filter(|(i, k)| i.vod_id.is_some() && !**k).count();
        // A group stays if anything in it does: what's in a group comes after it.
        for i in (0..state.items.len()).rev() {
            if let (true, Some(p)) = (keep[i], state.items[i].parent) {
                keep[p] = true;
            }
        }
        let mut at = vec![None; state.items.len()];
        let mut items = Vec::new();
        for (i, item) in state.items.iter().enumerate() {
            if keep[i] {
                at[i] = Some(items.len());
                items.push(ItemState { parent: item.parent.and_then(|p| at[p]), ..item.clone() });
            }
        }
        let vods: Vec<i64> = items.iter().filter_map(|i| i.vod_id).collect();
        let marks = state.marks.iter().filter(|m| vods.contains(&m.vod_id)).cloned().collect();
        Ok((EditState { title: state.title.clone(), items, marks }, dropped))
    }

    /// The VOD whose own clip bar clip `id` is, or whose bar the snippet
    /// `id` is on: copies of other VODs' footage don't go there.
    fn vod_of_bar(&self, id: i64) -> Result<Option<i64>> {
        let row: Option<(Option<i64>, Option<i64>)> = self
            .conn
            .query_row(
                "SELECT c.whole_of, b.whole_of FROM clips c LEFT JOIN clips b ON b.id = c.bar_of WHERE c.id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (whole, bar) = row.context("That clip is gone.")?;
        Ok(whole.or(bar))
    }

    /// Puts `items` (copies, in their order) where `onto` says, as one
    /// change: all of them, or none if one can't go. Pieces whose VOD is
    /// gone since the copy are left out (`Pasted::dropped`); an error if
    /// nothing's left. On a VOD's clip bar, or in a clip of it open in the
    /// editor, only copies of that VOD's own footage go (`Pasted::refused`);
    /// an error if none are.
    pub fn paste(&self, onto: Paste, items: &[Copied]) -> Result<Pasted> {
        let mut out = Pasted::default();
        let mut kept = Vec::with_capacity(items.len());
        for (of, c) in items.iter().enumerate() {
            let (state, dropped) = self.present(&c.state)?;
            out.dropped += dropped;
            if state.items.iter().any(|i| i.vod_id.is_some()) {
                kept.push((of, Copied { kind: c.kind, state }));
            }
        }
        if kept.is_empty() {
            bail!("The VOD it was copied from is gone.");
        }
        let target = match onto {
            Paste::Bar(bar) => bar,
            Paste::Timeline { clip, .. } => clip,
        };
        let whole_of: Option<i64> = self
            .conn
            .query_row("SELECT whole_of FROM clips WHERE id = ?1", [target], |r| r.get(0))
            .optional()?
            .flatten();
        if let (Paste::Timeline { .. }, Some(_)) = (onto, whole_of) {
            bail!("A VOD's timeline takes no pieces: paste on its clip bar.");
        }
        if let Some(vod) = self.vod_of_bar(target)? {
            let before = kept.len();
            kept.retain(|(_, c)| c.state.items.iter().all(|i| i.vod_id.is_none_or(|v| v == vod)));
            out.refused = before - kept.len();
            if kept.is_empty() {
                bail!("Clips of another VOD go in an edit.");
            }
        }
        match onto {
            Paste::Bar(bar) => {
                out.clips = self.change(bar, None, |db| {
                    let mut made = Vec::with_capacity(kept.len());
                    for (of, c) in &kept {
                        let id = db.new_clip(&c.state.title, Place::Bar(bar))?;
                        db.restore_edit(id, &c.state)?;
                        made.push((id, *of));
                    }
                    Ok(made)
                })?;
            }
            Paste::Timeline { clip, at, replace } => {
                let kept: Vec<Copied> = kept.into_iter().map(|(_, c)| c).collect();
                out.span = Some(self.change(clip, None, |db| db.paste_in(clip, at, replace, &kept))?);
            }
        }
        Ok(out)
    }

    /// `Paste::Timeline`'s writes: the stretch replaced taken out, the piece
    /// at the moment split there, the copies in. The stretch they play.
    fn paste_in(&self, clip: i64, at: f64, replace: Option<(f64, f64)>, items: &[Copied]) -> Result<(f64, f64)> {
        let at = match replace {
            Some((from, to)) => {
                self.remove_edit_stretch(clip, from, to)?;
                from
            }
            None => at,
        };
        let (mut index, start) = self.insertion(clip, at)?;
        let mut length = 0.0;
        for c in items {
            length += self.played_of(&c.state)?;
            match c.kind {
                CopiedKind::Clip => {
                    let marks = group_marks(&c.state);
                    self.insert_group(clip, &EditState { marks, ..c.state.clone() }, Some(index))?;
                    index += 1;
                }
                CopiedKind::Stretch => {
                    for id in self.insert_items(clip, &c.state, None)? {
                        self.place(clip, id, Some(index))?;
                        index += 1;
                    }
                    for m in &c.state.marks {
                        self.insert_mark_with(clip, m.vod_id, &m.kind, m.at, m.text.as_deref(), &m.detail)?;
                    }
                }
            }
        }
        Ok((start, start + length))
    }

    /// Where a paste at `at` seconds of clip `clip`'s timeline goes: the
    /// index of its top level, and the moment that is. As pasted text goes in
    /// at the caret: the piece there is split at `at` (nothing split within a
    /// sliver of its ends); inside a group (a clip dropped in, one band), at
    /// its nearer edge, the group left whole.
    fn insertion(&self, clip: i64, at: f64) -> Result<(usize, f64)> {
        let pieces = self.edit_pieces(clip)?;
        let top: Vec<i64> = self.item_rows(clip)?.into_iter().filter(|i| i.parent.is_none()).map(|i| i.id).collect();
        let index_of = |unit: i64| top.iter().position(|&id| id == unit).unwrap_or(top.len());
        // Each band on the timeline: its item at the top level, where it is,
        // and the piece it is if it's one alone.
        let mut bands: Vec<(i64, f64, f64, Option<usize>)> = Vec::new();
        let mut t = 0.0;
        for (i, p) in pieces.iter().enumerate() {
            let length: f64 = p.spans().iter().map(|(a, b)| b - a).sum();
            let unit = p.group.as_ref().map_or(p.id, |g| g.id);
            match bands.last_mut() {
                Some(band) if band.0 == unit => band.2 += length,
                _ => bands.push((unit, t, t + length, p.group.is_none().then_some(i))),
            }
            t += length;
        }
        for &(unit, start, end, piece) in &bands {
            if at <= start + SLIVER {
                return Ok((index_of(unit), start));
            }
            if at >= end - SLIVER {
                continue;
            }
            if let Some(i) = piece {
                // The VOD's moment `at` plays, in the piece's spans.
                let p = &pieces[i];
                let mut into = at - start;
                for (a, b) in p.spans() {
                    if into < b - a {
                        if self.split_piece(p.id, a + into)? {
                            return Ok((index_of(unit) + 1, at));
                        }
                        break;
                    }
                    into -= b - a;
                }
            }
            return Ok(if at - start < end - at { (index_of(unit), start) } else { (index_of(unit) + 1, end) });
        }
        Ok((top.len(), t))
    }

    /// How long `state`'s pieces play, end to end.
    fn played_of(&self, state: &EditState) -> Result<f64> {
        let mut total = 0.0;
        for i in &state.items {
            let (Some(vod), Some(start)) = (i.vod_id, i.start) else { continue };
            let end = match i.end {
                Some(end) => end.seconds(),
                None => self.vod_duration(vod)?.unwrap_or(0.0),
            };
            let struck: Vec<(f64, f64)> = i.struck.iter().map(|(a, b)| (a.seconds(), b.seconds())).collect();
            total += spans(start.seconds(), end, &struck).iter().map(|(a, b)| b - a).sum::<f64>();
        }
        Ok(total)
    }
}

/// The marks of a copied clip that go with it into a timeline as a group:
/// its chapters and highlights whose moments it plays, as `add_group` takes
/// a clip's (its cuts were where its own stretches were).
fn group_marks(state: &EditState) -> Vec<MarkAt> {
    let plays = |m: &MarkAt| {
        state.items.iter().any(|i| match (i.vod_id, i.start) {
            (Some(vod), Some(start)) if vod == m.vod_id => {
                let end = i.end.map_or(f64::INFINITY, |e| e.seconds());
                let struck: Vec<(f64, f64)> = i.struck.iter().map(|(a, b)| (a.seconds(), b.seconds())).collect();
                let t = m.at.seconds();
                spans(start.seconds(), end, &struck).iter().any(|&(a, b)| t >= a && t < b)
            }
            _ => false,
        })
    };
    state.marks.iter().filter(|m| m.kind != "cut" && plays(m)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edits::tests::info;
    use std::path::Path;

    fn library() -> (Db, i64) {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let vod = db.add_vod(&info("x")).unwrap().0.id;
        (db, vod)
    }

    /// An edit's pieces as (VOD, start, end, group title).
    fn shape(db: &Db, edit: i64) -> Vec<(i64, f64, f64, Option<String>)> {
        db.edit_pieces(edit)
            .unwrap()
            .into_iter()
            .map(|p| (p.vod_id, p.start, p.end, p.group.map(|g| g.title)))
            .collect()
    }

    #[test]
    fn a_copy_of_an_edits_stretch_is_what_it_was_when_copied() {
        let (db, vod) = library();
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (10.0, 20.0), "a", None).unwrap();
        db.add_piece(edit, vod, (30.0, 40.0), "b", None).unwrap();
        let copy = db.copy_of(&Put::Span { clip: edit, from: 5.0, to: 15.0 }).unwrap();
        assert_eq!(copy.kind, CopiedKind::Stretch);
        // The edit changes after the copy: its second piece goes.
        let second = db.edit_pieces(edit).unwrap()[1].id;
        db.remove_piece(second).unwrap();
        let other = db.add_edit("f").unwrap().id;
        let bar = db.paste(Paste::Bar(other), &[copy]).unwrap().clips;
        assert_eq!(bar.len(), 1);
        let pasted: Vec<(f64, f64)> = db.edit_pieces(bar[0].0).unwrap().iter().map(|p| (p.start, p.end)).collect();
        assert_eq!(pasted, [(15.0, 20.0), (30.0, 35.0)], "the pieces as they were copied");
    }

    #[test]
    fn a_pill_pasted_on_its_own_bar_is_a_second_clip_and_its_own() {
        let (db, vod) = library();
        db.add_mark(vod, "highlight", 12.0, Some("wow")).unwrap();
        let pill = db.add_clip(vod, 10.0, 20.0, "Hello", true).unwrap().id;
        let copy = db.copy_of(&Put::Clip { id: pill }).unwrap();
        assert_eq!(copy.kind, CopiedKind::Clip);
        let bar = db.vod_clip(vod).unwrap().unwrap();
        let made: Vec<i64> = db.paste(Paste::Bar(bar), &[copy]).unwrap().clips.iter().map(|c| c.0).collect();
        assert_eq!(made.len(), 1);
        assert_ne!(made[0], pill);
        let clip = db.clip(made[0]).unwrap().unwrap();
        assert_eq!((clip.start, clip.end, clip.title.as_str(), clip.shown), (10.0, 20.0, "Hello", true));
        assert_eq!(db.clips(vod).unwrap().len(), 2);
        // Its marks are its own: the pill's highlight came with it, and
        // deleting the pill leaves the copy as it is.
        let marks = |id| db.clip_marks(id).unwrap().into_iter().map(|(_, m)| m.text).collect::<Vec<_>>();
        assert_eq!(marks(made[0]), [Some("wow".to_string())]);
        db.delete_clip(pill).unwrap();
        assert_eq!(marks(made[0]), [Some("wow".to_string())]);
        assert_eq!(db.clip(made[0]).unwrap().unwrap().end, 20.0);
    }

    #[test]
    fn a_vods_bar_takes_only_its_own_footage() {
        let (db, vod) = library();
        let other = db.add_vod(&info("y")).unwrap().0.id;
        let theirs = db.copy_of(&Put::Range { vod_id: other, start: 1.0, end: 2.0, title: None }).unwrap();
        let ours = db.copy_of(&Put::Range { vod_id: vod, start: 1.0, end: 2.0, title: None }).unwrap();
        let bar = db.vod_clip(vod).unwrap().unwrap();
        let refused = db.paste(Paste::Bar(bar), std::slice::from_ref(&theirs)).unwrap_err();
        assert_eq!(refused.to_string(), "Clips of another VOD go in an edit.");
        assert!(db.clips(vod).unwrap().is_empty());
        let pasted = db.paste(Paste::Bar(bar), &[theirs.clone(), ours]).unwrap();
        assert_eq!((pasted.clips.len(), pasted.refused), (1, 1));
        // A clip of the VOD's bar open in the editor: the same.
        assert_eq!(pasted.clips[0].1, 1, "the second copy's");
        let pill = pasted.clips[0].0;
        assert!(
            db.paste(Paste::Timeline { clip: pill, at: 0.5, replace: None }, std::slice::from_ref(&theirs)).is_err()
        );
        // An edit takes any VOD's.
        let edit = db.add_edit("e").unwrap().id;
        db.paste(Paste::Bar(edit), &[theirs]).unwrap();
    }

    #[test]
    fn pieces_of_a_vod_gone_since_the_copy_are_left_out() {
        let (db, vod) = library();
        let other = db.add_vod(&info("y")).unwrap().0.id;
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (0.0, 10.0), "a", None).unwrap();
        db.add_piece(edit, other, (0.0, 10.0), "b", None).unwrap();
        let both = db.copy_of(&Put::Clip { id: edit }).unwrap();
        let theirs = db.copy_of(&Put::Range { vod_id: other, start: 1.0, end: 2.0, title: None }).unwrap();
        // The other VOD taken out of the library and used no more: erased.
        let other_edit_piece = db.edit_pieces(edit).unwrap()[1].id;
        db.remove_piece(other_edit_piece).unwrap();
        assert!(db.remove_entry(other).unwrap(), "nothing uses it");
        let target = db.add_edit("f").unwrap().id;
        let pasted = db.paste(Paste::Bar(target), &[both]).unwrap();
        assert_eq!(pasted.dropped, 1);
        assert_eq!(shape(&db, pasted.clips[0].0), [(vod, 0.0, 10.0, None)]);
        let before = db.edit_state(target).unwrap();
        let gone = db.paste(Paste::Timeline { clip: target, at: 0.0, replace: None }, &[theirs]).unwrap_err();
        assert_eq!(gone.to_string(), "The VOD it was copied from is gone.");
        assert_eq!(db.edit_state(target).unwrap(), before, "nothing changed");
    }

    #[test]
    fn pasted_in_a_timeline_at_the_playhead_splits_the_piece_there() {
        let (db, vod) = library();
        let pill = db.add_clip(vod, 1.0, 2.0, "Hello", true).unwrap().id;
        let edit = db.add_edit("e").unwrap().id;
        db.add_piece(edit, vod, (1.0, 2.0), "a", None).unwrap();
        let copy = db.copy_of(&Put::Clip { id: pill }).unwrap();
        let before = db.edit_state(edit).unwrap().unwrap();
        let pasted =
            db.paste(Paste::Timeline { clip: edit, at: 0.5, replace: None }, std::slice::from_ref(&copy)).unwrap();
        assert_eq!(pasted.span, Some((0.5, 1.5)));
        assert_eq!(
            shape(&db, edit),
            [(vod, 1.0, 1.5, None), (vod, 1.0, 2.0, Some("Hello".into())), (vod, 1.5, 2.0, None)]
        );
        // Undone (the state before put back), one piece again.
        db.restore_edit(edit, &before).unwrap();
        assert_eq!(shape(&db, edit), [(vod, 1.0, 2.0, None)]);

        // Inside a group: at its nearer edge, the group left whole.
        db.paste(Paste::Timeline { clip: edit, at: 1.0, replace: None }, std::slice::from_ref(&copy)).unwrap();
        let stretch = db.copy_of(&Put::Range { vod_id: vod, start: 5.0, end: 6.0, title: None }).unwrap();
        let pasted =
            db.paste(Paste::Timeline { clip: edit, at: 1.8, replace: None }, std::slice::from_ref(&stretch)).unwrap();
        assert_eq!(pasted.span, Some((2.0, 3.0)));
        assert_eq!(
            shape(&db, edit),
            [(vod, 1.0, 2.0, None), (vod, 1.0, 2.0, Some("Hello".into())), (vod, 5.0, 6.0, None)]
        );
        // A stretch selected is replaced by what's pasted.
        let pasted =
            db.paste(Paste::Timeline { clip: edit, at: 0.0, replace: Some((0.25, 0.75)) }, &[stretch]).unwrap();
        assert_eq!(pasted.span, Some((0.25, 1.25)));
        assert_eq!(
            shape(&db, edit),
            [
                (vod, 1.0, 1.25, None),
                (vod, 5.0, 6.0, None),
                (vod, 1.75, 2.0, None),
                (vod, 1.0, 2.0, Some("Hello".into())),
                (vod, 5.0, 6.0, None)
            ]
        );
        // A VOD's own timeline takes nothing.
        let own = db.vod_clip(vod).unwrap().unwrap();
        assert!(db.paste(Paste::Timeline { clip: own, at: 0.0, replace: None }, &[copy]).is_err());
    }

    #[test]
    fn a_pasted_pills_marks_are_its_own() {
        let (db, vod) = library();
        db.add_mark(vod, "chapter", 12.0, Some("in")).unwrap();
        let pill = db.add_clip(vod, 10.0, 20.0, "Hello", true).unwrap().id;
        db.add_clip_mark(pill, vod, "cut", 15.0, None).unwrap();
        let copy = db.copy_of(&Put::Clip { id: pill }).unwrap();
        // The VOD's marks change after the copy: the pasted one keeps the copy's.
        db.add_mark(vod, "highlight", 14.0, Some("later")).unwrap();
        let edit = db.add_edit("e").unwrap().id;
        db.paste(Paste::Timeline { clip: edit, at: 0.0, replace: None }, std::slice::from_ref(&copy)).unwrap();
        let kinds = |clip| -> Vec<(String, Option<String>)> {
            db.clip_marks(clip).unwrap().into_iter().map(|(_, m)| (m.kind, m.text)).collect()
        };
        assert_eq!(
            kinds(edit),
            [("chapter".to_string(), Some("in".to_string()))],
            "not its cut, not the new highlight"
        );
        // On a bar, the clip whole: its cut too.
        let bar = db.paste(Paste::Bar(edit), &[copy]).unwrap().clips[0].0;
        assert_eq!(kinds(bar), [("chapter".to_string(), Some("in".to_string())), ("cut".to_string(), None)]);
    }
}
