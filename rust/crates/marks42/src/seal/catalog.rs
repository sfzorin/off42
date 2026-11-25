//! Catalogue of stamp and seal templates by region and purpose (Kit42 `SealCatalog`). Generic company, personal
//! and office stamps only: no state emblems, and no notary, court, customs, police or medical-prescription seals.
//!
//! Every text on a stamp is a field the user can edit; the template gives an example. Fields: `key`, English label
//! and example text. Titles come in English, Russian and Turkish, as in the macOS catalogue.

use crate::InkColor;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SealCategory {
    Cis,
    Europe,
    Uk,
    Turkey,
    Americas,
    HongKong,
    China,
    Japan,
    Korea,
    SouthAsia,
    MiddleEast,
    LatinAmerica,
    Office,
    Dates,
    Personal,
}

impl SealCategory {
    pub const ALL: [SealCategory; 15] = [
        Self::Cis,
        Self::Europe,
        Self::Uk,
        Self::Turkey,
        Self::Americas,
        Self::HongKong,
        Self::China,
        Self::Japan,
        Self::Korea,
        Self::SouthAsia,
        Self::MiddleEast,
        Self::LatinAmerica,
        Self::Office,
        Self::Dates,
        Self::Personal,
    ];

    /// English title (the macOS app translates it in Kit42/Strings.swift).
    pub fn title(self) -> &'static str {
        match self {
            Self::Cis => "Russia and CIS",
            Self::Europe => "Europe",
            Self::Uk => "United Kingdom and Ireland",
            Self::Turkey => "Turkey",
            Self::Americas => "USA and Canada",
            Self::HongKong => "Hong Kong and Taiwan",
            Self::China => "Mainland China",
            Self::Japan => "Japan",
            Self::Korea => "Korea",
            Self::SouthAsia => "India and South Asia",
            Self::MiddleEast => "Middle East",
            Self::LatinAmerica => "Latin America",
            Self::Office => "Office stamps",
            Self::Dates => "Date and registration stamps",
            Self::Personal => "Personal stamps",
        }
    }
}

/// One editable text of a stamp.
#[derive(Clone, Debug, PartialEq)]
pub struct SealField {
    pub key: &'static str,
    /// English label (translated by the app).
    pub label: &'static str,
    /// Example text used when the user leaves the field untouched.
    pub example: &'static str,
    pub multiline: bool,
}

