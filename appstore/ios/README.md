# App Store: OFF42 for iPad and iPhone

Material for the App Store Connect record of the iOS/iPadOS app (`ios/`, bundle id `cc.off42.office`). Nothing here is
uploaded automatically. The Mac apps' store variants are described in [../README.md](../README.md).

| Item | Where |
| --- | --- |
| Name (≤30), subtitle (≤30), promotional text (≤170), description (≤4000), keywords (≤100), 20 languages | [listings/](listings) `<language>.json`, written by `python3 appstore/ios/make_listings.py` from [texts.json](texts.json) and the Play listing (`android/play/listing.json`); `--check` (run in CI) checks the limits and that the files are current. `appStoreLocale` names the App Store Connect localization; Filipino has none (`null`) |
| App icon 1024 × 1024 (no transparency) | `ios/OFF42/Assets.xcassets/AppIcon.appiconset/icon-1024.png` (the site's "42" mark, `ios/scripts/make-icon.py`) |
| Screenshots, iPhone 6.9" (1320 × 2868) and iPad 13" (2752 × 2064 landscape) | CI artifacts `ios-screenshots-iphone-6.9` and `ios-screenshots-ipad-13` of the iOS workflow, taken by the UI tests on the newest "Pro Max" iPhone and 13-inch iPad Pro simulators: 1 document, 2 spreadsheet, 3 presentation, 4 PDF, 5 codes, 6 signatures, 7 stamp (light), 8 dark mode, 9 Arabic |
| Support URL | https://off42.cc/en/support/ |
| Marketing URL | https://off42.cc/ |
| Privacy Policy URL | https://off42.cc/en/privacy/ |
| Contact (App Review) | tr.zorin@gmail.com |
| Category | Productivity (primary); Business (secondary, optional) |
| Price | Free, no in-app purchases |

Keywords avoid other companies' product names (App Review rejects them); `make_listings.py` checks for the obvious
ones. The 19 translated texts were written for this release and should be read by native speakers.

## App Privacy ("nutrition label")

Answer **Data Not Collected**. From the code in this repository:

- No account, analytics, advertising, crash reporting or third-party SDK. The only linked code besides Apple's
  frameworks is the off42 engine (Rust, `rust/crates/off42-mobile`), which takes and returns bytes.
- No network: the app makes no connections; the document preview's `WKWebView` loads only the in-memory page and its
  Content-Security-Policy blocks network loads. CI checks that the binary references no socket or URLSession API
  (`ios/scripts/check-app-bundle.sh`). iOS itself offers no way to remove network access from an app, so the store
  text says "makes no network connections".
- Files are read only when the user picks them (document picker) or sends them to OFF42 ("Open in"/Share); they stay
  in memory. Copies iOS places in `Documents/Inbox` for "Open in" are deleted after reading. Files are written only
  where the user saves them or handed to the app the user shares to (through a temporary file).
- No preferences or other data are stored between launches.

Privacy manifest: `ios/OFF42/PrivacyInfo.xcprivacy` — no tracking, no tracking domains, no collected data types.
One required-reason category is declared: **file timestamp APIs, reason C617.1**, because the binary references
`stat`, `fstat` and `fstatat` through the Rust standard library in the engine (fontdb looks for font files when stamp
text is drawn; file sizes are read). No timestamp is read or leaves the device. **Owner:** check that C617.1 is the
answer you want to give; Apple's reasons do not describe a library probing font paths exactly. CI fails if the binary
references a required-reason category (file timestamps, boot time, disk space) that the manifest does not declare, or
names user-defaults, keyboard, boot-time or disk-space Foundation APIs.

## Export compliance

`ITSAppUsesNonExemptEncryption` is `false` in `ios/OFF42/Info.plist`: the app has no encryption of its own and no
network. (Technical summary, not legal advice; the owner confirms it when App Store Connect asks.)

## Age rating

Answer "None" to every content question (no violence, sexual content, profanity, drugs, gambling, horror, medical
information), no unrestricted web access (the preview shows only the opened file and follows no links), no
user-generated content shared with others, no messaging, no contests. Expected rating: 4+.

## App Review notes (suggested)

"No account is needed. OFF42 opens and converts office files on the device and makes no network connections. To try
it: open a DOCX, XLSX, PPTX or PDF from Files (Open file in any section, or Share › OFF42 in Files), use Save as in
the toolbar, or create a QR code in Labels & codes and a signature in Signatures & stamps. A drawn signature is an
image, not a certificate-based digital signature."

## Owner's steps

1. **Apple Developer Program** membership (the same account as for the Mac App Store and DMG notarization, D12). Note
   the **Team ID**.
2. **Identifiers**: register the explicit App ID `cc.off42.office` (platform iOS, no capabilities needed).
3. **Certificates**: an **Apple Distribution** certificate; export it with its private key as .p12.
4. **Profiles**: an **App Store** distribution provisioning profile for `cc.off42.office`.
5. **App Store Connect** → Apps → New App: platform iOS, name from `listings/en.json` (names are unique store-wide; if
   it is taken, choose another, e.g. "OFF42 Office"), primary language English (U.S.), bundle id `cc.off42.office`,
   SKU e.g. `off42-ios`. Fill App Information (category, content rights: no third-party content), Age Rating, App
   Privacy ("Data Not Collected"), Pricing (Free), and per language the texts from `listings/`.
6. **Repository secrets** for signed CI builds (see `ios/README.md`): `IOS_TEAM_ID`,
   `IOS_DISTRIBUTION_CERTIFICATE_P12_BASE64`, `IOS_DISTRIBUTION_CERTIFICATE_PASSWORD`, `IOS_APP_STORE_PROFILE_BASE64`;
   for uploads also an App Store Connect API key (Users and Access → Integrations): `APP_STORE_CONNECT_API_KEY_ID`,
   `APP_STORE_CONNECT_API_ISSUER_ID`, `APP_STORE_CONNECT_API_KEY_BASE64`.
7. Run the iOS workflow by hand with "upload" (or upload `off42-ios-ipa` with Transporter), test with TestFlight on an
   iPad and an iPhone, add the screenshots from the CI artifacts, and submit for review.

Not done or verified yet: a signed build, App Store Connect validation, TestFlight, runs on physical devices, and
review of the translations by native speakers.
