#!/usr/bin/env python3
"""Drafts of the Mac App Store listings for the six apps in the 20 site languages.

Usage: python3 appstore/mac/make_listings.py      (writes appstore/mac/listings/<language>.json)

Subtitle, description and promotional text are assembled from the website's translated strings
(website/locales/<language>.json), so they say what the site already says, in the same words. Keywords are
written here; they avoid other companies' product names, which App Review rejects. Every field is checked
against the App Store Connect limits; anything over a limit is listed under "check" for the owner.
"""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent / "listings"

PRIVACY_URL = "https://off42.cc/{lang}/privacy/"
SUPPORT_URL = "https://off42.cc/{lang}/support/"
MARKETING_URL = "https://off42.cc/{lang}/"

# Site language → App Store Connect localization. Filipino is not offered by App Store Connect.
STORE_LOCALE = {
    "en": "en-US", "zh-Hans": "zh-Hans", "zh-Hant": "zh-Hant", "es": "es-ES", "ar": "ar-SA", "fr": "fr-FR",
    "pt": "pt-BR", "id": "id", "ru": "ru", "de": "de-DE", "ja": "ja", "vi": "vi", "tr": "tr", "fil": None,
    "ko": "ko", "th": "th", "it": "it", "pl": "pl", "ms": "ms", "hu": "hu",
}

APPS = {  # app → (site app id, feature pages for the description)
    "PDF42": ("pdf", ["edit-pdf", "sign-pdf"]),
    "DOC42": ("doc", ["documents"]),
    "XLS42": ("xls", ["spreadsheets", "labels-codes"]),
    "PPT42": ("ppt", ["presentations"]),
    "IMG42": ("img", ["image-editor"]),
    "SCN42": ("scn", ["scan-ocr"]),
}

LIMITS = {"name": 30, "subtitle": 30, "keywords": 100, "promotionalText": 170, "description": 4000}

