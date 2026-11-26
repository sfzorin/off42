//! Object entrance/exit animations: model, click-step timeline, playback state and PPTX `p:timing`.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const MAX_ANIMATIONS: usize = 1000;
pub const MAX_MS: u32 = 60_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    Appear,
    Fade,
    FlyIn,
    Zoom,
    Disappear,
    FadeOut,
    FlyOut,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Start {
    #[default]
    OnClick,
    WithPrevious,
    AfterPrevious,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Animation {
    pub object: u64,
    pub effect: Effect,
    #[serde(default)]
    pub start: Start,
    pub duration_ms: u32,
    #[serde(default)]
    pub delay_ms: u32,
}

pub const EFFECTS: [(&str, Effect); 7] = [
    ("Appear", Effect::Appear),
    ("Fade", Effect::Fade),
    ("Fly in", Effect::FlyIn),
    ("Zoom", Effect::Zoom),
    ("Disappear", Effect::Disappear),
    ("Fade out", Effect::FadeOut),
    ("Fly out", Effect::FlyOut),
];
pub const STARTS: [(&str, Start); 3] = [
    ("On click", Start::OnClick),
    ("With previous", Start::WithPrevious),
    ("After previous", Start::AfterPrevious),
];

impl Effect {
    pub fn label(self) -> &'static str {
        EFFECTS.iter().find(|e| e.1 == self).unwrap().0
    }
    pub fn from_label(label: &str) -> Option<Self> {
        EFFECTS.iter().find(|e| e.0.eq_ignore_ascii_case(label.trim())).map(|e| e.1)
    }
    pub fn entrance(self) -> bool {
        matches!(self, Effect::Appear | Effect::Fade | Effect::FlyIn | Effect::Zoom)
    }
    fn instant(self) -> bool {
        matches!(self, Effect::Appear | Effect::Disappear)
    }
}
impl Start {
    pub fn label(self) -> &'static str {
        STARTS.iter().find(|e| e.1 == self).unwrap().0
    }
    pub fn from_label(label: &str) -> Option<Self> {
        STARTS.iter().find(|e| e.0.eq_ignore_ascii_case(label.trim())).map(|e| e.1)
    }
}
impl Animation {
    pub fn valid(&self) -> bool {
        self.duration_ms <= MAX_MS && self.delay_ms <= MAX_MS
    }
    fn length(&self) -> u32 {
        if self.effect.instant() { 0 } else { self.duration_ms }
    }
}

/// One scheduled animation: index into the slide list, click group and begin time within it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scheduled {
    pub index: usize,
    pub group: usize,
    pub begin_ms: u32,
}
/// Group 0 runs when the slide appears; each "On click" animation starts a new group.
pub fn schedule(animations: &[Animation]) -> Vec<Scheduled> {
    let mut out = Vec::with_capacity(animations.len());
    let mut group = 0;
    let (mut previous_begin, mut group_end) = (0u32, 0u32);
    for (index, a) in animations.iter().enumerate() {
        let begin = match a.start {
            Start::OnClick => {
                group += 1;
                group_end = 0;
                a.delay_ms
            }
            Start::WithPrevious => previous_begin + a.delay_ms,
            Start::AfterPrevious => group_end + a.delay_ms,
        };
        previous_begin = begin;
        group_end = group_end.max(begin + a.length());
        out.push(Scheduled { index, group, begin_ms: begin });
    }
    out
}
/// Number of clicks the slide's animations consume before the next slide.
pub fn clicks(animations: &[Animation]) -> usize {
    schedule(animations).last().map_or(0, |s| s.group)
}
/// Milliseconds until every animation of `group` has finished.
pub fn group_length(animations: &[Animation], group: usize) -> u32 {
    schedule(animations)
        .into_iter()
        .filter(|s| s.group == group)
        .map(|s| s.begin_ms + animations[s.index].length())
        .max()
        .unwrap_or(0)
}

/// How an object is drawn at a point of the timeline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Visual {
    pub opacity: f64,
    /// Vertical offset in slide points.
    pub dy: f64,
    /// Scale about the object centre.
    pub scale: f64,
}
impl Visual {
    pub const SHOWN: Self = Self { opacity: 1.0, dy: 0.0, scale: 1.0 };
    pub const HIDDEN: Self = Self { opacity: 0.0, dy: 0.0, scale: 1.0 };
    pub fn hidden(&self) -> bool {
        self.opacity <= 0.0 || self.scale <= 0.0
    }
}

