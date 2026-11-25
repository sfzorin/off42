use super::*;

fn spec(id: &str, values: &[(&str, &str)]) -> SealSpec {
    let mut s = SealSpec::example(named(id).unwrap());
    s.values = values.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    s
}

fn inked(img: &image::RgbaImage) -> usize {
    img.pixels().filter(|p| p.0[3] > 40).count()
}

#[test]
fn catalogue_matches_macos() {
    let all = templates();
    assert_eq!(all.len(), 132);
    assert_eq!(all[0].id, "ru.ooo");
    assert_eq!(all[131].id, "personal.seal.cjk");
    let mut ids: Vec<_> = all.iter().map(|t| t.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), all.len(), "ids are unique");
    for t in all {
        assert!(!t.fields.is_empty(), "{}", t.id);
        assert!(!t.title().is_empty() && !t.titles[1].is_empty() && !t.titles[2].is_empty());
    }
    let f = fields("ru.ooo").unwrap();
    assert_eq!(f.iter().map(|f| f.key).collect::<Vec<_>>(), ["edge", "inner", "centre"]);
    assert!(f[2].multiline);
    assert_eq!(fields("office.copy").unwrap().iter().map(|f| f.key).collect::<Vec<_>>(), ["top", "word", "date"]);
    assert!(fields("nope").is_none());
    assert_eq!(SealCategory::ALL.len(), 15);
    for c in SealCategory::ALL {
        assert!(all.iter().any(|t| t.category == c), "{c:?} has templates");
    }
}

#[test]
fn every_template_renders_non_blank_and_bounded() {
    for (i, t) in templates().iter().enumerate() {
        let s = SealSpec::example(t);
        let img = render(&s, (i as u64 + 1) * 104_729, 0.25).unwrap_or_else(|| panic!("{} renders", t.id));
        let (w, h) = (img.width(), img.height());
        // Canvas side is 1.25 × the larger layout side (at most 1300 units); trimming leaves only the stamp.
        assert!(w > 60 && h > 40 && w <= 407 && h <= 407, "{}: {w}×{h}", t.id);
        let ink = inked(&img);
        assert!(ink > 800, "{}: only {ink} inked pixels", t.id);
        assert!(ink < (w * h) as usize, "{}", t.id);
    }
}

#[test]
fn same_seed_same_impression_and_seeds_differ() {
    let s = spec("ru.ooo", &[]);
    let a = render(&s, 7, 0.5).unwrap();
    let b = render(&s, 7, 0.5).unwrap();
    assert_eq!(a.as_raw(), b.as_raw());
    let c = render(&s, 8, 0.5).unwrap();
    assert!(a.dimensions() != c.dimensions() || a.as_raw() != c.as_raw());
    // The design seed never changes the layout, only impressions do.
    let mut d = s.clone();
    d.design_seed = 9;
    assert_eq!(render(&d, 7, 0.5).unwrap().as_raw(), a.as_raw());
}

#[test]
fn pixel_scale_keeps_the_impression() {
    let s = spec("de.firm", &[]);
    let small = render(&s, 3, 0.5).unwrap();
    let large = render(&s, 3, 1.0).unwrap();
    let ratio = large.width() as f64 / small.width() as f64;
    assert!((ratio - 2.0).abs() < 0.08, "{ratio}");
}

#[test]
fn unknown_template_is_none() {
    let s = SealSpec {
        template_id: "missing".into(),
        values: HashMap::new(),
        ink: crate::InkColor::Blue.stamp_pad(),
        design_seed: 0,
    };
    assert!(render(&s, 1, 1.0).is_none());
}