# Keywords (comma-separated, no spaces after commas; the app name and subtitle words count already).
KEYWORDS = {
    "en": {
        "PDF42": "pdf,editor,annotate,sign,signature,stamp,compress,merge,pages,form,highlight,reader",
        "DOC42": "word processor,docx,odt,rtf,markdown,writer,letter,document,text,office",
        "XLS42": "spreadsheet,xlsx,ods,csv,formula,chart,table,budget,calc,labels,qr code,barcode",
        "PPT42": "presentation,slides,pptx,odp,slideshow,deck,speaker notes,pitch,office",
        "IMG42": "image editor,photo,crop,resize,annotate,markup,heic,jpeg,png,convert,viewer",
        "SCN42": "scanner,scan,ocr,document scanner,pdf,text recognition,continuity camera,deskew,receipt",
    },
    "ru": {
        "PDF42": "pdf,редактор,аннотации,подпись,эцп,печать,сжать,объединить,страницы,форма,просмотр",
        "DOC42": "текстовый редактор,docx,odt,rtf,markdown,документ,письмо,текст,офис",
        "XLS42": "таблица,электронная таблица,xlsx,ods,csv,формулы,диаграмма,бюджет,этикетки,qr код",
        "PPT42": "презентация,слайды,pptx,odp,слайд-шоу,заметки докладчика,офис",
        "IMG42": "редактор фото,изображение,обрезка,размер,разметка,heic,jpeg,png,конвертер",
        "SCN42": "сканер,сканирование,ocr,распознавание текста,pdf,документ,камера,чек",
    },
    "tr": {
        "PDF42": "pdf,düzenleyici,açıklama,imza,e-imza,mühür,sıkıştır,birleştir,sayfa,form,okuyucu",
        "DOC42": "kelime işlemci,docx,odt,rtf,markdown,belge,mektup,metin,ofis",
        "XLS42": "elektronik tablo,xlsx,ods,csv,formül,grafik,tablo,bütçe,etiket,qr kod,barkod",
        "PPT42": "sunum,slayt,pptx,odp,slayt gösterisi,konuşmacı notları,ofis",
        "IMG42": "resim düzenleyici,fotoğraf,kırp,boyutlandır,işaretleme,heic,jpeg,png,dönüştür",
        "SCN42": "tarayıcı,tarama,ocr,metin tanıma,pdf,belge tarayıcı,kamera,fiş",
    },
    "de": {
        "PDF42": "pdf,editor,kommentieren,unterschreiben,signatur,stempel,komprimieren,zusammenfügen,formular",
        "DOC42": "textverarbeitung,docx,odt,rtf,markdown,dokument,brief,text,büro",
        "XLS42": "tabellenkalkulation,xlsx,ods,csv,formel,diagramm,tabelle,budget,etiketten,qr-code",
        "PPT42": "präsentation,folien,pptx,odp,diashow,referentennotizen,büro",
        "IMG42": "bildbearbeitung,foto,zuschneiden,skalieren,markieren,heic,jpeg,png,konvertieren",
        "SCN42": "scanner,scannen,ocr,texterkennung,pdf,dokumentenscanner,kamera,beleg",
    },
    "fr": {
        "PDF42": "pdf,éditeur,annoter,signer,signature,tampon,compresser,fusionner,pages,formulaire",
        "DOC42": "traitement de texte,docx,odt,rtf,markdown,document,lettre,texte,bureau",
        "XLS42": "tableur,xlsx,ods,csv,formule,graphique,tableau,budget,étiquettes,code qr",
        "PPT42": "présentation,diapositives,pptx,odp,diaporama,notes,bureau",
        "IMG42": "retouche photo,image,recadrer,redimensionner,annoter,heic,jpeg,png,convertir",
        "SCN42": "scanner,numériser,ocr,reconnaissance de texte,pdf,document,caméra,reçu",
    },
    "es": {
        "PDF42": "pdf,editor,anotar,firmar,firma,sello,comprimir,combinar,páginas,formulario",
        "DOC42": "procesador de texto,docx,odt,rtf,markdown,documento,carta,texto,oficina",
        "XLS42": "hoja de cálculo,xlsx,ods,csv,fórmula,gráfico,tabla,presupuesto,etiquetas,código qr",
        "PPT42": "presentación,diapositivas,pptx,odp,notas del orador,oficina",
        "IMG42": "editor de fotos,imagen,recortar,redimensionar,anotar,heic,jpeg,png,convertir",
        "SCN42": "escáner,escanear,ocr,reconocimiento de texto,pdf,documento,cámara,recibo",
    },
    "pt": {
        "PDF42": "pdf,editor,anotar,assinar,assinatura,carimbo,comprimir,juntar,páginas,formulário",
        "DOC42": "processador de texto,docx,odt,rtf,markdown,documento,carta,texto,escritório",
        "XLS42": "planilha,xlsx,ods,csv,fórmula,gráfico,tabela,orçamento,etiquetas,código qr",
        "PPT42": "apresentação,slides,pptx,odp,notas do apresentador,escritório",
        "IMG42": "editor de fotos,imagem,recortar,redimensionar,anotar,heic,jpeg,png,converter",
        "SCN42": "scanner,digitalizar,ocr,reconhecimento de texto,pdf,documento,câmera,recibo",
    },
    "it": {
        "PDF42": "pdf,editor,annotare,firmare,firma,timbro,comprimere,unire,pagine,modulo",
        "DOC42": "elaboratore di testi,docx,odt,rtf,markdown,documento,lettera,testo,ufficio",
        "XLS42": "foglio di calcolo,xlsx,ods,csv,formula,grafico,tabella,budget,etichette,codice qr",
        "PPT42": "presentazione,diapositive,pptx,odp,note del relatore,ufficio",
        "IMG42": "editor foto,immagine,ritagliare,ridimensionare,annotare,heic,jpeg,png,convertire",
        "SCN42": "scanner,scansione,ocr,riconoscimento testo,pdf,documento,fotocamera,scontrino",
    },
    "pl": {
        "PDF42": "pdf,edytor,adnotacje,podpis,podpisać,pieczęć,kompresja,scalanie,strony,formularz",
        "DOC42": "edytor tekstu,docx,odt,rtf,markdown,dokument,list,tekst,biuro",
        "XLS42": "arkusz kalkulacyjny,xlsx,ods,csv,formuła,wykres,tabela,budżet,etykiety,kod qr",
        "PPT42": "prezentacja,slajdy,pptx,odp,pokaz slajdów,notatki prelegenta,biuro",
        "IMG42": "edytor zdjęć,obraz,kadrowanie,zmiana rozmiaru,adnotacje,heic,jpeg,png,konwersja",
        "SCN42": "skaner,skanowanie,ocr,rozpoznawanie tekstu,pdf,dokument,aparat,paragon",
    },
    "hu": {
        "PDF42": "pdf,szerkesztő,jegyzetelés,aláírás,bélyegző,tömörítés,egyesítés,oldalak,űrlap",
        "DOC42": "szövegszerkesztő,docx,odt,rtf,markdown,dokumentum,levél,szöveg,iroda",
        "XLS42": "táblázatkezelő,xlsx,ods,csv,képlet,diagram,táblázat,költségvetés,címkék,qr-kód",
        "PPT42": "prezentáció,diák,pptx,odp,diavetítés,előadói jegyzetek,iroda",
        "IMG42": "képszerkesztő,fotó,vágás,átméretezés,jelölés,heic,jpeg,png,konvertálás",
        "SCN42": "szkenner,szkennelés,ocr,szövegfelismerés,pdf,dokumentum,kamera,nyugta",
    },
    "id": {
        "PDF42": "pdf,editor,anotasi,tanda tangan,cap,kompres,gabung,halaman,formulir,pembaca",
        "DOC42": "pengolah kata,docx,odt,rtf,markdown,dokumen,surat,teks,kantor",
        "XLS42": "spreadsheet,lembar kerja,xlsx,ods,csv,rumus,grafik,tabel,anggaran,label,kode qr",
        "PPT42": "presentasi,slide,pptx,odp,tayangan slide,catatan pembicara,kantor",
        "IMG42": "editor foto,gambar,potong,ubah ukuran,anotasi,heic,jpeg,png,konversi",
        "SCN42": "pemindai,pindai,ocr,pengenalan teks,pdf,dokumen,kamera,struk",
    },
    "ms": {
        "PDF42": "pdf,editor,anotasi,tandatangan,cop,mampat,gabung,halaman,borang,pembaca",
        "DOC42": "pemproses perkataan,docx,odt,rtf,markdown,dokumen,surat,teks,pejabat",
        "XLS42": "hamparan,xlsx,ods,csv,formula,carta,jadual,belanjawan,label,kod qr",
        "PPT42": "persembahan,slaid,pptx,odp,tayangan slaid,nota penceramah,pejabat",
        "IMG42": "editor foto,imej,pangkas,ubah saiz,anotasi,heic,jpeg,png,tukar",
        "SCN42": "pengimbas,imbas,ocr,pengecaman teks,pdf,dokumen,kamera,resit",
    },
    "vi": {
        "PDF42": "pdf,chỉnh sửa,chú thích,ký,chữ ký,con dấu,nén,ghép,trang,biểu mẫu",
        "DOC42": "soạn thảo văn bản,docx,odt,rtf,markdown,tài liệu,thư,văn phòng",
        "XLS42": "bảng tính,xlsx,ods,csv,công thức,biểu đồ,bảng,ngân sách,nhãn,mã qr",
        "PPT42": "trình chiếu,bản trình bày,slide,pptx,odp,ghi chú,văn phòng",
        "IMG42": "chỉnh sửa ảnh,ảnh,cắt,đổi kích thước,chú thích,heic,jpeg,png,chuyển đổi",
        "SCN42": "máy quét,quét,ocr,nhận dạng văn bản,pdf,tài liệu,camera,hóa đơn",
    },
    "fil": {
        "PDF42": "pdf,editor,anotasyon,pirma,lagda,selyo,compress,pagsamahin,pahina,form",
        "DOC42": "word processor,docx,odt,rtf,markdown,dokumento,liham,teksto,opisina",
        "XLS42": "spreadsheet,xlsx,ods,csv,formula,tsart,talahanayan,badyet,label,qr code",
        "PPT42": "presentasyon,slide,pptx,odp,slideshow,tala ng tagapagsalita,opisina",
        "IMG42": "photo editor,larawan,crop,resize,anotasyon,heic,jpeg,png,convert",
        "SCN42": "scanner,scan,ocr,pagkilala ng teksto,pdf,dokumento,camera,resibo",
    },
    "ja": {
        "PDF42": "pdf,編集,注釈,署名,電子署名,印影,圧縮,結合,ページ,フォーム,ビューア",
        "DOC42": "ワープロ,文書作成,docx,odt,rtf,markdown,文書,手紙,オフィス",
        "XLS42": "表計算,スプレッドシート,xlsx,ods,csv,数式,グラフ,予算,ラベル,qrコード",
        "PPT42": "プレゼンテーション,スライド,pptx,odp,スライドショー,発表者ノート,オフィス",
        "IMG42": "画像編集,写真,切り取り,サイズ変更,注釈,heic,jpeg,png,変換",
        "SCN42": "スキャナ,スキャン,ocr,文字認識,pdf,書類,カメラ,レシート",
    },
    "ko": {
        "PDF42": "pdf,편집기,주석,서명,전자서명,도장,압축,병합,페이지,양식,뷰어",
        "DOC42": "워드프로세서,문서 작성,docx,odt,rtf,markdown,문서,편지,오피스",
        "XLS42": "스프레드시트,표 계산,xlsx,ods,csv,수식,차트,예산,라벨,qr 코드",
        "PPT42": "프레젠테이션,슬라이드,pptx,odp,슬라이드쇼,발표자 노트,오피스",
        "IMG42": "이미지 편집,사진,자르기,크기 조절,주석,heic,jpeg,png,변환",
        "SCN42": "스캐너,스캔,ocr,문자 인식,pdf,문서,카메라,영수증",
    },
    "th": {
        "PDF42": "pdf,แก้ไข,หมายเหตุ,ลายเซ็น,ลงนาม,ตราประทับ,บีบอัด,รวมไฟล์,แบบฟอร์ม",
        "DOC42": "โปรแกรมพิมพ์เอกสาร,docx,odt,rtf,markdown,เอกสาร,จดหมาย,ข้อความ,สำนักงาน",
        "XLS42": "สเปรดชีต,ตารางคำนวณ,xlsx,ods,csv,สูตร,แผนภูมิ,งบประมาณ,ฉลาก,คิวอาร์โค้ด",
        "PPT42": "งานนำเสนอ,สไลด์,pptx,odp,สไลด์โชว์,บันทึกผู้บรรยาย,สำนักงาน",
        "IMG42": "แต่งรูป,รูปภาพ,ครอบตัด,ปรับขนาด,หมายเหตุ,heic,jpeg,png,แปลงไฟล์",
        "SCN42": "สแกนเนอร์,สแกน,ocr,รู้จำข้อความ,pdf,เอกสาร,กล้อง,ใบเสร็จ",
    },
    "zh-Hans": {
        "PDF42": "pdf,编辑器,批注,签名,电子签名,印章,压缩,合并,页面,表单,阅读器",
        "DOC42": "文字处理,文档,docx,odt,rtf,markdown,信函,文本,办公",
        "XLS42": "电子表格,表格,xlsx,ods,csv,公式,图表,预算,标签,二维码,条形码",
        "PPT42": "演示文稿,幻灯片,pptx,odp,放映,演讲者备注,办公",
        "IMG42": "图片编辑,照片,裁剪,调整大小,标注,heic,jpeg,png,转换",
        "SCN42": "扫描仪,扫描,ocr,文字识别,pdf,文档,相机,收据",
    },
    "zh-Hant": {
        "PDF42": "pdf,編輯器,註解,簽名,電子簽章,印章,壓縮,合併,頁面,表單,閱讀器",
        "DOC42": "文書處理,文件,docx,odt,rtf,markdown,信件,文字,辦公",
        "XLS42": "試算表,表格,xlsx,ods,csv,公式,圖表,預算,標籤,qr碼,條碼",
        "PPT42": "簡報,投影片,pptx,odp,播放,講者備忘稿,辦公",
        "IMG42": "影像編輯,照片,裁切,調整大小,標示,heic,jpeg,png,轉換",
        "SCN42": "掃描器,掃描,ocr,文字辨識,pdf,文件,相機,收據",
    },
    "ar": {
        "PDF42": "pdf,محرر,تعليقات,توقيع,توقيع رقمي,ختم,ضغط,دمج,صفحات,نموذج",
        "DOC42": "معالج نصوص,docx,odt,rtf,markdown,مستند,رسالة,نص,مكتب",
        "XLS42": "جدول بيانات,xlsx,ods,csv,صيغة,مخطط,جدول,ميزانية,ملصقات,رمز qr",
        "PPT42": "عرض تقديمي,شرائح,pptx,odp,عرض الشرائح,ملاحظات المتحدث,مكتب",
        "IMG42": "محرر صور,صورة,قص,تغيير الحجم,تعليقات,heic,jpeg,png,تحويل",
        "SCN42": "ماسح ضوئي,مسح,ocr,التعرف على النص,pdf,مستند,كاميرا,إيصال",
    },
}


