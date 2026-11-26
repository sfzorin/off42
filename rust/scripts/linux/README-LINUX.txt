off42 — Linux x86_64 preview (experimental)

Apps: PDF42, DOC42, XLS42, PPT42, IMG42, SCN42. Unsigned. Needs glibc, OpenGL and an X11 or Wayland session
for the window; the command-line options work without a display.

Run an app:            ./DOC42 [file]
Install for this user: ./install.sh   (copies to ~/.local/opt/off42, adds menu entries and ~/.local/bin links)

The window is a simple shared front end: page/sheet/slide list, document text or cell table, image preview,
a Commands menu, Export (file actions) and Save As (the format follows the file extension).

Command line (every app):
  --convert IN OUT     open IN and save OUT, e.g. DOC42 --convert notes.md notes.docx
  --text FILE          print document text
  --items FILE         print pages, sheets, slides or frames
  --table FILE         print the sheet as tab-separated values (XLS42)
  --export ID IN OUT   run an import/export action listed by --formats
  --formats, --commands, --help, --version

Not available on Linux yet: page canvases and WYSIWYG rich-text editing, rulers, printing, scanner
acquisition, OCR, PDF page rendering and text extraction, signing with system certificates, and the
Windows settings dialogs used by many commands. These report "requires Windows" or are missing.
