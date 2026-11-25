# Website regression scenarios

Run `npm run build && npm run check`, then `npm run preview` before browser checks.
Do not publish a partial build.

1. At a 390 × 844 viewport, visit every localized route from `build-manifest.json`.
   Require one H1, the expected `html[lang]`, and document scroll width equal to
   viewport width. Inspect EN/RU/AR/CJK screenshots. Repeat at desktop width.
2. On `/ru/features/compress-pdf/`, open the language picker. It must contain 20
   languages. Select Deutsch and require `/de/features/compress-pdf/`, with German
   heading, description, navigation and page content. Open the picker and press
   Escape; require it closed and focus returned to its summary.
3. On mobile, open the main menu, follow Downloads, and require the localized
   download route. With null release entries, require two unavailable statuses
   and no direct DMG/EXE download links. GitHub releases and build instructions
   must have real repository destinations.
4. Open a feature FAQ; close and reopen it with the keyboard. Require the answer
   to follow its expanded state. Follow related feature links and the main CTA.
5. Request a missing URL; require HTTP 404 and the noindex error page. Require
   `/` → `/en/`, valid CSS/JS/image MIME types and no console errors on live pages.
6. Inspect live `robots.txt`, `sitemap.xml`, canonical/alternate URLs, response
   security headers, and both canonical/custom-host routes after a deployment.

7. On `/en/download/` press each copy button; require the command in the clipboard and
   the button to show ✓ and return. Require the build commands to stay inside the
   viewport at 390 px (horizontal scroll inside the code line, not the page).
8. On the home page require the formats table (six apps, Opens/Saves) and the FAQ;
   at 390 px each format cell shows its Opens/Saves label.

9. On `/en/` require the `#demo` video with poster, `preload="none"` and controls; play it and require
   `readyState` 4, 1920×1080 and no console errors. Require no `<video>` on any other language's home page.
10. On every home page require the `#codes` section with both screenshots loaded; at 390 px require no horizontal
   overflow (check AR for RTL) and the QR dialog below the window. Open `features/labels-codes/` in EN/AR/DE and
   require one H1, the label-sheet screenshot and the certification-mark caveat in the note.

## Reproduced defects

- 2026-11-06: screen-reader-only text at `left: -10000px` caused Arabic RTL pages
  to have document scroll width 10390 at viewport 390. Replaced offscreen positioning
  with clipped accessible text. Keep scenario 1 for Arabic across all page types;
  do not hide body overflow to conceal a layout error.
- 2026-11-06: normal-size white button labels and several muted labels failed
  WCAG AA text contrast. Darkened the orange button and muted text tokens. Preserve
  at least 4.5:1 contrast for normal-size text during design changes.
- 2026-11-06: on `/ar/download/` at 375 px the unbroken `git clone` command widened the
  page to 554 px (grid track without `minmax(0, …)`). The code line now scrolls inside
  its own box. Keep scenario 7 for long commands in RTL and LTR locales.
- 2026-11-06: the signatures section used a percentage `gap`; in the one-column mobile
  layout the percentage row gap resolved after sizing and the signed-contract figure
  overflowed the section by about 95 px onto the next heading. Row gaps are now fixed
  lengths. Check the signatures section bottom at 375 px.