# The site's app taglines that exceed the 30-character subtitle limit, shortened here (review with a native speaker).
SUBTITLES = {
    "es": {"XLS42": "Hojas de cálculo con fórmulas", "IMG42": "Recorta, ajusta y anota fotos", "SCN42": "Escanea y reconoce texto"},
    "fr": {"PDF42": "Vos PDF, plein de possibilités", "PPT42": "Des idées à présenter", "IMG42": "Recadrer, ajuster, annoter",
           "SCN42": "Numériser et reconnaître"},
    "pt": {"XLS42": "Planilhas com fórmulas", "PPT42": "Ideias que merecem palco", "IMG42": "Recorte, ajuste e anote fotos",
           "SCN42": "Digitalize e reconheça texto"},
    "id": {"PDF42": "PDF penuh kemungkinan", "PPT42": "Ide yang layak dipresentasikan", "IMG42": "Potong, atur, dan beri anotasi",
           "SCN42": "Pindai dan kenali teks"},
    "ru": {"IMG42": "Обрезка, правка и разметка"},
    "de": {"PPT42": "Ideen, die überzeugen", "IMG42": "Bilder zuschneiden, anpassen"},
    "vi": {"PPT42": "Ý tưởng đáng trình bày", "SCN42": "Quét và nhận dạng văn bản"},
    "tr": {"XLS42": "Rakamlarınıza hikâye katın", "IMG42": "Kırp, ayarla, işaretle"},
    "fil": {"XLS42": "Spreadsheet na may formula", "IMG42": "I-crop, ayusin, at markahan", "SCN42": "I-scan at basahin ang teksto"},
    "it": {"XLS42": "Fogli di calcolo e formule", "PPT42": "Idee da presentare", "IMG42": "Ritaglia, regola, annota",
           "SCN42": "Scansiona e riconosci testo"},
    "pl": {"XLS42": "Arkusze, formuły i wykresy"},
    "ms": {"PDF42": "PDF dengan pelbagai fungsi", "IMG42": "Pangkas, laras dan anotasi", "SCN42": "Imbas dan kenal pasti teks"},
    "hu": {"PPT42": "Bemutatásra érdemes ötletek"},
}