/// Object states once clicks `0..=step` have started and `elapsed_ms` passed in group `step`.
/// `travel` gives the vertical distance a fly in/out covers for an object.
pub fn visuals(
    animations: &[Animation],
    step: usize,
    elapsed_ms: u64,
    travel: impl Fn(u64) -> f64,
) -> HashMap<u64, Visual> {
    let mut out: HashMap<u64, Visual> = HashMap::new();
    for item in schedule(animations) {
        let a = &animations[item.index];
        let state = out.entry(a.object).or_insert(if a.effect.entrance() { Visual::HIDDEN } else { Visual::SHOWN });
        if item.group > step {
            continue;
        }
        let progress = if item.group < step {
            1.0
        } else {
            let since = elapsed_ms as f64 - item.begin_ms as f64;
            if since < 0.0 {
                continue;
            }
            if a.length() == 0 { 1.0 } else { (since / a.length() as f64).clamp(0.0, 1.0) }
        };
        let distance = travel(a.object);
        *state = match a.effect {
            Effect::Appear => Visual::SHOWN,
            Effect::Fade => Visual { opacity: progress, ..Visual::SHOWN },
            Effect::FlyIn => Visual { dy: (1.0 - progress) * distance, ..Visual::SHOWN },
            Effect::Zoom => Visual { scale: progress, ..Visual::SHOWN },
            Effect::Disappear => Visual::HIDDEN,
            Effect::FadeOut => Visual { opacity: 1.0 - progress, ..Visual::SHOWN },
            Effect::FlyOut => {
                if progress >= 1.0 { Visual::HIDDEN } else { Visual { dy: progress * distance, ..Visual::SHOWN } }
            }
        };
    }
    out
}

fn preset(effect: Effect) -> (u32, &'static str, u32) {
    match effect {
        Effect::Appear => (1, "entr", 0),
        Effect::Fade => (10, "entr", 0),
        Effect::FlyIn => (2, "entr", 4),
        Effect::Zoom => (53, "entr", 16),
        Effect::Disappear => (1, "exit", 0),
        Effect::FadeOut => (10, "exit", 0),
        Effect::FlyOut => (2, "exit", 4),
    }
}

