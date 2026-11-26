off42 — Linux x86_64 preview (experimental)

Contents: off42 (command line) and PDF42, DOC42, XLS42, PPT42, IMG42, SCN42 (experimental windows). Unsigned.
Needs glibc; the windows also need OpenGL and an X11 or Wayland session.

  ./off42 --help                          all commands
  ./off42 convert report.docx report.pdf  documents, sheets, slides, images; plain-layout PDF
  ./off42 text book.xlsx                  text of documents, sheets, slides and PDF
  ./off42 pdf merge all.pdf a.pdf b.pdf   also: off42 pdf extract IN 1,3-5 OUT
  ./off42 labels sheet.pdf --code qr A-1 A-2   also: barcode, marks, signature, stamp

Run an app:            ./DOC42 [file]
Install for this user: ./install.sh   (copies to ~/.local/opt/off42, adds menu entries and ~/.local/bin links)

The window is a simple shared front end: page/sheet/slide list, document text or cell table, image preview,
a Commands menu, Export (file actions) and Save As (the format follows the file extension).

App command line (every app):
  --convert IN OUT     open IN and save OUT, e.g. DOC42 --convert notes.md notes.docx
  --text FILE          print document text
  --items FILE         print pages, sheets, slides or frames
  --table FILE         print the sheet as tab-separated values (XLS42)
  --export ID IN OUT   run an import/export action listed by --formats
  --formats, --commands, --help, --version

Not available on Linux yet: page canvases and WYSIWYG rich-text editing, rulers, printing, scanner
acquisition, OCR, PDF page rendering and text extraction, signing with system certificates, and the
Windows settings dialogs used by many commands. These report "requires Windows" or are missing.