#[test]
fn long_text_is_fitted_inside_the_stamp() {
    let long = "VERY LONG COMPANY NAME THAT WOULD NEVER FIT ON ONE LINE OF A RUBBER STAMP ".repeat(4);
    // Rectangular stamps keep their 1300-unit width: rows shrink to fit (down to 6 pt), so the row gets lower.
    let short = render(&spec("de.firm.plain", &[]), 5, 0.5).unwrap();
    let wide = render(&spec("de.firm.plain", &[("name", &long)]), 5, 0.5).unwrap();
    assert!(
        wide.width() <= 650 && wide.height() <= short.height() + 4,
        "{:?} vs {:?}",
        wide.dimensions(),
        short.dimensions()
    );
    let row = text::fit("HelveticaNeue-Bold", &long, 1220.0, 92.0);
    assert!(row.size < 30.0 && row.width(&long) <= 1220.5);
    // Round seals squeeze the ring text instead of running past the start.
    let round = render(&spec("ru.simple", &[("edge", &long)]), 5, 0.5).unwrap();
    assert!(round.width() <= 640 && round.height() <= 640);
    // Word stamps shrink the rows to the frame.
    let word = render(&spec("office.paid", &[("word", &long), ("date", "PAID IN FULL WITH THANKS")]), 5, 0.5).unwrap();
    let plain = render(&spec("office.paid", &[]), 5, 0.5).unwrap();
    assert!(word.width() <= plain.width() + 10);
}

#[test]
fn fields_use_examples_dates_and_user_text() {
    let s = spec("ru.ooo", &[("edge", "  ООО «ТЕСТ»  ")]);
    assert_eq!(s.text("edge"), "ООО «ТЕСТ»");
    assert_eq!(s.text("centre"), "ДЛЯ\nДОКУМЕНТОВ");
    assert_eq!(s.text("unknown"), "");
    // An empty date field means today; a single space (or a " " example) means no date.
    let today = today();
    assert_eq!(today.len(), 10);
    assert_eq!(spec("us.received", &[]).text("date"), today);
    assert_eq!(spec("us.received", &[("date", " ")]).text("date"), "");
    assert_eq!(spec("office.copy", &[]).text("date"), "");
    assert_eq!(spec("office.paid", &[]).text("date"), today);
    assert_eq!(spec("date.round", &[]).text("centre"), today);
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(civil_from_days(20_417), (2025, 11, 25));
    assert_eq!(civil_from_days(11_016), (2000, 2, 29));
}

#[test]
fn cyrillic_latin_and_cjk_text_draws_ink() {
    // Cyrillic and Latin always draw (system font or the bundled Noto Sans).
    let empty =
        render(&spec("personal.name", &[("name", " "), ("position", " ")]), 1, 0.5).map(|i| inked(&i)).unwrap_or(0);
    for name in ["Иван Петров", "Ivan Petrov", "Ιωάννης"] {
        let img = render(&spec("personal.name", &[("name", name), ("position", " ")]), 1, 0.5).unwrap();
        assert!(inked(&img) > empty + 1_000, "{name}");
    }
    // CJK: drawn when the host has a CJK font (macOS, Windows); a grid seal without characters is just its frame.
    let frame_only = inked(&render(&spec("jp.hanko", &[("chars", " ")]), 1, 0.5).unwrap());
    let hanko = inked(&render(&spec("jp.hanko", &[]), 1, 0.5).unwrap());
    let has_cjk = text::font("HiraMinProN-W6", 100.0).covers("山田");
    if has_cjk {
        assert!(hanko > frame_only + 5_000, "{hanko} vs {frame_only}");
    }
    // Latin in a grid seal reads in rows.
    let anna = inked(&render(&spec("personal.seal.cjk", &[]), 1, 0.5).unwrap());
    let anna_frame = inked(&render(&spec("personal.seal.cjk", &[("chars", " ")]), 1, 0.5).unwrap());
    assert!(anna > anna_frame + 5_000);
}

#[test]
fn impress_presses_a_picture() {
    let mut pic = image::RgbaImage::new(200, 120);
    for (x, y, p) in pic.enumerate_pixels_mut() {
        if !(20..180).contains(&x) || !(20..100).contains(&y) || (40..160).contains(&x) && (40..80).contains(&y) {
            continue;
        }
        *p = image::Rgba([40, 80, 180, 255]);
    }
    let a = impress(&pic, false, 11, 1.0).unwrap();
    let b = impress(&pic, false, 11, 1.0).unwrap();
    assert_eq!(a.as_raw(), b.as_raw());
    assert!(a.width() >= 160 && a.width() < 260 && inked(&a) > 4_000);
    let big = impress(&pic, true, 11, 2.0).unwrap();
    assert!(big.width() > a.width() * 3 / 2);
}
