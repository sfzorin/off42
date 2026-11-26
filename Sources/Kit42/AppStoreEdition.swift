// Mac App Store variant (sandboxed). Compiled only with `-D APP_STORE` (scripts/build-app-store.sh);
// the DMG build (./build.sh) contains none of this file.
#if APP_STORE
import AppKit

/// Shared storage, persistent file access and the features the App Sandbox does not allow.
///
/// - The library of signatures, stamps and signing keys and the shared preferences live in the App Group container
///   (`Suite42AppGroup` in Info.plist, set by scripts/build-app-store.sh), so the six apps keep sharing them.
/// - The DMG version's library (~/Library/Application Support/Suite42) lies outside the sandbox: the user imports it
///   once by choosing that folder (File access is granted by the open panel; nothing is read without it).
public enum AppStoreEdition {
    /// The App Group identifier, e.g. `ABCDE12345.cc.off42`.
    public static let appGroup: String? = Bundle.main.object(forInfoDictionaryKey: "Suite42AppGroup") as? String

    /// Preferences shared by every app (language, label sheets, printer corrections).
    public static let sharedDefaults: UserDefaults = appGroup.flatMap { UserDefaults(suiteName: $0) }
        ?? UserDefaults(suiteName: "com.sfzorin.suite42") ?? .standard