/// Families of layouts the renderer knows; each template is one family with its options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Ring {
    Solid,
    Thick,
    Thin,
    Dotted,
    Rope,
    Serrated,
    Stars,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Edge {
    Full,
    TopBottom,
    Top,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Centre {
    Lines,
    Star,
    Vertical,
    #[allow(dead_code)]
    Grid,
    Initials,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Round {
    pub ring: Ring,
    pub double: bool,
    pub edge: Edge,
    /// Second, smaller ring of text ("inner" field).
    pub inner: bool,
    pub centre: Centre,
    pub font: &'static str,
    pub centre_font: &'static str,
    /// Small round chop: one ring, larger letters.
    pub small: bool,
}

const R: Round = Round {
    ring: Ring::Solid,
    double: true,
    edge: Edge::Full,
    inner: false,
    centre: Centre::Lines,
    font: COND,
    centre_font: SANS,
    small: false,
};
const OVAL: Round = Round { edge: Edge::TopBottom, ..R };

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Frame {
    None,
    Single,
    Double,
    Rounded,
    Thick,
}

#[derive(Clone, Debug)]
pub(crate) enum Row {
    Text {
        key: &'static str,
        font: &'static str,
        size: f64,
        upper: bool,
    },
    Space(f64),
    /// Signature line, fraction of the width.
    Line(f64),
    Divider,
}

#[derive(Clone, Debug)]
pub(crate) struct Rect {
    pub rows: Vec<Row>,
    pub frame: Frame,
    pub left: bool,
    pub width: f64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Word {
    pub frame: Frame,
    pub rounded: bool,
    pub font: &'static str,
}

const W: Word = Word { frame: Frame::Double, rounded: true, font: BLACK };

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Shape {
    Square,
    Round,
    Oval,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Grid {
    pub shape: Shape,
    pub font: &'static str,
    pub frame: f64,
    pub per_column: Option<usize>,
}

#[derive(Clone, Debug)]
pub(crate) enum Layout {
    Round(Round),
    Oval(Round),
    Rect(Rect),
    Word(Word),
    Grid(Grid),
    Dater(&'static str),
    Table,
    Triangle,
}

/// A stamp style: its editable fields with example texts, default ink and layout.
#[derive(Clone, Debug)]
pub struct SealTemplate {
    pub id: &'static str,
    pub category: SealCategory,
    /// Title in English, Russian and Turkish.
    pub titles: [&'static str; 3],
    pub fields: Vec<SealField>,
    /// Default ink (use `ink.stamp_pad()` for the colour).
    pub ink: InkColor,
    pub(crate) layout: Layout,
}

impl SealTemplate {
    /// English title.
    pub fn title(&self) -> &'static str {
        self.titles[0]
    }

    /// The editable fields, in the order the editor shows them.
    pub fn fields(&self) -> &[SealField] {
        &self.fields
    }

    /// The example text for a field ("" for an unknown key).
    pub fn example(&self, key: &str) -> &'static str {
        self.fields.iter().find(|f| f.key == key).map_or("", |f| f.example)
    }

    /// Whether impressions of this stamp tilt like a round seal (more) or a rectangular stamp (less).
    pub fn is_round(&self) -> bool {
        matches!(
            self.layout,
            Layout::Round(_) | Layout::Oval(_) | Layout::Grid(_) | Layout::Dater(_) | Layout::Triangle
        )
    }

    /// Tilt range in radians: rectangular stamps are aligned by eye along the text, round ones are not.
    pub(crate) fn tilt(&self) -> (f64, f64) {
        match self.layout {
            Layout::Rect(_) | Layout::Table => (0.005, 0.035),
            Layout::Word(_) => (0.02, 0.12),
            _ => (0.035, 0.14),
        }
    }
}

/// Every catalogue template, in the macOS order.
pub fn templates() -> &'static [SealTemplate] {
    static ALL: OnceLock<Vec<SealTemplate>> = OnceLock::new();
    ALL.get_or_init(catalog)
}

/// The template with this id.
pub fn named(id: &str) -> Option<&'static SealTemplate> {
    templates().iter().find(|t| t.id == id)
}

// Fonts
pub(crate) const COND: &str = "HelveticaNeue-CondensedBold";
pub(crate) const SANS: &str = "HelveticaNeue-Bold";
pub(crate) const MEDIUM: &str = "HelveticaNeue-Medium";
pub(crate) const SERIF: &str = "TimesNewRomanPS-BoldMT";
pub(crate) const BLACK: &str = "HelveticaNeue-CondensedBlack";
pub(crate) const MONO: &str = "Courier-Bold";
const SONG: &str = "STSongti-SC-Black";
const SONG_TC: &str = "STSongti-TC-Bold";
const MINCHO: &str = "HiraMinProN-W6";
const HANGUL: &str = "AppleMyungjo";
const HANGUL_SANS: &str = "AppleSDGothicNeo-Bold";
const ARABIC: &str = "GeezaPro-Bold";
const HEBREW: &str = "ArialHebrew-Bold";

use InkColor::{Black as K, Blue as B, Green as G, Red as Rd, Violet as V};
use SealCategory as C;

// Field helpers
fn f(key: &'static str, label: &'static str, example: &'static str) -> SealField {
    SealField { key, label, example, multiline: false }
}
fn fm(key: &'static str, label: &'static str, example: &'static str) -> SealField {
    SealField { key, label, example, multiline: true }
}
fn edge(s: &'static str) -> SealField {
    f("edge", "Text around the edge", s)
}
fn top(s: &'static str) -> SealField {
    f("edge", "Text around the top", s)
}
fn bottom(s: &'static str) -> SealField {
    f("bottom", "Text around the bottom", s)
}
fn inner(s: &'static str) -> SealField {
    f("inner", "Inner ring (optional)", s)
}
fn centre(s: &'static str) -> SealField {
    fm("centre", "Centre text", s)
}
fn name(s: &'static str) -> SealField {
    f("name", "Company name", s)
}
fn lines(s: &'static str) -> SealField {
    fm("lines", "Address lines", s)
}
fn numbers(s: &'static str) -> SealField {
    fm("numbers", "Registration numbers", s)
}
fn wordf(s: &'static str) -> SealField {
    f("word", "Main word", s)
}
fn date(s: &'static str) -> SealField {
    f("date", "Date or note (empty: today)", s)
}
fn caption(s: &'static str) -> SealField {
    f("caption", "Caption", s)
}
fn toprow(s: &'static str) -> SealField {
    f("top", "Top line (optional)", s)
}
fn chars(s: &'static str) -> SealField {
    f("chars", "Characters", s)
}

// Row helpers
fn t(key: &'static str, size: f64, font: &'static str) -> Row {
    Row::Text { key, font, size, upper: false }
}
fn tu(key: &'static str, size: f64, font: &'static str) -> Row {
    Row::Text { key, font, size, upper: true }
}

type Title = [&'static str; 3];

fn tpl(
    id: &'static str,
    category: C,
    titles: Title,
    ink: InkColor,
    fields: Vec<SealField>,
    layout: Layout,
) -> SealTemplate {
    SealTemplate { id, category, titles, fields, ink, layout }
}
fn round(id: &'static str, c: C, title: Title, ink: InkColor, fields: Vec<SealField>, r: Round) -> SealTemplate {
    tpl(id, c, title, ink, fields, Layout::Round(r))
}
fn oval(id: &'static str, c: C, title: Title, ink: InkColor, fields: Vec<SealField>, r: Round) -> SealTemplate {
    tpl(id, c, title, ink, fields, Layout::Oval(r))
}
#[allow(clippy::too_many_arguments)]
fn rect_with(
    id: &'static str,
    c: C,
    title: Title,
    ink: InkColor,
    fields: Vec<SealField>,
    rows: Vec<Row>,
    frame: Frame,
    left: bool,
) -> SealTemplate {
    tpl(id, c, title, ink, fields, Layout::Rect(Rect { rows, frame, left, width: 1300.0 }))
}
fn rect(id: &'static str, c: C, title: Title, ink: InkColor, fields: Vec<SealField>, rows: Vec<Row>) -> SealTemplate {
    rect_with(id, c, title, ink, fields, rows, Frame::None, false)
}
fn rect_framed(
    id: &'static str,
    c: C,
    title: Title,
    ink: InkColor,
    fields: Vec<SealField>,
    rows: Vec<Row>,
    frame: Frame,
) -> SealTemplate {
    rect_with(id, c, title, ink, fields, rows, frame, false)
}
fn rect_left(
    id: &'static str,
    c: C,
    title: Title,
    ink: InkColor,
    fields: Vec<SealField>,
    rows: Vec<Row>,
) -> SealTemplate {
    rect_with(id, c, title, ink, fields, rows, Frame::None, true)
}
#[allow(clippy::too_many_arguments)]
fn word_with(
    id: &'static str,
    c: C,
    title: Title,
    ink: InkColor,
    w: &'static str,
    top: &'static str,
    d: &'static str,
    style: Word,
) -> SealTemplate {
    tpl(id, c, title, ink, vec![toprow(top), wordf(w), date(d)], Layout::Word(style))
}
fn word(id: &'static str, c: C, title: Title, ink: InkColor, w: &'static str) -> SealTemplate {
    word_with(id, c, title, ink, w, "", "", W)
}
fn word_top(id: &'static str, c: C, title: Title, ink: InkColor, w: &'static str, top: &'static str) -> SealTemplate {
    word_with(id, c, title, ink, w, top, "", W)
}
/// A word stamp without a date line (the example is a single space, so an empty field does not mean today).
fn word_nodate(id: &'static str, c: C, title: Title, ink: InkColor, w: &'static str) -> SealTemplate {
    word_with(id, c, title, ink, w, "", " ", W)
}
fn grid(id: &'static str, c: C, title: Title, ink: InkColor, example: &'static str, g: Grid) -> SealTemplate {
    tpl(id, c, title, ink, vec![chars(example)], Layout::Grid(g))
}
fn g(shape: Shape, font: &'static str, frame: f64, per_column: Option<usize>) -> Grid {
    Grid { shape, font, frame, per_column }
}

/// Standard rectangular company stamp: bold name, address lines, registration numbers.
#[allow(clippy::too_many_arguments)]
fn company(
    id: &'static str,
    c: C,
    title: Title,
    ink: InkColor,
    n: &'static str,
    l: &'static str,
    num: &'static str,
    frame: Frame,
    left: bool,
    upper: bool,
) -> SealTemplate {
    let name_row = if upper { tu("name", 92.0, SANS) } else { t("name", 92.0, SANS) };
    rect_with(
        id,
        c,
        title,
        ink,
        vec![name(n), lines(l), numbers(num)],
        vec![name_row, t("lines", 62.0, MEDIUM), t("numbers", 52.0, MEDIUM)],
        frame,
        left,
    )
}

/// "For and on behalf of … Authorised Signature" chops.
#[allow(clippy::too_many_arguments)]
fn behalf(
    id: &'static str,
    c: C,
    title: Title,
    ink: InkColor,
    tp: &'static str,
    n: &'static str,
    local: Option<&'static str>,
    cap: &'static str,
    frame: Frame,
) -> SealTemplate {
    let mut fields = vec![toprow(tp), name(n)];
    let mut rows = vec![t("top", 50.0, MEDIUM), tu("name", 84.0, SANS)];
    if let Some(local) = local {
        fields.push(f("local", "Name in local script", local));
        rows.push(t("local", 74.0, SONG_TC));
    }
    fields.push(caption(cap));
    rows.extend([Row::Space(150.0), Row::Line(0.62), t("caption", 48.0, MEDIUM)]);
    rect_framed(id, c, title, ink, fields, rows, frame)
}

#[rustfmt::skip]
fn catalog() -> Vec<SealTemplate> {
    use Frame as F;
    let m = MEDIUM;
    vec![
        // MARK: Russia and CIS
        round("ru.ooo", C::Cis, ["Round seal with registration ring", "Круглая печать с кольцом ОГРН/ИНН", "Sicil halkalı yuvarlak mühür"], V,
              vec![edge("ООО «РОМАШКА» * г. МОСКВА *"), inner("ОГРН 1234567890123 * ИНН 7700000000"), centre("ДЛЯ\nДОКУМЕНТОВ")],
              Round { inner: true, ..R }),
        round("ru.simple", C::Cis, ["Round company seal", "Круглая печать организации", "Yuvarlak şirket mührü"], B,
              vec![edge("ОБЩЕСТВО С ОГРАНИЧЕННОЙ ОТВЕТСТВЕННОСТЬЮ * МОСКВА *"), centre("«РОМАШКА»\nИНН 7700000000")], Round { double: false, ..R }),
        round("ru.ip", C::Cis, ["Sole proprietor seal", "Печать ИП", "Şahıs işletmesi mührü"], B,
              vec![edge("ИНДИВИДУАЛЬНЫЙ ПРЕДПРИНИМАТЕЛЬ * г. КАЗАНЬ *"), inner("ОГРНИП 312345678901234"), centre("ИВАНОВ\nИВАН\nИВАНОВИЧ")],
              Round { inner: true, ..R }),
        round("ru.stars", C::Cis, ["Round seal with star border", "Круглая печать с каймой из звёзд", "Yıldız kenarlı yuvarlak mühür"], V,
              vec![edge("ООО «ВЕКТОР» * САНКТ-ПЕТЕРБУРГ *"), centre("ДЛЯ\nСЧЕТОВ")], Round { ring: Ring::Stars, ..R }),
        round("ru.rope", C::Cis, ["Round seal with rope border", "Круглая печать с витой каймой", "Burgu kenarlı yuvarlak mühür"], B,
              vec![edge("АО «СЕВЕР» * НОВОСИБИРСК *"), centre("ОТДЕЛ\nКАДРОВ")], Round { ring: Ring::Rope, ..R }),
        round("ru.dotted", C::Cis, ["Round seal with dotted ring", "Круглая печать с точечным кольцом", "Noktalı halkalı yuvarlak mühür"], V,
              vec![edge("ООО «ЗАРЯ» * ЕКАТЕРИНБУРГ *"), inner("ИНН 6600000000 * КПП 660001001"), centre("БУХГАЛТЕРИЯ")],
              Round { ring: Ring::Dotted, inner: true, ..R }),
        round("ru.small", C::Cis, ["Small round stamp", "Малая круглая печать", "Küçük yuvarlak kaşe"], B,
              vec![edge("ООО «РОМАШКА» * МОСКВА *"), centre("ДЛЯ\nПАКЕТОВ")], Round { double: false, small: true, ..R }),
        oval("ru.oval", C::Cis, ["Oval stamp", "Овальный штамп", "Oval kaşe"], V,
             vec![top("ООО «РОМАШКА»"), bottom("г. МОСКВА"), centre("ДЛЯ\nДОКУМЕНТОВ")], OVAL),
        rect("ru.corner", C::Cis, ["Corner stamp", "Угловой штамп", "Antet kaşesi"], B,
             vec![name("ООО «РОМАШКА»"), lines("125009, г. Москва, ул. Тверская, д. 1\nтел. +7 495 000-00-00"),
                  numbers("ИНН 7700000000 / КПП 770001001"), f("ref", "Reference line", "Исх. № ______ от «___» ________ 20__ г.")],
             vec![tu("name", 80.0, SANS), t("lines", 50.0, m), t("numbers", 48.0, m), Row::Space(20.0), t("ref", 48.0, m)]),
        rect_left("ru.copy", C::Cis, ["“Certified copy” stamp", "Штамп «Копия верна»", "“Aslı gibidir” kaşesi (RU)"], B,
             vec![wordf("КОПИЯ ВЕРНА"), f("position", "Position", "Генеральный директор"),
                  f("sign", "Signature line", "________________ / И. И. Иванов /"), date("«___» ____________ 20__ г.")],
             vec![t("word", 96.0, SANS), t("position", 54.0, m), t("sign", 54.0, m), t("date", 54.0, m)]),
        rect_framed("ru.incoming", C::Cis, ["Incoming document stamp", "Штамп входящего документа", "Gelen evrak kaşesi (RU)"], B,
             vec![name("ООО «РОМАШКА»"), wordf("ВХОДЯЩИЙ № ________"), date("«___» ____________ 20__ г.")],
             vec![tu("name", 54.0, SANS), t("word", 74.0, SANS), t("date", 60.0, m)], F::Single),
        rect_left("ru.approve", C::Cis, ["“Approved” block", "Гриф «Утверждаю»", "“Onaylıyorum” bloğu"], B,
             vec![wordf("УТВЕРЖДАЮ"), fm("position", "Position", "Генеральный директор\nООО «РОМАШКА»"),
                  f("sign", "Signature line", "____________ И. И. Иванов"), date("«___» ____________ 20__ г.")],
             vec![t("word", 80.0, SANS), t("position", 56.0, m), t("sign", 56.0, m), t("date", 56.0, m)]),
        rect_left("ru.agreed", C::Cis, ["“Agreed” block", "Гриф «Согласовано»", "“Mutabık kalındı” bloğu"], B,
             vec![wordf("СОГЛАСОВАНО"), fm("position", "Position", "Главный бухгалтер"),
                  f("sign", "Signature line", "____________ П. П. Петров"), date("«___» ____________ 20__ г.")],
             vec![t("word", 80.0, SANS), t("position", 56.0, m), t("sign", 56.0, m), t("date", 56.0, m)]),
        word_top("ru.paid", C::Cis, ["“Paid” stamp (Russian)", "Штамп «Оплачено»", "“Ödendi” kaşesi (RU)"], B, "ОПЛАЧЕНО", "ООО «РОМАШКА»"),
        word_top("ru.received", C::Cis, ["“Received” stamp (Russian)", "Штамп «Получено»", "“Teslim alındı” kaşesi (RU)"], V, "ПОЛУЧЕНО", "ООО «РОМАШКА»"),
        tpl("ru.qc", C::Cis, ["Quality control stamp", "Штамп ОТК", "Kalite kontrol kaşesi"], V,
            vec![wordf("ОТК"), f("number", "Number", "5")], Layout::Triangle),
        round("kz.round", C::Cis, ["Kazakhstan round seal (two languages)", "Круглая печать Казахстана (два языка)", "Kazakistan yuvarlak mühür (iki dil)"], B,
              vec![top("«ҮЛГІ» ЖШС * АЛМАТЫ"), bottom("ТОО «ОБРАЗЕЦ» * АЛМАТЫ"), centre("БСН\n123456789012")], Round { edge: Edge::TopBottom, ..R }),
        round("ua.round", C::Cis, ["Ukraine round seal", "Круглая печать Украины", "Ukrayna yuvarlak mühür"], B,
              vec![edge("ТОВАРИСТВО З ОБМЕЖЕНОЮ ВІДПОВІДАЛЬНІСТЮ «ЗРАЗОК»"), inner("ІДЕНТИФІКАЦІЙНИЙ КОД 12345678"), centre("м. КИЇВ")],
              Round { inner: true, ..R }),
        round("by.round", C::Cis, ["Belarus round seal", "Круглая печать Беларуси", "Belarus yuvarlak mühür"], B,
              vec![edge("ОБЩЕСТВО С ОГРАНИЧЕННОЙ ОТВЕТСТВЕННОСТЬЮ «ОБРАЗЕЦ»"), centre("УНП\n190000000\nг. МИНСК")], R),

        // MARK: Europe
        company("de.firm", C::Europe, ["Company stamp (Germany)", "Штамп компании (Германия)", "Şirket kaşesi (Almanya)"], B,
                "Musterfirma GmbH", "Hauptstraße 1\n10115 Berlin", "USt-IdNr. DE123456789", F::Rounded, false, false),
        company("de.firm.plain", C::Europe, ["Company stamp, left aligned (Germany)", "Штамп компании, по левому краю (Германия)", "Sola hizalı şirket kaşesi (Almanya)"], B,
                "Muster & Söhne KG", "Bahnhofstr. 12 · 80335 München\nTel. 089 123456", "HRA 12345 · Amtsgericht München", F::None, true, false),
        rect_framed("de.eingang", C::Europe, ["“Received” date stamp (Germany)", "Штамп «Получено» с датой (Германия)", "Tarihli “Alındı” kaşesi (Almanya)"], B,
             vec![name("Musterfirma GmbH"), wordf("EINGANG"), date(""), f("by", "Handled by", "Bearb.: ________")],
             vec![t("name", 52.0, SANS), t("word", 96.0, BLACK), t("date", 70.0, MONO), t("by", 48.0, m)], F::Double),
        company("at.firm", C::Europe, ["Company stamp (Austria)", "Штамп компании (Австрия)", "Şirket kaşesi (Avusturya)"], B,
                "Beispiel GmbH", "Ringstraße 5, 1010 Wien", "FN 123456a · UID ATU12345678", F::Single, false, false),
        company("ch.firm", C::Europe, ["Company stamp (Switzerland)", "Штамп компании (Швейцария)", "Şirket kaşesi (İsviçre)"], K,
                "Muster AG", "Bahnhofstrasse 10\n8001 Zürich", "UID CHE-123.456.789 MWST", F::None, false, false),
        company("fr.tampon", C::Europe, ["Company stamp (France)", "Штамп компании (Франция)", "Şirket kaşesi (Fransa)"], B,
                "EXEMPLE SARL", "12 rue de la Paix\n75002 Paris", "SIRET 123 456 789 00012\nTVA FR12 123456789", F::Rounded, false, false),
        word_top("fr.recu", C::Europe, ["“Reçu le” stamp (France)", "Штамп «Получено» (Франция)", "“Alındı” kaşesi (Fransa)"], B, "REÇU LE", "EXEMPLE SARL"),
        company("it.timbro", C::Europe, ["Company stamp (Italy)", "Штамп компании (Италия)", "Şirket kaşesi (İtalya)"], B,
                "ESEMPIO S.R.L.", "Via Roma, 1 – 00100 Roma", "P.IVA 01234567890\nC.F. 01234567890", F::None, false, true),
        round("it.round", C::Europe, ["Round company seal (Italy)", "Круглая печать (Италия)", "Yuvarlak şirket mührü (İtalya)"], B,
              vec![top("ESEMPIO S.R.L."), bottom("ROMA"), centre("P.IVA\n01234567890")], Round { edge: Edge::TopBottom, ..R }),
        oval("es.sello", C::Europe, ["Oval company stamp (Spain)", "Овальный штамп (Испания)", "Oval şirket kaşesi (İspanya)"], B,
             vec![top("EJEMPLO S.L."), bottom("MADRID"), centre("CIF B12345678")], OVAL),
        company("es.rect", C::Europe, ["Company stamp (Spain)", "Штамп компании (Испания)", "Şirket kaşesi (İspanya)"], B,
                "EJEMPLO S.L.", "C/ Mayor, 1 · 28013 Madrid", "CIF B12345678", F::Single, false, true),
        company("pt.carimbo", C::Europe, ["Company stamp (Portugal)", "Штамп компании (Португалия)", "Şirket kaşesi (Portekiz)"], B,
                "EXEMPLO, LDA.", "Rua Augusta, 1 · 1100-048 Lisboa", "NIF 501234567", F::Rounded, false, false),
        company("pl.pieczatka", C::Europe, ["Company stamp (Poland)", "Штамп компании (Польша)", "Şirket kaşesi (Polonya)"], B,
                "PRZYKŁAD Sp. z o.o.", "ul. Marszałkowska 1, 00-001 Warszawa", "NIP 123-456-78-90 · REGON 123456789", F::None, false, false),
        company("cz.razitko", C::Europe, ["Company stamp (Czechia, Slovakia)", "Штамп компании (Чехия, Словакия)", "Şirket kaşesi (Çekya, Slovakya)"], B,
                "Příklad s.r.o.", "Václavské nám. 1, 110 00 Praha 1", "IČ 12345678 · DIČ CZ12345678", F::Single, false, false),
        company("nl.stempel", C::Europe, ["Company stamp (Netherlands, Belgium)", "Штамп компании (Нидерланды, Бельгия)", "Şirket kaşesi (Hollanda, Belçika)"], B,
                "Voorbeeld B.V.", "Damrak 1, 1012 LG Amsterdam", "KvK 12345678 · BTW NL123456789B01", F::None, false, false),
        company("nordic.stamp", C::Europe, ["Company stamp (Nordic countries)", "Штамп компании (Скандинавия)", "Şirket kaşesi (İskandinav ülkeleri)"], K,
                "Exempel AB", "Drottninggatan 1, 111 51 Stockholm", "Org.nr 556123-4567", F::None, true, false),
        company("gr.stamp", C::Europe, ["Company stamp (Greece)", "Штамп компании (Греция)", "Şirket kaşesi (Yunanistan)"], B,
                "ΠΑΡΑΔΕΙΓΜΑ Α.Ε.", "Οδός Ερμού 1, 105 63 Αθήνα", "ΑΦΜ 123456789 · ΔΟΥ Α' Αθηνών", F::None, false, true),
        round("eu.round", C::Europe, ["Round company seal (Europe)", "Круглая печать (Европа)", "Yuvarlak şirket mührü (Avrupa)"], B,
              vec![edge("EXAMPLE TRADING S.A. * BRUXELLES *"), centre("TVA\nBE 0123.456.789")], Round { ring: Ring::Dotted, ..R }),

        // MARK: United Kingdom and Ireland
        round("uk.common", C::Uk, ["Common seal (UK)", "Общая печать компании (Великобритания)", "Ortak mühür (Birleşik Krallık)"], K,
              vec![edge("THE COMMON SEAL OF EXAMPLE TRADING LIMITED *"), centre("Company No.\n01234567")],
              Round { ring: Ring::Rope, font: SERIF, centre_font: SERIF, ..R }),
        company("uk.company", C::Uk, ["Company stamp (UK)", "Штамп компании (Великобритания)", "Şirket kaşesi (Birleşik Krallık)"], B,
                "Example Trading Ltd", "1 High Street, London EC1A 1AA", "Registered in England No. 01234567\nVAT No. GB 123 4567 89", F::None, false, false),
        oval("uk.received", C::Uk, ["Oval “Received” stamp", "Овальный штамп «Получено»", "Oval “Alındı” kaşesi"], B,
             vec![top("EXAMPLE TRADING LTD"), bottom("LONDON"), centre("RECEIVED")], OVAL),
        word_top("uk.paid", C::Uk, ["“Paid” stamp (UK)", "Штамп «Paid» (Великобритания)", "“Paid” kaşesi (Birleşik Krallık)"], Rd, "PAID", "EXAMPLE TRADING LTD"),
        company("ie.company", C::Uk, ["Company stamp (Ireland)", "Штамп компании (Ирландия)", "Şirket kaşesi (İrlanda)"], G,
                "Example Holdings DAC", "1 Grafton Street, Dublin 2", "CRO No. 123456 · VAT IE1234567T", F::Single, false, false),

        // MARK: Turkey
        rect("tr.kase", C::Turkey, ["Kaşe (company stamp)", "Каше (штамп компании)", "Şirket kaşesi"], V,
             vec![name("ÖRNEK TİCARET LTD. ŞTİ."), lines("Atatürk Cad. No: 1\nKadıköy / İSTANBUL"), numbers("Kadıköy V.D. 1234567890")],
             vec![tu("name", 88.0, SANS), t("lines", 62.0, m), t("numbers", 54.0, m)]),
        rect_framed("tr.kase.framed", C::Turkey, ["Framed kaşe", "Каше в рамке", "Çerçeveli kaşe"], B,
             vec![name("ÖRNEK İNŞAAT A.Ş."), lines("Cumhuriyet Mah. 1. Sok. No: 5\nÇankaya / ANKARA"), numbers("Çankaya V.D. 1234567890\nMersis No: 0123456789000015")],
             vec![tu("name", 84.0, SANS), Row::Divider, t("lines", 56.0, m), t("numbers", 50.0, m)], F::Single),
        rect("tr.sahis", C::Turkey, ["Sole trader kaşe", "Каше индивидуального предпринимателя", "Şahıs şirketi kaşesi"], B,
             vec![name("AHMET YILMAZ"), lines("Örnek Ticaret\nKonak / İZMİR"), numbers("Konak V.D. 12345678901")],
             vec![tu("name", 84.0, SANS), t("lines", 60.0, m), t("numbers", 52.0, m)]),
        word_top("tr.asli", C::Turkey, ["“Aslı gibidir” stamp", "Штамп «Aslı gibidir» (копия верна)", "“Aslı gibidir” kaşesi"], B, "ASLI GİBİDİR", "ÖRNEK TİCARET LTD. ŞTİ."),
        word("tr.teslim", C::Turkey, ["“Teslim alındı” stamp", "Штамп «Teslim alındı» (получено)", "“Teslim alındı” kaşesi"], V, "TESLİM ALINDI"),
        word("tr.odendi", C::Turkey, ["“Ödendi” stamp", "Штамп «Ödendi» (оплачено)", "“Ödendi” kaşesi"], Rd, "ÖDENDİ"),
        round("tr.round", C::Turkey, ["Round association seal", "Круглая печать объединения", "Yuvarlak dernek mührü"], B,
              vec![top("ÖRNEK KÜLTÜR DERNEĞİ"), bottom("İSTANBUL"), centre("2026")], Round { edge: Edge::TopBottom, ..R }),

        // MARK: USA and Canada
        round("us.corporate", C::Americas, ["Corporate seal (USA)", "Корпоративная печать (США)", "Kurumsal mühür (ABD)"], K,
              vec![top("EXAMPLE CORPORATION"), bottom("DELAWARE"), centre("CORPORATE\nSEAL\n2026")],
              Round { ring: Ring::Serrated, edge: Edge::TopBottom, font: SERIF, centre_font: SERIF, ..R }),
        round("us.corporate.plain", C::Americas, ["Corporate seal, plain border", "Корпоративная печать, простая кайма", "Düz kenarlı kurumsal mühür"], K,
              vec![top("EXAMPLE HOLDINGS INC."), bottom("NEW YORK"), centre("SEAL\n2026")],
              Round { edge: Edge::TopBottom, font: SERIF, centre_font: SERIF, ..R }),
        rect_left("us.address", C::Americas, ["Address stamp", "Адресный штамп", "Adres kaşesi"], K,
             vec![name("Example Company, Inc."), lines("100 Main Street, Suite 200\nSpringfield, IL 62701")],
             vec![t("name", 82.0, SANS), t("lines", 62.0, m)]),
        word("us.paid", C::Americas, ["“Paid” stamp", "Штамп «Paid»", "“Paid” kaşesi"], Rd, "PAID"),
        rect_framed("us.received", C::Americas, ["“Received” stamp with “By”", "Штамп «Received» с «By»", "“By” alanlı “Received” kaşesi"], B,
             vec![wordf("RECEIVED"), date(""), f("by", "Handled by", "BY: ____________")],
             vec![t("word", 110.0, BLACK), t("date", 70.0, MONO), t("by", 54.0, m)], F::Double),
        rect_framed("us.deposit", C::Americas, ["“For deposit only” endorsement", "Индоссамент «For deposit only»", "“For deposit only” cirosu"], K,
             vec![wordf("FOR DEPOSIT ONLY"), name("EXAMPLE COMPANY, INC."), f("account", "Account line", "ACCT # ____________")],
             vec![t("word", 74.0, SANS), tu("name", 62.0, m), t("account", 56.0, m)], F::Single),
        round("ca.corporate", C::Americas, ["Corporate seal (Canada, bilingual)", "Корпоративная печать (Канада, два языка)", "Kurumsal mühür (Kanada, iki dilli)"], K,
              vec![top("EXAMPLE CORPORATION"), bottom("CORPORATE SEAL · SCEAU"), centre("ONTARIO\n2026")],
              Round { ring: Ring::Serrated, edge: Edge::TopBottom, font: SERIF, centre_font: SERIF, ..R }),

        // MARK: Hong Kong and Taiwan
        behalf("hk.chop", C::HongKong, ["Company chop with signature (Hong Kong)", "Чоп компании с подписью (Гонконг)", "İmzalı şirket kaşesi (Hong Kong)"], Rd,
               "For and on behalf of", "EXAMPLE TRADING LIMITED", Some("例子貿易有限公司"), "Authorised Signature(s)", F::None),
        behalf("hk.chop.framed", C::HongKong, ["Framed company chop with signature (Hong Kong)", "Чоп компании с подписью в рамке (Гонконг)", "Çerçeveli imzalı şirket kaşesi (Hong Kong)"], B,
               "For and on behalf of", "EXAMPLE HOLDINGS LIMITED", None, "Authorized Signature(s)", F::Single),
        behalf("hk.chop.zh", C::HongKong, ["Bilingual chop with signature (Hong Kong)", "Двуязычный чоп с подписью (Гонконг)", "İki dilli imzalı kaşe (Hong Kong)"], Rd,
               "代表 For and on behalf of", "EXAMPLE LIMITED", Some("例子有限公司"), "授權簽署 Authorised Signature(s)", F::None),
        round("hk.round.small", C::HongKong, ["Small round company chop (Hong Kong)", "Малый круглый чоп (Гонконг)", "Küçük yuvarlak şirket kaşesi (Hong Kong)"], Rd,
              vec![edge("EXAMPLE TRADING LIMITED *"), centre("例子貿易\n有限公司")],
              Round { ring: Ring::Thin, double: false, centre_font: SONG_TC, small: true, ..R }),
        round("hk.round.zh", C::HongKong, ["Round chop, Chinese around (Hong Kong)", "Круглый чоп, китайский по кругу (Гонконг)", "Yuvarlak kaşe, Çince çevrede (Hong Kong)"], Rd,
              vec![edge("例子貿易有限公司 *"), centre("EXAMPLE\nTRADING\nLIMITED")],
              Round { ring: Ring::Thick, double: false, font: SONG_TC, centre_font: COND, ..R }),
        round("hk.common", C::HongKong, ["Common seal (Hong Kong)", "Общая печать (Гонконг)", "Ortak mühür (Hong Kong)"], K,
              vec![edge("THE COMMON SEAL OF EXAMPLE LIMITED * 例子有限公司 *"), centre("HONG KONG\n2026")],
              Round { ring: Ring::Rope, font: SERIF, centre_font: SERIF, ..R }),
        oval("hk.oval", C::HongKong, ["Oval company chop (Hong Kong)", "Овальный чоп (Гонконг)", "Oval şirket kaşesi (Hong Kong)"], Rd,
             vec![top("EXAMPLE TRADING LIMITED"), bottom("HONG KONG"), centre("例子貿易有限公司")], Round { centre_font: SONG_TC, ..OVAL }),
        grid("tw.big", C::HongKong, ["Company seal, large square (Taiwan)", "Большая квадратная печать компании (Тайвань)", "Büyük kare şirket mührü (Tayvan)"], Rd,
             "範例股份有限公司", g(Shape::Square, SONG_TC, 30.0, Some(4))),
        grid("tw.small", C::HongKong, ["Responsible person seal, small square (Taiwan)", "Малая квадратная личная печать (Тайвань)", "Küçük kare yetkili mührü (Tayvan)"], Rd,
             "王大明印", g(Shape::Square, SONG_TC, 22.0, Some(2))),

        // MARK: Mainland China
        round("cn.company", C::China, ["Company seal with star", "Печать компании со звездой", "Yıldızlı şirket mührü"], Rd,
              vec![edge("示例科技有限公司")], Round { ring: Ring::Thick, double: false, edge: Edge::Top, centre: Centre::Star, font: SONG, ..R }),
        round("cn.contract", C::China, ["Contract seal", "Печать для договоров", "Sözleşme mührü"], Rd,
              vec![edge("示例科技有限公司"), f("bottom", "Bottom line (optional)", "合同专用章")],
              Round { ring: Ring::Thick, double: false, edge: Edge::Top, centre: Centre::Star, font: SONG, ..R }),
        round("cn.department", C::China, ["Department seal", "Печать отдела", "Departman mührü"], Rd,
              vec![edge("示例科技有限公司"), f("bottom", "Bottom line (optional)", "人事部")],
              Round { ring: Ring::Thick, double: false, edge: Edge::Top, centre: Centre::Star, font: SONG, ..R }),
        grid("cn.finance", C::China, ["Finance seal, square", "Квадратная финансовая печать", "Kare finans mührü"], Rd,
             "示例科技有限公司财务专用章", g(Shape::Square, SONG, 26.0, Some(4))),
        oval("cn.invoice", C::China, ["Invoice seal, oval", "Овальная печать для счетов", "Oval fatura mührü"], Rd,
             vec![top("示例科技有限公司"), f("centre", "Tax number", "91110000123456789X"), bottom("发票专用章")],
             Round { ring: Ring::Thick, double: false, edge: Edge::TopBottom, font: SONG, centre_font: COND, ..R }),
        grid("cn.person", C::China, ["Legal representative seal, square", "Квадратная печать руководителя", "Kare yasal temsilci mührü"], Rd,
             "王明之印", g(Shape::Square, SONG, 20.0, Some(2))),
        word_with("cn.received", C::China, ["“Received” stamp (Chinese)", "Штамп «Получено» (китайский)", "“Alındı” kaşesi (Çince)"], Rd, "已收讫", "", "",
                  Word { frame: Frame::Double, font: SONG, ..W }),

        // MARK: Japan
        grid("jp.hanko", C::Japan, ["Personal hanko, round", "Личная печать ханко, круглая", "Kişisel hanko, yuvarlak"], Rd,
             "山田", g(Shape::Round, MINCHO, 26.0, None)),
        grid("jp.hanko.oval", C::Japan, ["Personal hanko, oval", "Личная печать ханко, овальная", "Kişisel hanko, oval"], Rd,
             "佐藤", g(Shape::Oval, MINCHO, 22.0, None)),
        grid("jp.kakuin", C::Japan, ["Company seal, square (kakuin)", "Квадратная печать компании (какуин)", "Kare şirket mührü (kakuin)"], Rd,
             "株式会社見本商事之印", g(Shape::Square, MINCHO, 30.0, Some(3))),
        round("jp.maruin", C::Japan, ["Representative seal, round (maruin)", "Круглая печать представителя (маруин)", "Temsilci mührü, yuvarlak (maruin)"], Rd,
              vec![edge("株式会社見本商事"), f("centre", "Centre text", "代表取締役印")],
              Round { ring: Ring::Thick, double: true, centre: Centre::Vertical, font: MINCHO, centre_font: MINCHO, ..R }),
        tpl("jp.date", C::Japan, ["Date stamp (nichizuke-in)", "Датированная печать (нитидзукэ-ин)", "Tarih mührü (nichizuke-in)"], Rd,
            vec![f("top", "Top line (optional)", "見本商事"), date(""), f("name", "Name", "山田")], Layout::Dater(MINCHO)),
        tpl("jp.uketsuke", C::Japan, ["“Received” date stamp (uketsuke)", "Штамп «Принято» с датой (укэцукэ)", "Tarihli “Alındı” mührü (uketsuke)"], Rd,
            vec![f("top", "Top line (optional)", "受付"), date(""), f("name", "Name", "総務部")], Layout::Dater(MINCHO)),
        rect_left("jp.address", C::Japan, ["Address stamp (Japan)", "Адресный штамп (Япония)", "Adres kaşesi (Japonya)"], K,
             vec![lines("〒100-0001 東京都千代田区千代田1-1"), name("株式会社見本商事"), numbers("代表取締役　山田太郎\nTEL 03-0000-0000")],
             vec![t("lines", 52.0, MINCHO), t("name", 86.0, MINCHO), t("numbers", 52.0, MINCHO)]),
        grid("jp.mitome", C::Japan, ["Small square seal (mitome)", "Малая квадратная печать (митомэ)", "Küçük kare mühür (mitome)"], Rd,
             "鈴木", g(Shape::Square, MINCHO, 18.0, None)),

        // MARK: Korea
        grid("kr.dojang", C::Korea, ["Name seal, round (dojang)", "Именная печать, круглая (доджан)", "İsim mührü, yuvarlak (dojang)"], Rd,
             "김민준", g(Shape::Round, HANGUL, 24.0, None)),
        grid("kr.dojang.square", C::Korea, ["Name seal, square (dojang)", "Именная печать, квадратная (доджан)", "İsim mührü, kare (dojang)"], Rd,
             "이서연인", g(Shape::Square, HANGUL, 22.0, Some(2))),
        grid("kr.company", C::Korea, ["Company seal, square (jigin)", "Квадратная печать компании (чжигин)", "Kare şirket mührü (jigin)"], Rd,
             "주식회사예시상사인", g(Shape::Square, HANGUL, 30.0, Some(3))),
        round("kr.round", C::Korea, ["Representative seal, round", "Круглая печать представителя", "Temsilci mührü, yuvarlak"], Rd,
              vec![edge("주식회사 예시상사"), f("centre", "Centre text", "대표이사인")],
              Round { ring: Ring::Thick, double: true, centre: Centre::Vertical, font: HANGUL_SANS, centre_font: HANGUL, ..R }),

        // MARK: India and South Asia
        behalf("in.authorised", C::SouthAsia, ["“For … Authorised Signatory” stamp", "Штамп «For … Authorised Signatory»", "“For … Authorised Signatory” kaşesi"], B,
               "For", "EXAMPLE TECHNOLOGIES PVT. LTD.", None, "Authorised Signatory", F::None),
        behalf("in.proprietor", C::SouthAsia, ["“Proprietor” stamp", "Штамп «Proprietor» (владелец)", "“Proprietor” kaşesi"], V,
               "For", "SHARMA TRADERS", None, "Proprietor", F::None),
        round("in.round", C::SouthAsia, ["Round company seal (India)", "Круглая печать (Индия)", "Yuvarlak şirket mührü (Hindistan)"], B,
              vec![edge("EXAMPLE TECHNOLOGIES PVT. LTD. * MUMBAI *"), centre("★")], Round { ring: Ring::Solid, centre: Centre::Lines, ..R }),
        company("in.address", C::SouthAsia, ["Company stamp with GSTIN", "Штамп компании с GSTIN", "GSTIN'li şirket kaşesi"], B,
                "Example Technologies Pvt. Ltd.", "12, M.G. Road, Bengaluru 560001", "GSTIN 29ABCDE1234F1Z5", F::Single, false, false),
        rect_framed("in.received", C::SouthAsia, ["“Received” stamp with sign", "Штамп «Received» с подписью", "İmzalı “Received” kaşesi"], B,
             vec![wordf("RECEIVED"), date("Date: ____________"), f("sign", "Signature line", "Sign: ____________")],
             vec![t("word", 100.0, BLACK), t("date", 56.0, m), t("sign", 56.0, m)], F::Double),

        // MARK: Middle East
        round("ae.round", C::MiddleEast, ["Round seal, Arabic and English", "Круглая печать, арабский и английский", "Yuvarlak mühür, Arapça ve İngilizce"], B,
              vec![top("EXAMPLE TRADING L.L.C."), bottom("DUBAI - U.A.E."), f("centre", "Name in Arabic", "مثال للتجارة\nش.ذ.م.م")],
              Round { edge: Edge::TopBottom, centre_font: ARABIC, ..R }),
        rect_framed("ae.rect", C::MiddleEast, ["Company stamp, Arabic and English", "Штамп компании, арабский и английский", "Şirket kaşesi, Arapça ve İngilizce"], B,
             vec![f("local", "Name in Arabic", "مثال للتجارة ش.ذ.م.م"), name("EXAMPLE TRADING L.L.C."), numbers("P.O. Box 12345, Dubai · TRN 100000000000003")],
             vec![t("local", 80.0, ARABIC), tu("name", 70.0, SANS), t("numbers", 48.0, m)], F::Single),
        round("sa.round", C::MiddleEast, ["Round company seal (Saudi Arabia)", "Круглая печать (Саудовская Аравия)", "Yuvarlak şirket mührü (Suudi Arabistan)"], G,
              vec![top("EXAMPLE COMPANY LTD."), bottom("C.R. 1010000000 · RIYADH"), f("centre", "Name in Arabic", "شركة مثال\nالمحدودة")],
              Round { ring: Ring::Dotted, edge: Edge::TopBottom, centre_font: ARABIC, ..R }),
        rect("il.stamp", C::MiddleEast, ["Company stamp (Israel)", "Штамп компании (Израиль)", "Şirket kaşesi (İsrail)"], B,
             vec![name("דוגמה בע״מ"), lines("רחוב הרצל 1, תל אביב"), numbers("ח.פ. 512345678")],
             vec![t("name", 88.0, HEBREW), t("lines", 60.0, HEBREW), t("numbers", 56.0, HEBREW)]),
        round("eg.round", C::MiddleEast, ["Round company seal (Egypt)", "Круглая печать (Египет)", "Yuvarlak şirket mührü (Mısır)"], B,
              vec![top("EXAMPLE FOR TRADING S.A.E."), bottom("CAIRO"), f("centre", "Name in Arabic", "مثال للتجارة\nش.م.م")],
              Round { ring: Ring::Rope, edge: Edge::TopBottom, centre_font: ARABIC, ..R }),

        // MARK: Latin America
        company("mx.sello", C::LatinAmerica, ["Company stamp (Mexico)", "Штамп компании (Мексика)", "Şirket kaşesi (Meksika)"], B,
                "EJEMPLO S.A. DE C.V.", "Av. Reforma 1, Col. Juárez, CDMX", "RFC EJE010101ABC", F::None, false, true),
        rect_framed("br.cnpj", C::LatinAmerica, ["CNPJ stamp (Brazil)", "Штамп CNPJ (Бразилия)", "CNPJ kaşesi (Brezilya)"], B,
             vec![numbers("12.345.678/0001-90"), name("EXEMPLO COMÉRCIO LTDA."), lines("Av. Paulista, 1000 – Bela Vista\nCEP 01310-100 – São Paulo – SP")],
             vec![t("numbers", 86.0, SANS), tu("name", 66.0, SANS), t("lines", 50.0, m)], F::Thick),
        oval("ar.sello", C::LatinAmerica, ["Oval stamp (Argentina)", "Овальный штамп (Аргентина)", "Oval kaşe (Arjantin)"], V,
             vec![top("EJEMPLO S.R.L."), bottom("BUENOS AIRES"), centre("CUIT\n30-12345678-9")], OVAL),
        round("latam.round", C::LatinAmerica, ["Round company seal (Latin America)", "Круглая печать (Латинская Америка)", "Yuvarlak şirket mührü (Latin Amerika)"], B,
              vec![top("EJEMPLO S.A.C."), bottom("LIMA - PERÚ"), centre("RUC\n20123456789")], Round { edge: Edge::TopBottom, ..R }),
        word("br.recebido", C::LatinAmerica, ["“Recebido” stamp (Brazil)", "Штамп «Recebido» (Бразилия)", "“Recebido” kaşesi (Brezilya)"], B, "RECEBIDO"),
        word("es.pagado", C::LatinAmerica, ["“Pagado” stamp", "Штамп «Pagado» (оплачено)", "“Pagado” kaşesi"], Rd, "PAGADO"),

        // MARK: Office stamps
        word_top("office.approved", C::Office, ["Approved", "Approved (утверждено)", "Approved (onaylandı)"], Rd, "APPROVED", "EXAMPLE LTD"),
        word("office.paid", C::Office, ["Paid", "Paid (оплачено)", "Paid (ödendi)"], Rd, "PAID"),
        word("office.received", C::Office, ["Received", "Received (получено)", "Received (alındı)"], B, "RECEIVED"),
        word_nodate("office.copy", C::Office, ["Copy", "Copy (копия)", "Copy (kopya)"], Rd, "COPY"),
        word_nodate("office.original", C::Office, ["Original", "Original (оригинал)", "Original (asıl)"], B, "ORIGINAL"),
        word_nodate("office.confidential", C::Office, ["Confidential", "Confidential (конфиденциально)", "Confidential (gizli)"], Rd, "CONFIDENTIAL"),
        word_nodate("office.urgent", C::Office, ["Urgent", "Urgent (срочно)", "Urgent (acil)"], Rd, "URGENT"),
        word_nodate("office.draft", C::Office, ["Draft", "Draft (черновик)", "Draft (taslak)"], B, "DRAFT"),
        word("office.rejected", C::Office, ["Rejected", "Rejected (отклонено)", "Rejected (reddedildi)"], Rd, "REJECTED"),
        word_nodate("office.void", C::Office, ["Void", "Void (аннулировано)", "Void (geçersiz)"], Rd, "VOID"),
        word("office.cancelled", C::Office, ["Cancelled", "Cancelled (отменено)", "Cancelled (iptal)"], Rd, "CANCELLED"),
        word("office.posted", C::Office, ["Posted", "Posted (проведено)", "Posted (işlendi)"], B, "POSTED"),
        word("office.entered", C::Office, ["Entered", "Entered (внесено)", "Entered (girildi)"], G, "ENTERED"),
        word("office.checked", C::Office, ["Checked", "Checked (проверено)", "Checked (kontrol edildi)"], G, "CHECKED"),
        word("office.faxed", C::Office, ["Faxed", "Faxed (отправлено по факсу)", "Faxed (fakslandı)"], B, "FAXED"),
        word("office.scanned", C::Office, ["Scanned", "Scanned (отсканировано)", "Scanned (tarandı)"], B, "SCANNED"),
        word("office.completed", C::Office, ["Completed", "Completed (выполнено)", "Completed (tamamlandı)"], G, "COMPLETED"),
        word_nodate("office.sample", C::Office, ["Sample", "Sample (образец)", "Sample (numune)"], Rd, "SAMPLE"),
        word_nodate("office.ru.copy", C::Office, ["“Копия” (Russian)", "«Копия»", "“Копия” (Rusça)"], B, "КОПИЯ"),
        word_nodate("office.ru.urgent", C::Office, ["“Срочно” (Russian)", "«Срочно»", "“Срочно” (Rusça)"], Rd, "СРОЧНО"),
        word("office.tr.onay", C::Office, ["“Onaylandı” (Turkish)", "«Onaylandı» (утверждено)", "“Onaylandı”"], Rd, "ONAYLANDI"),
        word_nodate("office.tr.gizli", C::Office, ["“Gizli” (Turkish)", "«Gizli» (конфиденциально)", "“Gizli”"], Rd, "GİZLİ"),

        // MARK: Date and registration stamps
        round("date.round", C::Dates, ["Round date stamp", "Круглый датер", "Yuvarlak tarih kaşesi"], B,
              vec![top("EXAMPLE TRADING LTD"), bottom("RECEIVED"), f("centre", "Date or note (empty: today)", "")],
              Round { edge: Edge::TopBottom, centre: Centre::Lines, centre_font: MONO, ..R }),
        rect_framed("date.rect", C::Dates, ["Received date stamp with reference", "Штамп получения с датой и номером", "Tarih ve numaralı alındı kaşesi"], B,
             vec![name("EXAMPLE TRADING LTD"), wordf("RECEIVED"), date(""), f("ref", "Reference line", "Ref. No. ____________")],
             vec![t("name", 50.0, SANS), t("word", 84.0, BLACK), t("date", 74.0, MONO), t("ref", 50.0, m)], F::Rounded),
        tpl("date.table", C::Dates, ["Approval table", "Таблица согласования", "Onay tablosu"], B,
            vec![f("top", "Top line (optional)", "APPROVAL"), fm("rows", "Rows", "Prepared by\nChecked by\nApproved by")], Layout::Table),
        tpl("date.table.ru", C::Dates, ["Registration table (Russian)", "Регистрационная таблица", "Kayıt tablosu (Rusça)"], V,
            vec![f("top", "Top line (optional)", "ООО «РОМАШКА»"), fm("rows", "Rows", "Вх. №\nДата\nПодпись")], Layout::Table),
        round("date.qc", C::Dates, ["Round inspection stamp", "Круглый штамп контроля", "Yuvarlak muayene kaşesi"], G,
              vec![top("QUALITY CONTROL"), bottom("INSPECTOR"), centre("PASSED\n07")], Round { ring: Ring::Thin, double: true, edge: Edge::TopBottom, ..R }),

        // MARK: Personal stamps
        rect_left("personal.name", C::Personal, ["Name and position", "Имя и должность", "Ad ve unvan"], B,
             vec![name("Ivan Petrov"), f("position", "Position", "Managing Director")],
             vec![t("name", 92.0, SANS), t("position", 62.0, m)]),
        rect_left("personal.address", C::Personal, ["Return address", "Обратный адрес", "Dönüş adresi"], K,
             vec![name("Anna Schmidt"), lines("Lindenweg 7\n12345 Musterstadt")], vec![t("name", 76.0, SANS), t("lines", 60.0, m)]),
        round("personal.monogram", C::Personal, ["Round monogram", "Круглая монограмма", "Yuvarlak monogram"], B,
              vec![edge("IVAN PETROV * MOSCOW *"), f("centre", "Initials", "IP")],
              Round { ring: Ring::Dotted, double: false, centre: Centre::Initials, centre_font: SERIF, ..R }),
        round("personal.exlibris", C::Personal, ["Book stamp (ex libris)", "Книжный штамп (экслибрис)", "Kitap kaşesi (ex libris)"], K,
              vec![top("EX LIBRIS"), bottom("ANNA SCHMIDT"), centre("❦")],
              Round { ring: Ring::Rope, edge: Edge::TopBottom, font: SERIF, centre_font: SERIF, ..R }),
        grid("personal.seal.cjk", C::Personal, ["Name seal, square (any script)", "Именная квадратная печать (любой алфавит)", "Kare isim mührü (herhangi bir yazı)"], Rd,
             "ANNA", g(Shape::Square, SERIF, 22.0, Some(2))),
    ]
}
