"""Sample documents (fictional companies) for OFF42 website screenshots: a signed letter (DOCX),
a budget with a chart (XLSX) and a short deck (PPTX), in English and Russian."""
import sys
from pathlib import Path
from docx import Document
from docx.shared import Pt, Cm, RGBColor
from docx.enum.text import WD_ALIGN_PARAGRAPH
from openpyxl import Workbook
from openpyxl.chart import BarChart, LineChart, Reference
from openpyxl.styles import Font, PatternFill, Alignment, Border, Side
from pptx import Presentation
from pptx.util import Inches, Pt as PPt, Emu
from pptx.dml.color import RGBColor as PRGB
from pptx.enum.shapes import MSO_SHAPE
from pptx.enum.text import PP_ALIGN
from docx.oxml.ns import qn
RTL = False

out = Path(sys.argv[1]); docs = Path(sys.argv[2]); hero = Path(sys.argv[3])
GREEN = RGBColor(0x2F, 0x44, 0x35); WARM = RGBColor(0xBD, 0x41, 0x25)

import json
samples = Path(sys.argv[4]); langs = sys.argv[5:] or sorted(p.stem for p in samples.glob('*.json'))
def load(lang):
    c = json.loads((samples / f'{lang}.json').read_text())
    L, B, D = c['letter'], c['budget'], c['deck']
    return dict(rtl=c['rtl'], brand=L['brand'], addr=L['addr'], date=L['date'], to=L['to'], subject=L['subject'], body=L['body'],
                table=[tuple(r) for r in L['table']], close=L['close'], name=L['name'], role=L['role'],
                sheet=B['sheet'], months=B['months'], rows=[(r[0], r[1]) for r in B['rows']], total=B['total'], head=B['head'], share=B['share'],
                deck=[(D['title'], D['subtitle']), (D['ideasTitle'], D['ideasSubtitle']), (D['timelineTitle'], '')],
                steps=[tuple(x) for x in D['steps']], ideas=[tuple(x) for x in D['ideas']])

def letter(lang, t):
    d = Document()
    sec = d.sections[0]; sec.page_width, sec.page_height = Cm(21), Cm(29.7)
    sec.left_margin = sec.right_margin = Cm(2.2); sec.top_margin = Cm(1.8)
    st = d.styles['Normal']; st.font.name = 'Helvetica Neue'; st.font.size = Pt(10.5)
    p = d.add_paragraph(); r = p.add_run(t['brand']); r.bold = True; r.font.size = Pt(16); r.font.color.rgb = GREEN
    p = d.add_paragraph(); r = p.add_run(t['addr']); r.font.size = Pt(8.5); r.font.color.rgb = RGBColor(0x66, 0x66, 0x66)
    p = d.add_paragraph(t['date']); p.alignment = WD_ALIGN_PARAGRAPH.RIGHT
    for line in t['to']: d.add_paragraph(line).paragraph_format.space_after = Pt(0)
    p = d.add_paragraph(); p.paragraph_format.space_before = Pt(14)
    r = p.add_run(t['subject']); r.bold = True; r.font.size = Pt(13); r.font.color.rgb = WARM
    for line in t['body']: d.add_paragraph(line).paragraph_format.space_after = Pt(8)
    tb = d.add_table(rows=len(t['table']), cols=3); tb.style = 'Light Grid Accent 1'
    for i, row in enumerate(t['table']):
        for j, v in enumerate(row):
            c = tb.cell(i, j); c.text = v
            if i == 0: c.paragraphs[0].runs[0].bold = True
            if j: c.paragraphs[0].alignment = WD_ALIGN_PARAGRAPH.RIGHT
    d.add_paragraph()
    for line in t['close']: d.add_paragraph(line).paragraph_format.space_after = Pt(6)
    p = d.add_paragraph(); run = p.add_run()
    from PIL import Image as _I
    def wide(f): im = _I.open(f); return im.width / im.height
    sig = docs / f'signature-{lang}.png'
    run.add_picture(str(sig), width=Cm(4.2 if wide(sig) > 1.6 else 2.0))
    run.add_text('    '); st = docs / f'stamp-{lang}.png'; run.add_picture(str(st), width=Cm(4.6 if wide(st) > 1.4 else 3.2))
    p = d.add_paragraph(); r = p.add_run(t['name']); r.bold = True
    d.add_paragraph(t['role'])
    if t['rtl']:
        for par in d.paragraphs:
            par.alignment = WD_ALIGN_PARAGRAPH.RIGHT
            pPr = par._p.get_or_add_pPr(); el = pPr.makeelement(qn('w:bidi'), {}); pPr.append(el)
    d.save(out / f'letter-{lang}.docx')

