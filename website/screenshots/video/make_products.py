import sys, openpyxl
from openpyxl.styles import Font, PatternFill, Alignment
data = {
 'en': (['Product','SKU','Batch','Product page'],[
  ('Desk lamp, 12 V LED','NL-24017','B-2611','https://example.com/p/NL-24017'),
  ('Power adapter, 24 W','NL-24022','B-2611','https://example.com/p/NL-24022'),
  ('USB-C hub, 7 ports','NL-24031','B-2612','https://example.com/p/NL-24031'),
  ('Wall charger, 65 W','NL-24046','B-2612','https://example.com/p/NL-24046'),
  ('Extension cord, 3 m','NL-24058','B-2613','https://example.com/p/NL-24058'),
  ('Night light, sensor','NL-24063','B-2613','https://example.com/p/NL-24063'),
  ('Cooling fan, 120 mm','NL-24079','B-2614','https://example.com/p/NL-24079'),
  ('Soldering station','NL-24085','B-2614','https://example.com/p/NL-24085'),
  ('LED strip driver','NL-24092','B-2615','https://example.com/p/NL-24092'),
  ('Bench power supply','NL-24104','B-2615','https://example.com/p/NL-24104')]),
 'ru': (['Товар','Артикул','Партия','Страница товара'],[
  ('Настольная лампа, 12 В','NL-24017','B-2611','https://example.com/p/NL-24017'),
  ('Блок питания, 24 Вт','NL-24022','B-2611','https://example.com/p/NL-24022'),
  ('USB-C хаб, 7 портов','NL-24031','B-2612','https://example.com/p/NL-24031'),
  ('Зарядное устройство, 65 Вт','NL-24046','B-2612','https://example.com/p/NL-24046'),
  ('Удлинитель, 3 м','NL-24058','B-2613','https://example.com/p/NL-24058'),
  ('Ночник с датчиком','NL-24063','B-2613','https://example.com/p/NL-24063'),
  ('Вентилятор, 120 мм','NL-24079','B-2614','https://example.com/p/NL-24079'),
  ('Паяльная станция','NL-24085','B-2614','https://example.com/p/NL-24085'),
  ('Драйвер LED-ленты','NL-24092','B-2615','https://example.com/p/NL-24092'),
  ('Лабораторный блок питания','NL-24104','B-2615','https://example.com/p/NL-24104')]),
}
for lang in sys.argv[2:]:
    head, rows = data[lang]
    wb = openpyxl.Workbook(); ws = wb.active; ws.title = 'Products' if lang=='en' else 'Товары'
    ws.append(head)
    for c in ws[1]: c.font = Font(bold=True, color='FFFFFF'); c.fill = PatternFill('solid', fgColor='2F5D50')
    for r in rows: ws.append(r)
    for col, w in zip('ABCD', (30, 13, 10, 36)): ws.column_dimensions[col].width = w
    wb.save(f'{sys.argv[1]}/products-{lang}.xlsx')