    /// `Library/Application Support/Suite42` inside the App Group container (this app's container as a fallback).
    public static let sharedSupportDirectory: URL = {
        let base = appGroup.flatMap { FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: $0) }?
            .appendingPathComponent("Library/Application Support", isDirectory: true)
            ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        let url = base.appendingPathComponent("Suite42", isDirectory: true)
        try? FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }()

    /// The user's real home folder (NSHomeDirectory() is the container inside the sandbox).
    static var realHome: URL {
        if let pw = getpwuid(getuid()), let dir = pw.pointee.pw_dir { return URL(fileURLWithPath: String(cString: dir), isDirectory: true) }
        return URL(fileURLWithPath: NSHomeDirectory(), isDirectory: true)
    }

    /// The DMG version's library.
    static var legacyLibrary: URL { realHome.appendingPathComponent("Library/Application Support/Suite42", isDirectory: true) }

    static let libraryItems = ["Stamps", "Signing Keys"]

    /// True inside the App Sandbox. A sandbox-flavoured binary run unsandboxed (e.g. from .build in Terminal)
    /// must never touch the DMG version's library.
    public static var isSandboxed: Bool { ProcessInfo.processInfo.environment["APP_SANDBOX_CONTAINER_ID"] != nil }

    /// Called once per launch (from the menu bar): moves a pre-group library into the group container, keeps the
    /// recent-document bookmarks current and offers the one-time import from the DMG version.
    static func start() {
        guard !started, isSandboxed else { return }
        started = true
        copyContainerLibrary()
        let center = NotificationCenter.default
        for name in [NSApplication.willTerminateNotification, NSApplication.didResignActiveNotification] {
            center.addObserver(forName: name, object: nil, queue: .main) { _ in
                SecurityScopedRecents.remember(NSDocumentController.shared.recentDocumentURLs)
            }
        }
        // Tests, audits and command-line modes run without prompts.
        let quiet = CommandLine.arguments.dropFirst().contains { $0.hasPrefix("--") }
            || ProcessInfo.processInfo.environment.keys.contains { $0.hasPrefix("K42_") }
        if !quiet { DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { offerLegacyImport() } }
    }
    private static var started = false

    /// A library written into this app's own container (before the App Group was configured) is copied into the
    /// shared one.
    static func copyContainerLibrary() {
        let own = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("Suite42", isDirectory: true)
        guard isSandboxed, own.standardizedFileURL != sharedSupportDirectory.standardizedFileURL else { return }
        copyLibrary(from: own)
    }

    static var libraryIsEmpty: Bool {
        libraryItems.allSatisfy { name in
            ((try? FileManager.default.contentsOfDirectory(atPath: sharedSupportDirectory.appendingPathComponent(name).path)) ?? []).isEmpty
        }
    }

    /// Asks once, when the DMG version's library exists and the shared library is still empty.
    static func offerLegacyImport() {
        let key = "appStore.legacyImportOffered"
        guard !UserDefaults.standard.bool(forKey: key), libraryIsEmpty,
              FileManager.default.fileExists(atPath: legacyLibrary.path) else { return }
        UserDefaults.standard.set(true, forKey: key)
        let alert = NSAlert()
        alert.messageText = text("Import signatures and stamps from the previous version?")
        alert.informativeText = text("The App Store version keeps signatures, stamps and signing keys in a new library shared by its apps. Choose the old Suite42 folder to copy them; macOS asks for your permission once.")
        alert.addButton(withTitle: L("Import"))
        alert.addButton(withTitle: L("Cancel"))
        if alert.runModal() == .alertFirstButtonReturn { importLegacyLibrary() }
    }

    /// File ▸ (app menu) Import Signatures and Stamps…: the user picks the old Suite42 folder.
    public static func importLegacyLibrary() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.showsHiddenFiles = true
        panel.directoryURL = legacyLibrary
        panel.message = text("Choose the Suite42 folder of the previous version")
        panel.prompt = L("Import")
        guard panel.runModal() == .OK, var folder = panel.url else { return }
        let access = folder.startAccessingSecurityScopedResource()
        defer { if access { folder.stopAccessingSecurityScopedResource() } }
        // Application Support itself was chosen: use its Suite42 folder.
        let nested = folder.appendingPathComponent("Suite42", isDirectory: true)
        if FileManager.default.fileExists(atPath: nested.path) { folder = nested }
        let count = copyLibrary(from: folder)
        let alert = NSAlert()
        alert.messageText = count > 0 ? String(format: text("Imported %d items."), count)
            : text("This folder has no signatures, stamps or signing keys.")
        alert.runModal()
    }

    /// Copies Stamps and Signing Keys item by item, never replacing what the shared library already has.
    @discardableResult
    static func copyLibrary(from source: URL) -> Int {
        let fm = FileManager.default
        var copied = 0
        for name in libraryItems {
            let from = source.appendingPathComponent(name, isDirectory: true)
            guard let entries = try? fm.contentsOfDirectory(at: from, includingPropertiesForKeys: nil) else { continue }
            let to = sharedSupportDirectory.appendingPathComponent(name, isDirectory: true)
            try? fm.createDirectory(at: to, withIntermediateDirectories: true)
            for entry in entries {
                let target = to.appendingPathComponent(entry.lastPathComponent)
                guard !fm.fileExists(atPath: target.path) else { continue }
                if (try? fm.copyItem(at: entry, to: target)) != nil { copied += 1 }
            }
        }
        return copied
    }

    /// Smart-card signing through third-party PKCS#11 modules needs the P11Bridge42 helper, which the App Store
    /// variant does not contain (it would load unsigned vendor code).
    public static func showSmartCardModulesUnavailable() {
        unavailable(text("Smart-card modules (PKCS#11) cannot be loaded in the App Store version. Smart cards that macOS supports directly appear in the Keychain certificate list."))
    }

    /// CryptoPro CSP is driven through its command-line tools in /opt/cprocsp/bin.
    public static func showCryptoProUnavailable() {
        unavailable(text("CryptoPro CSP cannot be used in the App Store version: the App Sandbox does not allow running its command-line tools."))
    }

    static func unavailable(_ info: String) {
        let alert = NSAlert()
        alert.messageText = text("Not available in the App Store version")
        alert.informativeText = info
        alert.addButton(withTitle: L("OK"))
        alert.runModal()
    }

    /// Menu item target for the app menu.
    final class Actions: NSObject {
        static let shared = Actions()
        @objc func importLibrary(_ sender: Any?) { AppStoreEdition.importLegacyLibrary() }
    }

    static func importMenuItem() -> NSMenuItem {
        let item = NSMenuItem(title: text("Import Signatures and Stamps…"), action: #selector(Actions.importLibrary(_:)), keyEquivalent: "")
        item.target = Actions.shared
        return item
    }

    // MARK: Strings of the App Store variant only (kept out of the shared catalogs so the DMG build is unchanged)

    public static func text(_ english: String) -> String {
        strings[english]?[Language.current.rawValue] ?? english
    }

    /// Headless checks for `--selftest` of the App Store build: the disabled features stay disabled and every
    /// string of this variant is translated.
    public static func selfCheck() -> Bool {
        var ok = true
        func check(_ condition: Bool, _ name: String) {
            print("\(condition ? "PASS" : "FAIL") App Store edition: \(name)")
            if !condition { ok = false }
        }
        check(PKCS11Modules.installed().isEmpty, "no PKCS#11 modules are offered")
        check(PKCS11Modules.bridgeURL() == nil, "no P11Bridge42 helper is used")
        let missing = missingTranslations()
        missing.forEach { print("  missing translation \($0)") }
        check(missing.isEmpty, "\(strings.count) strings translated into \(Language.allCases.count) languages")
        let env = ProcessInfo.processInfo.environment
        check(StampLibrary.shared.folder.path.hasPrefix(sharedSupportDirectory.path) || env["K42_STAMP_LIBRARY"] != nil,
              "stamp library is in the shared directory")
        check(KeyVault.directory.path.hasPrefix(sharedSupportDirectory.path) || env["SUITE42_KEY_DIR"] != nil,
              "signing keys are in the shared directory")
        return ok
    }

    /// Languages missing for any string (empty when complete); checked by `--selftest` of the App Store build.
    public static func missingTranslations() -> [String] {
        strings.flatMap { key, table in
            Language.allCases.filter { $0 != .en && (table[$0.rawValue] ?? "").isEmpty }.map { "\($0.rawValue): \(key)" }
        }.sorted()
    }

    static let strings: [String: [String: String]] = [
        "Import Signatures and Stamps…": [
            "ru": "Импортировать подписи и печати…", "tr": "İmzaları ve Mühürleri İçe Aktar…",
            "de": "Unterschriften und Stempel importieren…", "fr": "Importer les signatures et tampons…",
            "es": "Importar firmas y sellos…", "pt": "Importar assinaturas e carimbos…", "it": "Importa firme e timbri…",
            "pl": "Importuj podpisy i pieczęcie…", "hu": "Aláírások és bélyegzők importálása…",
            "id": "Impor Tanda Tangan dan Cap…", "ms": "Import Tandatangan dan Cop…", "vi": "Nhập chữ ký và con dấu…",
            "fil": "I-import ang mga Lagda at Selyo…", "ja": "署名と印影を読み込む…", "ko": "서명 및 도장 가져오기…",
            "th": "นำเข้าลายเซ็นและตราประทับ…", "zh-Hans": "导入签名和印章…", "zh-Hant": "匯入簽名和印章…",
            "ar": "استيراد التوقيعات والأختام…",
        ],
        "Import signatures and stamps from the previous version?": [
            "ru": "Импортировать подписи и печати из прежней версии?",
            "tr": "İmzalar ve mühürler önceki sürümden içe aktarılsın mı?",
            "de": "Unterschriften und Stempel aus der vorherigen Version importieren?",
            "fr": "Importer les signatures et tampons de la version précédente ?",
            "es": "¿Importar las firmas y los sellos de la versión anterior?",
            "pt": "Importar assinaturas e carimbos da versão anterior?",
            "it": "Importare firme e timbri dalla versione precedente?",
            "pl": "Zaimportować podpisy i pieczęcie z poprzedniej wersji?",
            "hu": "Importálja az aláírásokat és bélyegzőket az előző verzióból?",
            "id": "Impor tanda tangan dan cap dari versi sebelumnya?",
            "ms": "Import tandatangan dan cop daripada versi sebelumnya?",
            "vi": "Nhập chữ ký và con dấu từ phiên bản trước?",
            "fil": "I-import ang mga lagda at selyo mula sa nakaraang bersyon?",
            "ja": "以前のバージョンから署名と印影を読み込みますか？",
            "ko": "이전 버전에서 서명과 도장을 가져오시겠습니까?",
            "th": "นำเข้าลายเซ็นและตราประทับจากเวอร์ชันก่อนหน้าหรือไม่",
            "zh-Hans": "要从旧版本导入签名和印章吗？", "zh-Hant": "要從舊版本匯入簽名和印章嗎？",
            "ar": "هل تريد استيراد التوقيعات والأختام من الإصدار السابق؟",
        ],
        "The App Store version keeps signatures, stamps and signing keys in a new library shared by its apps. Choose the old Suite42 folder to copy them; macOS asks for your permission once.": [
            "ru": "Версия из App Store хранит подписи, печати и ключи подписи в новой библиотеке, общей для её приложений. Выберите старую папку Suite42, чтобы скопировать их; macOS один раз попросит разрешения.",
            "tr": "App Store sürümü imzaları, mühürleri ve imzalama anahtarlarını uygulamalarının paylaştığı yeni bir kitaplıkta tutar. Bunları kopyalamak için eski Suite42 klasörünü seçin; macOS bir kez izninizi ister.",
            "de": "Die App-Store-Version speichert Unterschriften, Stempel und Signaturschlüssel in einer neuen Bibliothek, die ihre Apps gemeinsam nutzen. Wählen Sie den alten Ordner „Suite42“, um sie zu kopieren; macOS fragt einmal nach Ihrer Erlaubnis.",
            "fr": "La version App Store conserve les signatures, tampons et clés de signature dans une nouvelle bibliothèque partagée par ses apps. Choisissez l’ancien dossier Suite42 pour les copier ; macOS vous demande l’autorisation une seule fois.",
            "es": "La versión de la App Store guarda las firmas, los sellos y las claves de firma en una nueva biblioteca compartida por sus apps. Elige la antigua carpeta Suite42 para copiarlos; macOS te pedirá permiso una sola vez.",
            "pt": "A versão da App Store guarda assinaturas, carimbos e chaves de assinatura em uma nova biblioteca compartilhada pelos seus apps. Escolha a pasta Suite42 antiga para copiá-los; o macOS pede sua permissão uma única vez.",
            "it": "La versione App Store conserva firme, timbri e chiavi di firma in una nuova libreria condivisa dalle sue app. Scegli la vecchia cartella Suite42 per copiarli; macOS chiede il permesso una sola volta.",
            "pl": "Wersja z App Store przechowuje podpisy, pieczęcie i klucze podpisu w nowej bibliotece wspólnej dla jej aplikacji. Wybierz stary folder Suite42, aby je skopiować; macOS jeden raz poprosi o zgodę.",
            "hu": "Az App Store-verzió az aláírásokat, bélyegzőket és aláírókulcsokat egy új, az alkalmazásai által közösen használt könyvtárban tárolja. Másolásukhoz válassza ki a régi Suite42 mappát; a macOS egyszer kér engedélyt.",
            "id": "Versi App Store menyimpan tanda tangan, cap, dan kunci penandatanganan di pustaka baru yang dipakai bersama oleh aplikasinya. Pilih folder Suite42 lama untuk menyalinnya; macOS meminta izin Anda sekali saja.",
            "ms": "Versi App Store menyimpan tandatangan, cop dan kunci tandatangan dalam pustaka baharu yang dikongsi oleh apnya. Pilih folder Suite42 lama untuk menyalinnya; macOS meminta kebenaran anda sekali sahaja.",
            "vi": "Phiên bản App Store lưu chữ ký, con dấu và khóa ký trong một thư viện mới dùng chung cho các ứng dụng của nó. Chọn thư mục Suite42 cũ để sao chép; macOS sẽ xin phép bạn một lần.",
            "fil": "Iniimbak ng bersyon sa App Store ang mga lagda, selyo at susi sa pagpirma sa isang bagong library na pinagsasaluhan ng mga app nito. Piliin ang lumang folder na Suite42 para kopyahin ang mga ito; minsan lang hihingi ng pahintulot ang macOS.",
            "ja": "App Store版では、署名、印影、署名鍵を各アプリで共有する新しいライブラリに保存します。コピーするには古いSuite42フォルダを選択してください。macOSが一度だけ許可を求めます。",
            "ko": "App Store 버전은 서명, 도장, 서명 키를 앱들이 함께 쓰는 새 라이브러리에 보관합니다. 복사하려면 이전 Suite42 폴더를 선택하십시오. macOS가 한 번만 권한을 요청합니다.",
            "th": "เวอร์ชัน App Store เก็บลายเซ็น ตราประทับ และกุญแจลงนามไว้ในคลังใหม่ที่แอปต่างๆ ใช้ร่วมกัน เลือกโฟลเดอร์ Suite42 เดิมเพื่อคัดลอกข้อมูลเหล่านี้ macOS จะขออนุญาตจากคุณเพียงครั้งเดียว",
            "zh-Hans": "App Store 版本将签名、印章和签名密钥保存在其各个应用共享的新资料库中。选择旧的 Suite42 文件夹以拷贝它们；macOS 只会请求一次权限。",
            "zh-Hant": "App Store 版本會將簽名、印章和簽署密鑰儲存在其各個 App 共用的新資料庫中。選擇舊的 Suite42 資料夾以拷貝它們；macOS 只會要求一次權限。",
            "ar": "يحفظ إصدار App Store التوقيعات والأختام ومفاتيح التوقيع في مكتبة جديدة تتشاركها تطبيقاته. اختر مجلد Suite42 القديم لنسخها؛ سيطلب macOS إذنك مرة واحدة.",
        ],
        "Choose the Suite42 folder of the previous version": [
            "ru": "Выберите папку Suite42 прежней версии", "tr": "Önceki sürümün Suite42 klasörünü seçin",
            "de": "Wählen Sie den Ordner „Suite42“ der vorherigen Version", "fr": "Choisissez le dossier Suite42 de la version précédente",
            "es": "Elige la carpeta Suite42 de la versión anterior", "pt": "Escolha a pasta Suite42 da versão anterior",
            "it": "Scegli la cartella Suite42 della versione precedente", "pl": "Wybierz folder Suite42 poprzedniej wersji",
            "hu": "Válassza ki az előző verzió Suite42 mappáját", "id": "Pilih folder Suite42 dari versi sebelumnya",
            "ms": "Pilih folder Suite42 versi sebelumnya", "vi": "Chọn thư mục Suite42 của phiên bản trước",
            "fil": "Piliin ang folder na Suite42 ng nakaraang bersyon", "ja": "以前のバージョンのSuite42フォルダを選択してください",
            "ko": "이전 버전의 Suite42 폴더를 선택하십시오", "th": "เลือกโฟลเดอร์ Suite42 ของเวอร์ชันก่อนหน้า",
            "zh-Hans": "选择旧版本的 Suite42 文件夹", "zh-Hant": "選擇舊版本的 Suite42 資料夾",
            "ar": "اختر مجلد Suite42 الخاص بالإصدار السابق",
        ],
        "Imported %d items.": [
            "ru": "Импортировано объектов: %d.", "tr": "%d öğe içe aktarıldı.", "de": "%d Objekte importiert.",
            "fr": "%d éléments importés.", "es": "Se importaron %d elementos.", "pt": "%d itens importados.",
            "it": "%d elementi importati.", "pl": "Zaimportowane elementy: %d.", "hu": "%d elem importálva.",
            "id": "%d item diimpor.", "ms": "%d item diimport.", "vi": "Đã nhập %d mục.", "fil": "Na-import ang %d item.",
            "ja": "%d 項目を読み込みました。", "ko": "항목 %d개를 가져왔습니다.", "th": "นำเข้าแล้ว %d รายการ",
            "zh-Hans": "已导入 %d 项。", "zh-Hant": "已匯入 %d 個項目。", "ar": "تم استيراد %d عنصرًا.",
        ],
        "This folder has no signatures, stamps or signing keys.": [
            "ru": "В этой папке нет подписей, печатей или ключей подписи.",
            "tr": "Bu klasörde imza, mühür veya imzalama anahtarı yok.",
            "de": "Dieser Ordner enthält keine Unterschriften, Stempel oder Signaturschlüssel.",
            "fr": "Ce dossier ne contient ni signatures, ni tampons, ni clés de signature.",
            "es": "Esta carpeta no contiene firmas, sellos ni claves de firma.",
            "pt": "Esta pasta não tem assinaturas, carimbos nem chaves de assinatura.",
            "it": "Questa cartella non contiene firme, timbri o chiavi di firma.",
            "pl": "Ten folder nie zawiera podpisów, pieczęci ani kluczy podpisu.",
            "hu": "Ebben a mappában nincs aláírás, bélyegző vagy aláírókulcs.",
            "id": "Folder ini tidak berisi tanda tangan, cap, atau kunci penandatanganan.",
            "ms": "Folder ini tiada tandatangan, cop atau kunci tandatangan.",
            "vi": "Thư mục này không có chữ ký, con dấu hay khóa ký.",
            "fil": "Walang lagda, selyo o susi sa pagpirma sa folder na ito.",
            "ja": "このフォルダには署名、印影、署名鍵がありません。",
            "ko": "이 폴더에는 서명, 도장 또는 서명 키가 없습니다.",
            "th": "โฟลเดอร์นี้ไม่มีลายเซ็น ตราประทับ หรือกุญแจลงนาม",
            "zh-Hans": "此文件夹中没有签名、印章或签名密钥。", "zh-Hant": "此資料夾中沒有簽名、印章或簽署密鑰。",
            "ar": "لا يحتوي هذا المجلد على توقيعات أو أختام أو مفاتيح توقيع.",
        ],
        "Not available in the App Store version": [
            "ru": "Недоступно в версии из App Store", "tr": "App Store sürümünde kullanılamaz",
            "de": "In der App-Store-Version nicht verfügbar", "fr": "Non disponible dans la version App Store",
            "es": "No disponible en la versión de la App Store", "pt": "Indisponível na versão da App Store",
            "it": "Non disponibile nella versione App Store", "pl": "Niedostępne w wersji z App Store",
            "hu": "Az App Store-verzióban nem érhető el", "id": "Tidak tersedia di versi App Store",
            "ms": "Tidak tersedia dalam versi App Store", "vi": "Không có trong phiên bản App Store",
            "fil": "Hindi available sa bersyon sa App Store", "ja": "App Store版では利用できません",
            "ko": "App Store 버전에서는 사용할 수 없음", "th": "ไม่มีในเวอร์ชัน App Store",
            "zh-Hans": "在 App Store 版本中不可用", "zh-Hant": "App Store 版本無法使用", "ar": "غير متاح في إصدار App Store",
        ],
        "Smart-card modules (PKCS#11) cannot be loaded in the App Store version. Smart cards that macOS supports directly appear in the Keychain certificate list.": [
            "ru": "Модули смарт-карт (PKCS#11) нельзя загрузить в версии из App Store. Смарт-карты, которые macOS поддерживает напрямую, появляются в списке сертификатов связки ключей.",
            "tr": "Akıllı kart modülleri (PKCS#11) App Store sürümünde yüklenemez. macOS'un doğrudan desteklediği akıllı kartlar Anahtar Zinciri sertifika listesinde görünür.",
            "de": "Smartcard-Module (PKCS#11) können in der App-Store-Version nicht geladen werden. Smartcards, die macOS direkt unterstützt, erscheinen in der Zertifikatsliste des Schlüsselbunds.",
            "fr": "Les modules de carte à puce (PKCS#11) ne peuvent pas être chargés dans la version App Store. Les cartes à puce prises en charge directement par macOS apparaissent dans la liste des certificats du trousseau.",
            "es": "Los módulos de tarjeta inteligente (PKCS#11) no se pueden cargar en la versión de la App Store. Las tarjetas inteligentes que macOS admite directamente aparecen en la lista de certificados del llavero.",
            "pt": "Os módulos de cartão inteligente (PKCS#11) não podem ser carregados na versão da App Store. Os cartões inteligentes que o macOS suporta diretamente aparecem na lista de certificados do Porta-chaves.",
            "it": "I moduli per smart card (PKCS#11) non possono essere caricati nella versione App Store. Le smart card supportate direttamente da macOS compaiono nell’elenco dei certificati del Portachiavi.",
            "pl": "Modułów kart inteligentnych (PKCS#11) nie można załadować w wersji z App Store. Karty inteligentne obsługiwane bezpośrednio przez macOS pojawiają się na liście certyfikatów pęku kluczy.",
            "hu": "Az intelligenskártya-modulok (PKCS#11) nem tölthetők be az App Store-verzióban. A macOS által közvetlenül támogatott intelligens kártyák a Kulcskarika tanúsítványlistájában jelennek meg.",
            "id": "Modul kartu pintar (PKCS#11) tidak dapat dimuat di versi App Store. Kartu pintar yang didukung langsung oleh macOS muncul di daftar sertifikat Rantai Kunci.",
            "ms": "Modul kad pintar (PKCS#11) tidak boleh dimuatkan dalam versi App Store. Kad pintar yang disokong terus oleh macOS dipaparkan dalam senarai sijil Rantai Kunci.",
            "vi": "Không thể tải mô-đun thẻ thông minh (PKCS#11) trong phiên bản App Store. Thẻ thông minh được macOS hỗ trợ trực tiếp sẽ xuất hiện trong danh sách chứng chỉ của Chuỗi khóa.",
            "fil": "Hindi maaaring i-load ang mga module ng smart card (PKCS#11) sa bersyon sa App Store. Lumalabas sa listahan ng mga certificate ng Keychain ang mga smart card na direktang sinusuportahan ng macOS.",
            "ja": "App Store版ではスマートカードモジュール（PKCS#11）を読み込めません。macOSが直接サポートするスマートカードは、キーチェーンの証明書リストに表示されます。",
            "ko": "App Store 버전에서는 스마트 카드 모듈(PKCS#11)을 불러올 수 없습니다. macOS가 직접 지원하는 스마트 카드는 키체인 인증서 목록에 표시됩니다.",
            "th": "ไม่สามารถโหลดโมดูลสมาร์ทการ์ด (PKCS#11) ในเวอร์ชัน App Store ได้ สมาร์ทการ์ดที่ macOS รองรับโดยตรงจะแสดงในรายการใบรับรองของพวงกุญแจ",
            "zh-Hans": "App Store 版本无法载入智能卡模块（PKCS#11）。macOS 直接支持的智能卡会显示在钥匙串证书列表中。",
            "zh-Hant": "App Store 版本無法載入智慧卡模組（PKCS#11）。macOS 直接支援的智慧卡會顯示在鑰匙圈的憑證列表中。",
            "ar": "لا يمكن تحميل وحدات البطاقات الذكية (PKCS#11) في إصدار App Store. تظهر البطاقات الذكية التي يدعمها macOS مباشرةً في قائمة شهادات سلسلة المفاتيح.",
        ],
        "CryptoPro CSP cannot be used in the App Store version: the App Sandbox does not allow running its command-line tools.": [
            "ru": "CryptoPro CSP нельзя использовать в версии из App Store: App Sandbox не позволяет запускать его инструменты командной строки.",
            "tr": "CryptoPro CSP App Store sürümünde kullanılamaz: App Sandbox, komut satırı araçlarının çalıştırılmasına izin vermez.",
            "de": "CryptoPro CSP kann in der App-Store-Version nicht verwendet werden: Die App Sandbox erlaubt nicht, seine Befehlszeilenwerkzeuge auszuführen.",
            "fr": "CryptoPro CSP ne peut pas être utilisé dans la version App Store : l’App Sandbox ne permet pas d’exécuter ses outils en ligne de commande.",
            "es": "CryptoPro CSP no se puede usar en la versión de la App Store: App Sandbox no permite ejecutar sus herramientas de línea de comandos.",
            "pt": "O CryptoPro CSP não pode ser usado na versão da App Store: o App Sandbox não permite executar suas ferramentas de linha de comando.",
            "it": "CryptoPro CSP non può essere usato nella versione App Store: App Sandbox non consente di eseguire i suoi strumenti da riga di comando.",
            "pl": "CryptoPro CSP nie może być używany w wersji z App Store: App Sandbox nie pozwala uruchamiać jego narzędzi wiersza poleceń.",
            "hu": "A CryptoPro CSP nem használható az App Store-verzióban: az App Sandbox nem engedi futtatni a parancssori eszközeit.",
            "id": "CryptoPro CSP tidak dapat digunakan di versi App Store: App Sandbox tidak mengizinkan menjalankan alat baris perintahnya.",
            "ms": "CryptoPro CSP tidak boleh digunakan dalam versi App Store: App Sandbox tidak membenarkan alat baris perintahnya dijalankan.",
            "vi": "Không thể dùng CryptoPro CSP trong phiên bản App Store: App Sandbox không cho phép chạy các công cụ dòng lệnh của nó.",
            "fil": "Hindi magagamit ang CryptoPro CSP sa bersyon sa App Store: hindi pinapayagan ng App Sandbox na patakbuhin ang mga command-line tool nito.",
            "ja": "App Store版ではCryptoPro CSPを使用できません。App Sandboxではそのコマンドラインツールを実行できないためです。",
            "ko": "App Store 버전에서는 CryptoPro CSP를 사용할 수 없습니다. App Sandbox에서 해당 명령줄 도구를 실행할 수 없기 때문입니다.",
            "th": "ไม่สามารถใช้ CryptoPro CSP ในเวอร์ชัน App Store ได้ เนื่องจาก App Sandbox ไม่อนุญาตให้เรียกใช้เครื่องมือบรรทัดคำสั่งของโปรแกรมนี้",
            "zh-Hans": "App Store 版本无法使用 CryptoPro CSP：App Sandbox 不允许运行其命令行工具。",
            "zh-Hant": "App Store 版本無法使用 CryptoPro CSP：App Sandbox 不允許執行其命令列工具。",
            "ar": "لا يمكن استخدام CryptoPro CSP في إصدار App Store: لا يسمح App Sandbox بتشغيل أدوات سطر الأوامر الخاصة به.",
        ],
    ]
}

