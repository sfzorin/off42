# Sample documents per language — brief for translators

Each `<lang>.json` here drives sample documents shown in OFF42 website screenshots: a signed design-services
agreement + invoice (PDF), a proposal letter (DOCX), a half-year budget with chart (XLSX) and a 3-slide deck (PPTX).
`en.json` and `ru.json` are the finished references — copy their structure EXACTLY (same keys, same array lengths,
same nesting; numbers stay numbers). Only values change.

## Rules
- Localise, don't just translate: invent plausible FICTIONAL companies, people, streets and registration numbers for
  the country of the language (a design studio and a media company, as in en/ru). Never use real companies or
  famous people. Use the local currency, number format, date format and legal forms (GmbH, SARL, S.L., Sp. z o.o.,
  Ltd. Şti., PT, 株式会社, 有限公司, 주식회사, Công ty TNHH, บริษัท … จำกัด, Sdn. Bhd., Inc., ش.ذ.م.م …).
- Keep lengths close to the English (the page layout is fixed). Fee/item names ≤ 32 characters; party `title` short.
- `budget.rows`: 4 rows × 6 numbers, realistic for the currency (e.g. JPY in thousands, VND in millions is fine — say
  the unit in `budget.head`). `fees`/`items`/`total`/`invoiceTotal` amounts must be consistent (total = sum).
- `rtl`: true only for Arabic.
- `signName`: the name the signature generator writes. It only draws Latin and Cyrillic letters well, so:
  - Latin-script languages: the person's own name.
  - Arabic, Thai: the person's name transliterated to Latin (many people sign in Latin script) — e.g. "Omar Haddad".
  - zh-Hans, zh-Hant, ja, ko: "" (empty) — these parties seal instead of signing; then `seal2` is REQUIRED (see below).
  Keep `design` exactly as given below; `pen` one of ballpoint|gel|fountain|felt.
- `generatorName`: a Latin-script name of the first party's person (for zh/ja/ko/ar/th use a romanised name).
- Stamps (`stamp`, `seal2`, `paid`): use exactly the template `id` and value keys listed for your language; put the
  fictional company data in the values (keep `\n` line breaks where shown). `width` as given.

## Per language: stamp templates and designs
Value keys by template: company-style (de.firm, at.firm, fr.tampon, es.rect, it.timbro, pt.carimbo, pl.pieczatka,
uk.company, tr.kase): name, lines, numbers. Round/oval (es.sello, it.round, eu.round, us.corporate, tr.round,
hk.oval, ae.round, sa.round): edge (top arc), bottom (bottom arc; eu.round has none), centre.
br.cnpj: numbers (CNPJ), name, lines. Word stamps (us.paid, es.pagado, tr.odendi, cn.received): top (small line,
company), word (the big word). cn.company: edge. cn.contract: edge, bottom (e.g. 合同专用章). hk.round.zh: edge,
centre. cn.person / jp.hanko / jp.kakuin: chars (characters of the seal, e.g. "王明之印", "田中", "株式会社青葉デザイン之印").
jp.maruin / kr.round: edge, centre.

| lang | party 1 stamp (width) | party 2 stamp (width) | seal2 (both parties) | paid stamp (word) | designs p1/p2 |
|---|---|---|---|---|---|
| de | de.firm (145) | at.firm (145) | – | us.paid "BEZAHLT" (150) | 2424853 / 2425117 |
| fr | fr.tampon (145) | eu.round (104) | – | us.paid "PAYÉ" (150) | 2424853 / 2425381 |
| es | es.sello (120) | es.rect (145) | – | es.pagado "PAGADO" (150) | 2424853 / 2425117 |
| it | it.timbro (145) | it.round (104) | – | us.paid "PAGATO" (150) | 2424853 / 2425381 |
| pt (Brazil) | br.cnpj (150) | br.cnpj (150) | – | us.paid "PAGO" (150) | 2424853 / 2425117 |
| pl | pl.pieczatka (150) | pl.pieczatka (150) | – | us.paid "ZAPŁACONO" (160) | 2424853 / 2425381 |
| hu | pl.pieczatka (150) | eu.round (104) | – | us.paid "FIZETVE" (150) | 2424853 / 2425117 |
| tr | tr.kase (150) | tr.round (104) | – | tr.odendi "ÖDENDİ" (150) | 2424853 / 2425381 |
| id | eu.round (104) | pl.pieczatka (150) | – | us.paid "LUNAS" (150) | 2424853 / 2425117 |
| vi | cn.company (104) | cn.company (104) | – | us.paid "ĐÃ THANH TOÁN" (170) | 2424853 / 2425381 |
| th | eu.round (104) | pl.pieczatka (150) | – | us.paid "ชำระแล้ว" (150) | 2424853 / 2425117 |
| ar (rtl) | ae.round (110) | sa.round (110) | – | us.paid "مدفوع" (150) | 2424853 / 2425381 |
| fil | us.corporate (104) | uk.company (150) | – | us.paid "BAYAD NA" (150) | 2424853 / 2425117 |
| ms | uk.company (150) | eu.round (104) | – | us.paid "TELAH DIBAYAR" (170) | 2424853 / 2425381 |
| zh-Hans | cn.contract (104) | cn.company (104) | cn.person (56) | cn.received "已付款" (150) | 0 / 0 |
| zh-Hant | hk.round.zh (104) | hk.oval (120) | cn.person (56, traditional chars) | cn.received "已付款" (150) | 0 / 0 |
| ja | jp.kakuin (80) | jp.kakuin (80) | jp.hanko (50) | us.paid "支払済" (150) | 0 / 0 |
| ko | kr.round (90) | kr.round (90) | kr.round (60; edge = person's name, centre "인") | us.paid "결제 완료" (160) | 0 / 0 |

Note: a stamp width < 120 is treated as round (placed over the date line); ≥ 120 as rectangular (placed beside the
signature). For zh-Hant hk.oval (120) is fine.
