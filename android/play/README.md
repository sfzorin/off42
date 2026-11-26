# Google Play listing

Material for the Play Console entry of the Android app (application id `cc.off42.office`). Nothing here is uploaded
automatically.

| Item | Where |
| --- | --- |
| Title (≤30), short (≤80) and full description, 20 languages | [listing.json](listing.json); `python3 android/play/generate-metadata.py` writes `metadata/<locale>/*.txt` (the `fastlane supply` layout) and checks the limits |
| App icon, 512 × 512 | [icon-512.png](icon-512.png) (the site icon, `website/public/assets/icon-512.png`) |
| Phone and 10-inch tablet screenshots | CI artifacts `android-screenshots-phone` and `android-screenshots-tablet` of the Android workflow (`phone-*.png`, `tablet-*.png`, light theme 1–7 and dark theme 8), taken by the instrumented tests on a Pixel 6 and a Pixel Tablet emulator, API 34 |
| Privacy policy | https://off42.cc/en/privacy/ (also in the other 19 language paths, for example https://off42.cc/ru/privacy/) |
| Support page | https://off42.cc/en/support/ |
| Contact email | tr.zorin@gmail.com |
| Category | Productivity (suggested) |

Locales use Play's codes: `en-US`, `ar`, `de-DE`, `es-ES`, `fil`, `fr-FR`, `hu-HU`, `id`, `it-IT`, `ja-JP`, `ko-KR`,
`ms`, `pl-PL`, `pt-BR`, `ru-RU`, `th`, `tr-TR`, `vi`, `zh-CN`, `zh-TW` (the site's `pt`, `zh-Hans` and `zh-Hant` texts
are Brazilian Portuguese, Simplified and Traditional Chinese).

## Data safety answers

These follow from the code in this repository (checked in CI: the release manifest has no `uses-permission`):

- **Data collected: none. Data shared: none.** The app has no `INTERNET` permission and no other permission, no
  analytics, advertising, crash reporting or account. It contains no third-party SDK that sends data.
- Files are read only when the user picks them in the system file picker (`ACTION_OPEN_DOCUMENT`), opens them with
  OFF42 from another app (`ACTION_VIEW`) or shares them to it (`ACTION_SEND`). They are kept in memory while open.
- Files are written only where the user chooses in the system save dialog (`ACTION_CREATE_DOCUMENT`) or handed to
  another app through the share sheet; for sharing and PDF page rendering, a temporary copy is written to the app's
  private cache.
- Backups and device transfer are disabled (`allowBackup="false"`, `data_extraction_rules.xml` excludes everything).
- Encryption in transit: not applicable (no network). Deletion requests: not applicable (no data kept).

## Still needed from the owner

- A Play developer account, the store entry and an upload key. CI signs the release bundle only when the repository
  has the secrets `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` and
  `ANDROID_KEY_PASSWORD`; otherwise the bundle artifact is unsigned. With Play App Signing, this key is the upload key.
- A feature graphic (1024 × 500), which Play requires and this repository does not have.
- The content rating questionnaire and the target audience form in the Play Console.
- Review of the 19 translated listings by native speakers (they were written for this release, not taken from the
  site).