/// Security-scoped bookmarks for the Open Recent menu and the Welcome window: access granted by an open panel or by
/// Finder lasts only until the app quits, so a bookmark is kept for each recent document while access is still held.
public enum SecurityScopedRecents {
    private static let key = "Suite42.recentBookmarks"
    private static var accessing = Set<String>()

    private static var store: [String: Data] {
        get { UserDefaults.standard.dictionary(forKey: key) as? [String: Data] ?? [:] }
        set { UserDefaults.standard.set(newValue, forKey: key) }
    }

    /// Adds bookmarks for recent documents that have none yet and drops those no longer in the list.
    public static func remember(_ urls: [URL]) {
        let paths = Set(urls.map(\.path))
        var bookmarks = store.filter { paths.contains($0.key) }
        for url in urls where bookmarks[url.path] == nil {
            if let data = try? url.bookmarkData(options: .withSecurityScope, includingResourceValuesForKeys: nil, relativeTo: nil) {
                bookmarks[url.path] = data
            }
        }
        if bookmarks != store { store = bookmarks }
    }

    /// Starts access to a recent document (kept until the app quits) before it is opened.
    public static func beginAccess(_ url: URL) {
        guard !accessing.contains(url.path), let data = store[url.path] else { return }
        var stale = false
        guard let resolved = try? URL(resolvingBookmarkData: data, options: .withSecurityScope, relativeTo: nil,
                                      bookmarkDataIsStale: &stale) else { return }
        if resolved.startAccessingSecurityScopedResource() { accessing.insert(url.path) }
        if stale, let fresh = try? resolved.bookmarkData(options: .withSecurityScope, includingResourceValuesForKeys: nil, relativeTo: nil) {
            var bookmarks = store
            bookmarks[url.path] = fresh
            store = bookmarks
        }
    }
}

