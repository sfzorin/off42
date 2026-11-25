use super::*;
use crate::InkColor;

const NAMES: [&str; 4] = ["Sergey Zorin", "Иван Петров", "Ayşe Yılmaz", "John A. Smith"];

fn blue() -> Ink {
    InkColor::Blue.pen()
}

fn inked(image: &image::RgbaImage) -> usize {
    image.pixels().filter(|p| p.0[3] > 40).count()
}

#[test]
fn swift_double_matches_stdlib_algorithm() {
    // Lemire draw below 2^53 + 1 is the top 64 bits of next() * (2^53 + 1).
    let mut a = SeededRandom::new(7);
    let mut b = SeededRandom::new(7);
    let x = kinematics::swift_double(&mut a, 0.2, 0.3);
    let raw = b.next_u64();
    let expected = ((raw as u128 * ((1u128 << 53) + 1)) >> 64) as f64 * (f64::EPSILON / 2.0) * (0.3 - 0.2) + 0.2;
    assert!((x - expected).abs() < 1e-15 && (0.2..=0.3).contains(&x));
}

#[test]
fn design_matches_kit42_style_index() {
    // design(form:variant:seed:) of the sample sheet: Sergey Zorin, full name.
    let d = design(SignatureForm::FullName, 0, 31);
    assert_eq!(d >> 16, 31);
    // variant 0: archetype .full (0), flourish 3, join 0, capital 0 → ((0*11+3)*4+0)*3+0 = 36.
    assert_eq!(d & 0xFFFF, 36);
    assert_eq!(style_number(d), 37);
    assert_eq!(STYLE_COUNT, 1320);
    let s = style(design(SignatureForm::InitialsScribble, 1, 5));
    assert_eq!(s.archetype, Archetype::Monogram);
}

#[test]
fn renders_latin_cyrillic_and_turkish_names_in_every_form() {
    for (n, name) in NAMES.iter().enumerate() {
        for (f, form) in SignatureForm::ALL.iter().skip(1).enumerate() {
            let d = design(*form, n * 3 + f, (31 + n * 7 + f) as u64);
            let image = render(name, d, 7919, blue(), Pen::Ballpoint, 130.0).unwrap_or_else(|| panic!("{name} {form:?}"));
            let (w, h) = image.dimensions();
            assert!(w > 60 && h > 40 && w < 2000 && h < 900, "{name} {form:?}: {w}x{h}");
            let ink = inked(&image);
            assert!(ink > 300 && ink < (w * h) as usize / 2, "{name} {form:?}: {ink} inked of {w}x{h}");
            // Trimmed: ink reaches near every edge (6 px pad).
            let rows_with_ink = |y: u32| (0..w).any(|x| image.get_pixel(x, y).0[3] > 8);
            assert!((0..10).any(rows_with_ink) && (h - 10..h).any(rows_with_ink), "{name} {form:?} not trimmed");
        }
    }
}

#[test]
fn same_seeds_give_identical_pixels_and_impressions_differ() {
    let d = design(SignatureForm::InitialSurname, 1, 501);
    let a = render("Sergey Zorin", d, 5, blue(), Pen::Ballpoint, 130.0).unwrap();
    let b = render("Sergey Zorin", d, 5, blue(), Pen::Ballpoint, 130.0).unwrap();
    assert_eq!(a.dimensions(), b.dimensions());
    assert_eq!(a.as_raw(), b.as_raw());
    let c = render("Sergey Zorin", d, 6, blue(), Pen::Ballpoint, 130.0).unwrap();
    assert!(a.dimensions() != c.dimensions() || a.as_raw() != c.as_raw());
    // Another design is another signature.
    let e = render("Sergey Zorin", design(SignatureForm::InitialSurname, 2, 501), 5, blue(), Pen::Ballpoint, 130.0).unwrap();
    assert!(a.dimensions() != e.dimensions() || a.as_raw() != e.as_raw());
}

#[test]
fn every_pen_and_high_resolution_render() {
    let d = design(SignatureForm::Surname, 2, 77);
    let mut sizes = Vec::new();
    for pen in Pen::ALL {
        let image = render("John A. Smith", d, 11, blue(), pen, 130.0).unwrap_or_else(|| panic!("{pen:?}"));
        assert!(inked(&image) > 300, "{pen:?}");
        sizes.push(image.dimensions());
    }
    let hi = render("John A. Smith", d, 11, blue(), Pen::Ballpoint, 130.0 * 4.0).unwrap();
    let lo = render("John A. Smith", d, 11, blue(), Pen::Ballpoint, 130.0).unwrap();
    let ratio = hi.width() as f64 / lo.width() as f64;
    assert!((3.5..4.5).contains(&ratio), "{ratio}");
}