/// A `p:timing` element for the animations; `spid` maps object ids to shape ids.
pub fn pptx_timing(animations: &[Animation], spid: impl Fn(u64) -> Option<u64>) -> Result<String, String> {
    if animations.is_empty() {
        return Ok(String::new());
    }
    let mut id = 2u32;
    let mut next = || {
        id += 1;
        id
    };
    let schedule = schedule(animations);
    let mut groups = String::new();
    for group in 0..=schedule.last().map_or(0, |s| s.group) {
        let items = schedule.iter().filter(|s| s.group == group).collect::<Vec<_>>();
        if items.is_empty() {
            continue;
        }
        let condition = if group == 0 {
            "<p:cond delay=\"indefinite\"/><p:cond evt=\"onBegin\" delay=\"0\"><p:tn val=\"2\"/></p:cond>"
        } else {
            "<p:cond delay=\"indefinite\"/>"
        };
        let outer = next();
        let mut effects = String::new();
        for item in items {
            let a = &animations[item.index];
            let shape = spid(a.object).ok_or("Animated object is missing")?;
            let target = format!("<p:tgtEl><p:spTgt spid=\"{shape}\"/></p:tgtEl>");
            let (preset_id, class, subtype) = preset(a.effect);
            let node = match (a.start, group) {
                (Start::OnClick, g) if g > 0 => "clickEffect",
                (Start::AfterPrevious, _) => "afterEffect",
                _ => "withEffect",
            };
            let duration = a.duration_ms.max(1);
            let visibility = |value: &str, delay: u32, id: u32| {
                format!("<p:set><p:cBhvr><p:cTn id=\"{id}\" dur=\"1\" fill=\"hold\"><p:stCondLst><p:cond delay=\"{delay}\"/></p:stCondLst></p:cTn>{target}<p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val=\"{value}\"/></p:to></p:set>")
            };
            let anim = |attribute: &str, from: &str, to: &str, id: u32| {
                format!("<p:anim calcmode=\"lin\" valueType=\"num\"><p:cBhvr additive=\"base\"><p:cTn id=\"{id}\" dur=\"{duration}\" fill=\"hold\"/>{target}<p:attrNameLst><p:attrName>{attribute}</p:attrName></p:attrNameLst></p:cBhvr><p:tavLst><p:tav tm=\"0\"><p:val><p:strVal val=\"{from}\"/></p:val></p:tav><p:tav tm=\"100000\"><p:val><p:strVal val=\"{to}\"/></p:val></p:tav></p:tavLst></p:anim>")
            };
            let fade = |transition: &str, id: u32| {
                format!("<p:animEffect transition=\"{transition}\" filter=\"fade\"><p:cBhvr><p:cTn id=\"{id}\" dur=\"{duration}\"/>{target}</p:cBhvr></p:animEffect>")
            };
            let effect_id = next();
            let behaviors = match a.effect {
                Effect::Appear => visibility("visible", 0, next()),
                Effect::Disappear => visibility("hidden", 0, next()),
                Effect::Fade => visibility("visible", 0, next()) + &fade("in", next()),
                Effect::FadeOut => fade("out", next()) + &visibility("hidden", duration - 1, next()),
                Effect::FlyIn => visibility("visible", 0, next()) + &anim("ppt_x", "#ppt_x", "#ppt_x", next()) + &anim("ppt_y", "1+#ppt_h/2", "#ppt_y", next()),
                Effect::FlyOut => anim("ppt_x", "ppt_x", "ppt_x", next()) + &anim("ppt_y", "ppt_y", "1+ppt_h/2", next()) + &visibility("hidden", duration - 1, next()),
                Effect::Zoom => visibility("visible", 0, next()) + &anim("ppt_w", "0", "#ppt_w", next()) + &anim("ppt_h", "0", "#ppt_h", next()),
            };
            let delay = item.begin_ms;
            effects.push_str(&format!("<p:par><p:cTn id=\"{effect_id}\" presetID=\"{preset_id}\" presetClass=\"{class}\" presetSubtype=\"{subtype}\" fill=\"hold\" nodeType=\"{node}\"><p:stCondLst><p:cond delay=\"{delay}\"/></p:stCondLst><p:childTnLst>{behaviors}</p:childTnLst></p:cTn></p:par>"));
        }
        let inner = next();
        groups.push_str(&format!("<p:par><p:cTn id=\"{outer}\" fill=\"hold\"><p:stCondLst>{condition}</p:stCondLst><p:childTnLst><p:par><p:cTn id=\"{inner}\" fill=\"hold\"><p:stCondLst><p:cond delay=\"0\"/></p:stCondLst><p:childTnLst>{effects}</p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>"));
    }
    Ok(format!("<p:timing xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:tnLst><p:par><p:cTn id=\"1\" dur=\"indefinite\" restart=\"never\" nodeType=\"tmRoot\"><p:childTnLst><p:seq concurrent=\"1\" nextAc=\"seek\"><p:cTn id=\"2\" dur=\"indefinite\" nodeType=\"mainSeq\"><p:childTnLst>{groups}</p:childTnLst></p:cTn><p:prevCondLst><p:cond evt=\"onPrev\" delay=\"0\"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:prevCondLst><p:nextCondLst><p:cond evt=\"onNext\" delay=\"0\"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:nextCondLst></p:seq></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>"))
}