/// Folders the app writes into later without asking again (SCN42's save folder, PDF42's batch-signing folder).
public enum SecurityScopedFolders {
    private static var open: [String: URL] = [:]

    /// Keeps a bookmark of a folder the user has just chosen in an open panel.
    public static func remember(_ url: URL, key: String) {
        guard let data = try? url.bookmarkData(options: .withSecurityScope, includingResourceValuesForKeys: nil, relativeTo: nil) else { return }
        UserDefaults.standard.set(data, forKey: "Suite42.folderBookmark." + key)
        open[key] = url
    }

    /// The folder at `path` with access started for this session when its bookmark is stored under `key`; otherwise
    /// the plain URL (writing there then fails with the sandbox's permission error).
    public static func url(path: String, key: String) -> URL {
        let plain = URL(fileURLWithPath: path, isDirectory: true)
        if let url = open[key], url.standardizedFileURL.path == plain.standardizedFileURL.path { return url }
        guard let data = UserDefaults.standard.data(forKey: "Suite42.folderBookmark." + key) else { return plain }
        var stale = false
        guard let url = try? URL(resolvingBookmarkData: data, options: .withSecurityScope, relativeTo: nil, bookmarkDataIsStale: &stale),
              url.standardizedFileURL.path == plain.standardizedFileURL.path else { return plain }
        if url.startAccessingSecurityScopedResource() { open[key] = url }
        if stale { remember(url, key: key) }
        return url
    }
}
#endif