#[test]
fn invalid_input_is_rejected() {
    let d = design(SignatureForm::FullName, 0, 31);
    for name in ["", "   ", ". .", "\n\t"] {
        assert!(render(name, d, 1, blue(), Pen::Ballpoint, 130.0).is_none(), "{name:?}");
    }
    for height in [0.0, -5.0, f64::NAN, f64::INFINITY, MAXIMUM_PIXEL_HEIGHT + 1.0] {
        assert!(render("Sergey Zorin", d, 1, blue(), Pen::Ballpoint, height).is_none(), "{height}");
    }
    assert!(render("Sergey Zorin", d, 1, blue(), Pen::Ballpoint, MAXIMUM_PIXEL_HEIGHT).is_some());
    // Bad geometry reaching the shared drawing routine.
    let hand = Hand::default();
    let mut copy = SeededRandom::new(1);
    let o = DrawOptions::default();
    let nan = vec![vec![pt(0.0, 0.0), pt(f64::NAN, 1.0)]];
    assert!(draw(&nan, &hand, 0.3, 0.03, Pen::Ballpoint, blue(), 130.0, &mut copy, &o).is_none());
    let line = vec![vec![pt(0.0, 0.0), pt(1.0, 1.0), pt(2.0, 0.0)]];
    assert!(draw(&line, &hand, 0.3, 0.0, Pen::Ballpoint, blue(), 130.0, &mut copy, &o).is_none());
    assert!(draw(&line, &hand, f64::NAN, 0.03, Pen::Ballpoint, blue(), 130.0, &mut copy, &o).is_none());
    let too_many = vec![vec![pt(0.0, 0.0), pt(1.0, 0.0)]; 1025];
    assert!(draw(&too_many, &hand, 0.3, 0.03, Pen::Ballpoint, blue(), 130.0, &mut copy, &o).is_none());
    let too_long = vec![(0..MAXIMUM_SAMPLE_POINTS + 1).map(|i| pt(i as f64 * 1e-3, 0.0)).collect::<Vec<_>>()];
    assert!(draw(&too_long, &hand, 0.3, 0.03, Pen::Ballpoint, blue(), 130.0, &mut copy, &o).is_none());
    // A line far too long for the work limit.
    let huge = vec![vec![pt(0.0, 0.0), pt(1e6, 0.0), pt(2e6, 1.0)]];
    assert!(draw(&huge, &hand, 0.3, 0.03, Pen::Ballpoint, blue(), 130.0, &mut copy, &o).is_none());
    // A very long name stays within the work limits (rejected or rendered, never unbounded).
    let long = "Abcdefghijklmnopqrstuvwxyz ".repeat(40);
    if let Some(image) = render(&long, d, 1, blue(), Pen::Ballpoint, 130.0) {
        assert!(image.width() < 6000 && image.height() < 4000);
    }
}

#[test]
fn unknown_scripts_and_digits_still_render() {
    let d = design(SignatureForm::FullName, 0, 3);
    for name in ["李 小龙", "Ωmega 42", "Ödön Ünal", "Ёлка Ïgor"] {
        assert!(render(name, d, 3, blue(), Pen::Ballpoint, 130.0).is_some(), "{name}");
    }
}

#[test]
fn design_seeds_propose_distinct_plans_of_the_form() {
    let mut rng = SeededRandom::new(9);
    for form in SignatureForm::ALL {
        let seeds = design_seeds_with(3, form, &mut rng);
        assert_eq!(seeds.len(), 3);
        let allowed = form.archetypes();
        let plans: Vec<Archetype> = seeds.iter().map(|&d| style(d).archetype).collect();
        assert!(plans.iter().all(|a| allowed.contains(a)), "{form:?} {plans:?}");
        if allowed.len() >= 3 {
            assert!(plans[0] != plans[1] && plans[1] != plans[2] && plans[0] != plans[2], "{form:?} {plans:?}");
        }
        for d in &seeds {
            assert!(d >> 16 >= 1 && (1..=STYLE_COUNT).contains(&style_number(*d)));
        }
    }
    assert_ne!(design_seeds(3, SignatureForm::Any), design_seeds(3, SignatureForm::Any));
}