def budget(lang, t):
    wb = Workbook(); ws = wb.active; ws.title = t['sheet']
    ws['A1'] = t['head']; ws['A1'].font = Font(bold=True, size=14, color='2F4435')
    head = [''] + t['months'] + [t['total'], t['share']]
    for j, v in enumerate(head):
        c = ws.cell(row=3, column=j + 1, value=v); c.font = Font(bold=True, color='FFFFFF')
        c.fill = PatternFill('solid', fgColor='2F4435'); c.alignment = Alignment(horizontal='center')
    n = len(t['rows'])
    for i, (name, vals) in enumerate(t['rows']):
        r = 4 + i; ws.cell(row=r, column=1, value=name).font = Font(bold=True)
        for j, v in enumerate(vals): ws.cell(row=r, column=2 + j, value=v).number_format = '#,##0'
        ws.cell(row=r, column=8, value=f'=SUM(B{r}:G{r})').number_format = '#,##0'
        ws.cell(row=r, column=9, value=f'=H{r}/H{4 + n}').number_format = '0.0%'
        if i % 2:
            for j in range(1, 10): ws.cell(row=r, column=j).fill = PatternFill('solid', fgColor='EEF0E8')
    tr = 4 + n; ws.cell(row=tr, column=1, value=t['total']).font = Font(bold=True)
    for j in range(2, 9):
        col = chr(64 + j); c = ws.cell(row=tr, column=j, value=f'=SUM({col}4:{col}{tr - 1})')
        c.number_format = '#,##0'; c.font = Font(bold=True); c.border = Border(top=Side(style='thin'))
    ws.cell(row=tr, column=9, value=f'=H{tr}/H{tr}').number_format = '0.0%'
    ws.column_dimensions['A'].width = 20
    for col in 'BCDEFGHI': ws.column_dimensions[col].width = 11
    ch = BarChart(); ch.type = 'col'; ch.grouping = 'stacked'; ch.overlap = 100; ch.title = t['head']
    ch.add_data(Reference(ws, min_col=1, max_col=7, min_row=4, max_row=3 + n), from_rows=True, titles_from_data=True)
    ch.set_categories(Reference(ws, min_col=2, max_col=7, min_row=3)); ch.height, ch.width = 8, 17
    ws.add_chart(ch, 'B11')
    ws.sheet_view.rightToLeft = t['rtl']
    wb.save(out / f'budget-{lang}.xlsx')

def deck(lang, t):
    global RTL; RTL = t['rtl']
    pr = Presentation(); pr.slide_width, pr.slide_height = Inches(13.333), Inches(7.5)
    blank = pr.slide_layouts[6]
    def bg(s, color):
        f = s.background.fill; f.solid(); f.fore_color.rgb = color
    def box(s, text, x, y, w, h, size, bold=False, color=PRGB(0x25, 0x2A, 0x26)):
        tb = s.shapes.add_textbox(Inches(x), Inches(y), Inches(w), Inches(h)); tf = tb.text_frame; tf.word_wrap = True
        p = tf.paragraphs[0]; r = p.add_run(); r.text = text; r.font.size = PPt(size); r.font.bold = bold; r.font.color.rgb = color
        r.font.name = 'Helvetica Neue'
        if RTL: p.alignment = PP_ALIGN.RIGHT
        return tb
    # 1: title with picture
    s = pr.slides.add_slide(blank); bg(s, PRGB(0x2F, 0x44, 0x35))
    s.shapes.add_picture(str(hero), Inches(6.4), Inches(0), height=Inches(7.5))
    box(s, t['deck'][0][0], 0.8, 2.6, 5.4, 1.4, 44, True, PRGB(0xF7, 0xF4, 0xE8))
    box(s, t['deck'][0][1], 0.8, 4.0, 5.4, 1, 18, False, PRGB(0xE9, 0xA1, 0x84))
    # 2: three ideas
    s = pr.slides.add_slide(blank); bg(s, PRGB(0xF7, 0xF6, 0xF2))
    box(s, t['deck'][1][0], 0.8, 0.6, 11, 1, 36, True)
    box(s, t['deck'][1][1], 0.8, 1.45, 11, 0.6, 16, False, PRGB(0x60, 0x67, 0x5E))
    colors = [PRGB(0xBD, 0x41, 0x25), PRGB(0x2F, 0x44, 0x35), PRGB(0xA4, 0x64, 0x47)]
    for i, (h, d) in enumerate(t['ideas']):
        x = 0.8 + i * 4.05
        card = s.shapes.add_shape(MSO_SHAPE.ROUNDED_RECTANGLE, Inches(x), Inches(2.5), Inches(3.7), Inches(4.0))
        card.fill.solid(); card.fill.fore_color.rgb = PRGB(0xFF, 0xFF, 0xFF); card.line.color.rgb = PRGB(0xE3, 0xE3, 0xDA)
        dot = s.shapes.add_shape(MSO_SHAPE.OVAL, Inches(x + 0.4), Inches(2.9), Inches(0.8), Inches(0.8))
        dot.fill.solid(); dot.fill.fore_color.rgb = colors[i]; dot.line.fill.background()
        box(s, h, x + 0.4, 4.0, 3.0, 0.7, 24, True)
        box(s, d, x + 0.4, 4.8, 3.0, 1.5, 14, False, PRGB(0x4D, 0x55, 0x4B))
    # 3: timeline
    s = pr.slides.add_slide(blank); bg(s, PRGB(0xF7, 0xF6, 0xF2))
    box(s, t['deck'][2][0], 0.8, 0.6, 11, 1, 36, True)
    line = s.shapes.add_shape(MSO_SHAPE.RECTANGLE, Inches(1.2), Inches(3.9), Inches(10.9), Inches(0.06))
    line.fill.solid(); line.fill.fore_color.rgb = PRGB(0xC9, 0xCE, 0xC2); line.line.fill.background()
    for i, (m, label) in enumerate(t['steps']):
        x = 1.2 + i * 3.4
        dot = s.shapes.add_shape(MSO_SHAPE.OVAL, Inches(x - 0.25), Inches(3.68), Inches(0.5), Inches(0.5))
        dot.fill.solid(); dot.fill.fore_color.rgb = colors[i % 3]; dot.line.fill.background()
        box(s, m, x - 0.4, 2.7, 2.5, 0.7, 26, True, colors[i % 3])
        box(s, label, x - 0.4, 4.5, 3.0, 0.7, 18)
    pr.save(out / f'deck-{lang}.pptx')

out.mkdir(parents=True, exist_ok=True)
for lang in langs:
    t = load(lang); letter(lang, t); budget(lang, t); deck(lang, t)
print('ok', sorted(p.name for p in out.iterdir()))