/// Reads the main sequence of a slide's `p:timing`. Returns the supported animations (shape ids in
/// `object`) and whether they describe the whole timeline, so it can be rewritten without loss.
pub fn read_pptx_timing(timing: roxmltree::Node<'_, '_>) -> (Vec<Animation>, bool) {
    let mut out = Vec::new();
    let mut complete = true;
    for node in timing.descendants().filter(|n| n.is_element() && n.tag_name().name() == "cTn") {
        let Some(class) = node.attribute("presetClass") else { continue };
        let preset_id = node.attribute("presetID").and_then(|v| v.parse::<u32>().ok());
        let effect = match (class, preset_id) {
            ("entr", Some(1)) => Effect::Appear,
            ("entr", Some(10)) => Effect::Fade,
            ("entr", Some(2)) => Effect::FlyIn,
            ("entr", Some(53)) => Effect::Zoom,
            ("exit", Some(1)) => Effect::Disappear,
            ("exit", Some(10)) => Effect::FadeOut,
            ("exit", Some(2)) => Effect::FlyOut,
            _ => {
                complete = false;
                continue;
            }
        };
        let targets = node
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "spTgt")
            .map(|n| (n.attribute("spid").and_then(|v| v.parse::<u64>().ok()), n.children().any(|c| c.is_element())))
            .collect::<Vec<_>>();
        let Some((Some(shape), _)) = targets.first().copied() else {
            complete = false;
            continue;
        };
        if targets.iter().any(|t| t.0 != Some(shape) || t.1) {
            // Paragraph-level builds and multiple targets are outside this model.
            complete = false;
        }
        let start = match node.attribute("nodeType") {
            Some("clickEffect") => Start::OnClick,
            Some("afterEffect") => Start::AfterPrevious,
            _ => Start::WithPrevious,
        };
        // Absolute begin within the click group: the effect delay plus enclosing time offsets.
        let mut delay = 0u32;
        for ctn in std::iter::once(node).chain(node.ancestors().filter(|n| n.is_element() && n.tag_name().name() == "cTn" && *n != node)) {
            match start_delay(ctn) {
                Some(Ok(v)) => delay = delay.saturating_add(v),
                _ => break,
            }
        }
        let delay = delay.min(MAX_MS);
        let duration = node
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "cTn" && *n != node)
            .filter_map(|n| n.attribute("dur").and_then(|v| v.parse::<u32>().ok()))
            .max()
            .unwrap_or(500)
            .min(MAX_MS);
        let effect_duration = if effect.instant() { 0 } else { duration };
        out.push(Animation { object: shape, effect, start, duration_ms: effect_duration, delay_ms: delay });
        if out.len() >= MAX_ANIMATIONS {
            complete = false;
            break;
        }
    }
    // Begin times are absolute in the file; this model chains them relative to the previous effect.
    let (mut previous_begin, mut group_end) = (0u32, 0u32);
    for a in out.iter_mut() {
        let begin = a.delay_ms;
        a.delay_ms = match a.start {
            Start::OnClick => {
                group_end = 0;
                begin
            }
            Start::WithPrevious => begin.saturating_sub(previous_begin),
            Start::AfterPrevious => begin.saturating_sub(group_end),
        };
        previous_begin = begin;
        group_end = group_end.max(begin + a.length());
    }
    if timing.descendants().any(|n| n.is_element() && matches!(n.tag_name().name(), "bldLst" | "audio" | "video" | "cmd" | "animMotion" | "animRot" | "animScale" | "animClr")) {
        complete = false;
    }
    (out, complete)
}