def description(t, app):
    site, pages = APPS[app]
    parts = [t["apps"][site]["text"]]
    for page in pages:
        f = t["features"][page]
        parts.append(f["intro"])
        parts.append("\n".join("• " + p for p in f["points"]))
        parts.append(f["note"])
    parts.append(t["home"]["privacyText"])
    parts.append(t["name"]["offText"])
    return "\n\n".join(parts)


def main():
    OUT.mkdir(exist_ok=True)
    summary = []
    for lang, locale in STORE_LOCALE.items():
        t = json.loads((ROOT / "website" / "locales" / f"{lang}.json").read_text(encoding="utf-8"))
        apps = {}
        for app, (site, _) in APPS.items():
            entry = {
                "name": app,
                "subtitle": SUBTITLES.get(lang, {}).get(app, t["apps"][site]["tag"]),
                "promotionalText": t["name"]["offText"],
                "keywords": KEYWORDS[lang][app],
                "description": description(t, app),
            }
            check = [f"{k}: {len(v)} characters, limit {LIMITS[k]}" for k, v in entry.items() if len(v) > LIMITS[k]]
            if any(" ," in k or ", " in k for k in [entry["keywords"]]):
                check.append("keywords: remove spaces around commas")
            entry["check"] = check
            apps[app] = entry
            summary += [f"{lang} {app} {c}" for c in check]
        doc = {
            "siteLanguage": lang,
            "appStoreLocale": locale,
            "note": None if locale else "App Store Connect has no Filipino localization; kept for reference only.",
            "privacyPolicyURL": PRIVACY_URL.format(lang=lang),
            "supportURL": SUPPORT_URL.format(lang=lang),
            "marketingURL": MARKETING_URL.format(lang=lang),
            "apps": apps,
        }
        (OUT / f"{lang}.json").write_text(json.dumps(doc, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"Wrote {len(STORE_LOCALE)} languages × {len(APPS)} apps to {OUT.relative_to(ROOT)}")
    for line in summary:
        print("check:", line)


if __name__ == "__main__":
    main()