#[test]
fn picture_copies_vary_and_keep_ink() {
    let d = design(SignatureForm::InitialSurname, 1, 501);
    let picture = render("Sergey Zorin", d, 77, InkColor::Black.pen(), Pen::Ballpoint, 130.0).unwrap();
    let grey = warp::encode(&picture).unwrap();
    assert_eq!(grey.dimensions(), picture.dimensions());
    let a = warp::copy(&picture, blue(), 1, 300.0).unwrap();
    let b = warp::copy(&picture, blue(), 2, 300.0).unwrap();
    assert!(inked(&a) > 500 && inked(&b) > 500);
    assert!(a.dimensions() != b.dimensions() || a.as_raw() != b.as_raw());
    assert!(warp::copy(&picture, blue(), 1, f64::NAN).is_none());
}

/// Values printed by the Kit42 Swift sources (SeededRandom, `Double.random(in:using:)`, SigmaLognormal and
/// `SignatureGenerator.plan`) for the same seeds: the port keeps every random draw in the same order.
#[test]
fn matches_kit42_reference_values() {
    let mut r = SeededRandom::new(7);
    let v: Vec<f64> = (0..3).map(|_| kinematics::swift_double(&mut r, 0.2, 0.3)).collect();
    assert_eq!(v, [0.20167882945281562, 0.29007606806068836, 0.2582930293028078]);

    let hand = Hand { overlap: 0.7, size_scaled: false, ..Hand::default() };
    let mut rr = SeededRandom::new(11);
    let via = [pt(0.0, 0.0), pt(0.3, 0.5), pt(0.6, 0.1), pt(1.0, 0.4)];
    let comps = kinematics::components(&via, &HashSet::from([1]), None, &hand, &mut rr);
    let comps = kinematics::perturb(&comps, 1.0, &mut rr);
    let expected = [
        [0.5596682359081048, 0.0, -2.764108168209135, 0.236303030271448, 1.0110820875667872, 1.0518501415607913],
        [0.49550507624904067, 0.10148910539212869, -2.8526396432109773, 0.2977922323910394, -1.2154037304162242, -0.509695511877049],
        [0.5239415117665578, 0.1435476026946568, -2.8680655444647565, 0.2754888945176974, 0.29665642804602377, 1.0416968820854569],
    ];
    for (c, e) in comps.iter().zip(expected) {
        let got = [c.d, c.t0, c.mu, c.sigma, c.theta_start, c.theta_end];
        for (g, e) in got.iter().zip(e) {
            assert!((g - e).abs() < 1e-12, "{got:?} vs {e:?}");
        }
    }
    let profile = kinematics::speed_profile(&comps, 1.0 / 400.0);
    assert_eq!(profile.len(), 90);
    assert!((profile.iter().map(|s| s.1).sum::<f64>() - 1.574711721001574).abs() < 1e-9);

    // Plans: number of strokes, points per stroke, last point and the next draw of the copy generator.
    let cases: [(&str, SignatureForm, usize, u64, u64, Vec<usize>, Pt, u64); 3] = [
        ("Sergey Zorin", SignatureForm::Surname, 2, 33, design(SignatureForm::Surname, 2, 33), vec![32, 3, 3, 2], pt(1.4849927461905132, 0.5038025552314409), 0),
        ("Иван Петров", SignatureForm::FullName, 3, 38, 2490527, vec![80], pt(-0.39406541603929657, -0.1255990324692262), 17110216471660409307),
        ("Ayşe Yılmaz", SignatureForm::InitialSurname, 7, 46, 3015158, vec![7, 30, 5], pt(4.231354294898304, -0.018910311534151666), 12117850117357256376),
    ];
    for (name, form, variant, seed, d, counts, last, next) in cases {
        assert_eq!(design(form, variant, seed), d);
        let mut copy = SeededRandom::new(7919);
        let raw = plan(&words(name), &style(d), &mut copy);
        assert_eq!(raw.iter().map(|s| s.len()).collect::<Vec<_>>(), counts, "{name}");
        let end = *raw.last().unwrap().last().unwrap();
        assert!((end.x - last.x).abs() < 1e-12 && (end.y - last.y).abs() < 1e-12, "{name}: {end:?}");
        if next != 0 {
            assert_eq!(copy.next_u64(), next, "{name}");
        }
    }
}