/// The first start condition delay of a `p:cTn`: `None` without one, `Err` for "indefinite".
fn start_delay(ctn: roxmltree::Node<'_, '_>) -> Option<Result<u32, ()>> {
    let value = ctn
        .children()
        .find(|c| c.is_element() && c.tag_name().name() == "stCondLst")?
        .children()
        .find(|c| c.is_element() && c.tag_name().name() == "cond")?
        .attribute("delay")?;
    Some(value.parse::<u32>().map_err(|_| ()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(object: u64, effect: Effect, start: Start, duration_ms: u32, delay_ms: u32) -> Animation {
        Animation { object, effect, start, duration_ms, delay_ms }
    }

    #[test]
    fn schedule_groups_clicks_and_chains() {
        let list = [
            a(1, Effect::Fade, Start::WithPrevious, 500, 0),
            a(2, Effect::FlyIn, Start::OnClick, 400, 100),
            a(3, Effect::Zoom, Start::WithPrevious, 300, 50),
            a(4, Effect::Appear, Start::AfterPrevious, 0, 200),
            a(2, Effect::FadeOut, Start::OnClick, 600, 0),
        ];
        let s = schedule(&list);
        assert_eq!(s.iter().map(|s| (s.group, s.begin_ms)).collect::<Vec<_>>(), vec![(0, 0), (1, 100), (1, 150), (1, 700), (2, 0)]);
        assert_eq!(clicks(&list), 2);
        assert_eq!(group_length(&list, 1), 700);
        let at = |step, ms| visuals(&list, step, ms, |_| 100.0);
        assert_eq!(at(0, 0)[&1].opacity, 0.0);
        assert_eq!(at(0, 250)[&1].opacity, 0.5);
        assert!(at(0, 10_000)[&2].hidden() && at(0, 10_000)[&4].hidden());
        assert_eq!(at(1, 300)[&2].dy, 50.0);
        assert_eq!(at(1, 300)[&3].scale, 0.5);
        assert!(at(1, 699)[&4].hidden() && !at(1, 700)[&4].hidden());
        assert_eq!(at(2, 300)[&2].opacity, 0.5);
        assert!(at(2, 600)[&2].hidden() && !at(2, 600)[&3].hidden());
    }

    #[test]
    fn pptx_timing_roundtrips_every_effect() {
        let list = EFFECTS
            .iter()
            .enumerate()
            .map(|(i, (_, effect))| a(i as u64 + 1, *effect, STARTS[i % 3].1, if effect.instant() { 0 } else { 250 + i as u32 * 100 }, i as u32 * 10))
            .collect::<Vec<_>>();
        let xml = pptx_timing(&list, |id| Some(id + 10)).unwrap();
        let doc = roxmltree::Document::parse(&xml).unwrap();
        let (read, complete) = read_pptx_timing(doc.root_element());
        assert!(complete);
        let expected = list.iter().map(|x| Animation { object: x.object + 10, ..x.clone() }).collect::<Vec<_>>();
        assert_eq!(read, expected);
        assert!(pptx_timing(&list, |_| None).is_err());
        let emphasis = xml.replacen("presetClass=\"entr\"", "presetClass=\"emph\"", 1);
        let doc = roxmltree::Document::parse(&emphasis).unwrap();
        let (read, complete) = read_pptx_timing(doc.root_element());
        assert!(!complete && read.len() == list.len() - 1);
    }

    fn entry(bytes: &[u8], path: &str) -> String {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut out = String::new();
        std::io::Read::read_to_string(&mut archive.by_name(path).unwrap(), &mut out).unwrap();
        out
    }

    #[test]
    fn slide_animations_edit_undo_delete_and_office_roundtrip() {
        let mut deck = crate::Presentation::new();
        let first = deck.insert_object(crate::ObjectKind::Rectangle, "A".into()).unwrap();
        let second = deck.insert_object(crate::ObjectKind::Ellipse, String::new()).unwrap();
        let third = deck.insert_object(crate::ObjectKind::Text, "C".into()).unwrap();
        deck.set_animation(second, Some(a(0, Effect::FlyIn, Start::OnClick, 700, 0))).unwrap();
        deck.set_animation(first, Some(a(0, Effect::Fade, Start::WithPrevious, 400, 100))).unwrap();
        deck.set_animation(third, Some(a(0, Effect::Zoom, Start::AfterPrevious, 300, 0))).unwrap();
        deck.move_animation(first, true).unwrap();
        let expected = vec![
            a(first, Effect::Fade, Start::WithPrevious, 400, 100),
            a(second, Effect::FlyIn, Start::OnClick, 700, 0),
            a(third, Effect::Zoom, Start::AfterPrevious, 300, 0),
        ];
        assert_eq!(deck.slides()[0].animations, expected);
        assert!(deck.undo());
        assert_eq!(deck.slides()[0].animations[0].object, second);
        assert!(deck.redo());
        assert!(deck.set_animation(99, Some(a(0, Effect::Appear, Start::OnClick, 0, 0))).is_err());

        let pptx = crate::pptx::export_new(&deck).unwrap();
        assert!(entry(&pptx, "ppt/slides/slide1.xml").contains("<p:timing"));
        let (mut opened, source) = crate::pptx::import(&pptx).unwrap();
        assert_eq!(opened.slides()[0].animations, expected);
        assert!(source.can_edit_animations(opened.slides()[0].id));
        let odp = crate::odp::export(&deck).unwrap();
        let (odp_deck, odp_source) = crate::odp::import_with_source(&odp).unwrap();
        assert_eq!(odp_deck.slides()[0].animations, expected);
        assert!(!odp_source.can_edit_animations(odp_deck.slides()[0].id));

        // Overlay: unchanged timelines stay byte-identical; edits rewrite p:timing, including new objects.
        let same = crate::pptx::export(&opened, &source).unwrap();
        assert_eq!(entry(&same, "ppt/slides/slide1.xml"), entry(&pptx, "ppt/slides/slide1.xml"));
        let added = opened.insert_object(crate::ObjectKind::Rectangle, String::new()).unwrap();
        opened.set_animation(added, Some(a(0, Effect::FadeOut, Start::OnClick, 500, 0))).unwrap();
        let removed = opened.slides()[0].animations[1].object;
        opened.set_animation(removed, None).unwrap();
        let updated = crate::pptx::export(&opened, &source).unwrap();
        let reopened = crate::pptx::import(&updated).unwrap().0;
        let list = &reopened.slides()[0].animations;
        assert_eq!(list.len(), 3);
        assert_eq!(list[2].effect, Effect::FadeOut);
        assert!(reopened.slides()[0].objects.iter().any(|o| o.id == list[2].object && o.kind == crate::ObjectKind::Rectangle && o.text.is_empty()));
        assert_eq!(entry(&updated, "ppt/slides/slide1.xml").matches("<p:timing").count(), 1);
        // Deleting an object removes its animations.
        deck.delete_objects(&[first]).unwrap();
        assert_eq!(deck.slides()[0].animations.len(), 2);
    }
}

