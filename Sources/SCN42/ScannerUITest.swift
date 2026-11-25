import AppKit
import Kit42

extension ScanWindowController {
    @MainActor func auditScanner(folder: URL) async -> Bool {
        var count = 0, failures = 0
        func check(_ value: Bool, _ name: String) { count += 1; if !value { failures += 1 }; print(value ? "PASS" : "FAIL", "SCN42 scanner:", name); fflush(stdout) }
        guard let window, let image = Bitmap(width: 160, height: 240, channels: 1, fill: 150).cgImage() else { return false }
        var plain = ScanSettings(); plain.autoCrop = false; plain.deskew = false; plain.autoRotate = false; plain.enhancement = .none; plain.resolution = 100
        let device = AuditScanner(); choose(device); settingsPanel.apply(plain)
        areas = [CGRect(x: 0.1, y: 0.2, width: 0.5, height: 0.6)]
        settingsPanel.scanButton.performClick(nil)
        check(isScanning && device.requests.count == 1, "native Scan button starts exactly one device request")
        let requestedArea = device.requests.first?.area ?? .null, expectedArea = areas[0]
        let matchingArea = zip([requestedArea.minX, requestedArea.minY, requestedArea.width, requestedArea.height],
                               [expectedArea.minX, expectedArea.minY, expectedArea.width, expectedArea.height]).allSatisfy { abs($0 - $1) < 1e-9 }
        check(matchingArea && device.requests.first?.resolution == settings.resolution,
              "request uses the visible resolution and selected glass area")
        if !matchingArea || device.requests.first?.resolution != settings.resolution {
            print("SCN42 scanner request diagnostic:", String(describing: device.requests.first), "areas:", areas, "resolution:", settings.resolution)
        }
        check(!canScan && !settingsPanel.scanButton.isEnabled && validate(#selector(cancelScanAction(_:))), "active scan disables duplicate scans and enables Stop")
        let gate = DispatchSemaphore(value: 0)
        processQueue.async { gate.wait() }
        device.page?(RawScan(image: image, dpi: 100)); device.done?(nil)
        check(!isScanning && pendingProcessing == 1 && !canScan, "scanner completion keeps a new scan disabled until processing finishes")
        check(!windowShouldClose(window), "Close cannot discard a page waiting in the processing queue")
        gate.signal()
        check(await UITest.wait { self.pendingProcessing == 0 && self.pages.count == 1 }, "processed scan arrives in the document")
        check(pages.first?.image.width == 160 && pages.first?.dpi == 100 && doc.isDirty, "processed page retains actual pixels and device DPI")
        check(canScan, "scan controls become ready after processing")

        // Editing an existing page has a separate generation from pages already received by the scanner.
        pageList.select([0]); let beforeConcurrent = pages.count
        let processingGate = DispatchSemaphore(value: 0); processQueue.async { processingGate.wait() }
        process(RawScan(image: image, dpi: 100), plain.processing, removeBlank: false)
        enhanceToggleAction(nil)
        processingGate.signal()
        check(await UITest.wait({ self.pendingProcessing == 0 && self.busyText == nil }, seconds: 20)
              && pages.count == beforeConcurrent + 1, "editing an existing page cannot discard a scan already queued for processing")

        areas = [CGRect(x: 0, y: 0, width: 0.4, height: 1), CGRect(x: 0.6, y: 0, width: 0.4, height: 1)]
        let startCount = pages.count, requests = device.requests.count
        scanAction(nil)
        let latePage = device.page, lateDone = device.done
        cancelScanAction(nil)
        check(!isScanning && device.cancellationCount == 1, "Stop cancels the current device operation")
        latePage?(RawScan(image: image, dpi: 100)); lateDone?(nil)
        try? await Task.sleep(nanoseconds: 80_000_000)
        check(pages.count == startCount && device.requests.count == requests + 1 && pendingProcessing == 0,
              "late page/completion after Stop cannot add pages or start the next area")
        areas = []
        var overviewResult: Bool?
        runOverview { overviewResult = $0 }
        let lateOverview = device.overviewDone
        check(isScanning && showingGlass, "Overview enters an active glass operation")
        cancelScanAction(nil); lateOverview?(.success(image))
        check(overviewResult == false && overview == nil && !isScanning, "cancelled overview cannot restore stale glass pixels")
        runOverview { overviewResult = $0 }; device.overviewDone?(.success(image))
        check(overviewResult == true && overview === image && preview.canvas.editsAreas, "successful Overview displays editable scanner glass")

        runOverview { overviewResult = $0 }
        settingsPanel.source.selectedSegment = 1
        if let selector = settingsPanel.source.action { _ = settingsPanel.source.sendAction(selector, to: settingsPanel.source.target) }
        check(isScanning && settings.source == .feeder && overview == nil,
              "source change during Overview invalidates pixels while retaining the active job")
        device.overviewDone?(.success(image))
        check(overviewResult == false && !isScanning && overview == nil && preparedSource == .feeder && canScan,
              "obsolete Overview completion releases its job and prepares the newly selected source")
        settingsPanel.source.selectedSegment = 0
        if let selector = settingsPanel.source.action { _ = settingsPanel.source.sendAction(selector, to: settingsPanel.source.target) }
        scanAction(nil)
        settingsPanel.source.selectedSegment = 1
        if let selector = settingsPanel.source.action { _ = settingsPanel.source.sendAction(selector, to: settingsPanel.source.target) }
        device.done?(nil)
        check(!isScanning && preparedSource == .feeder && settingsPanel.resolution.isEnabled && canScan,
              "scan completion prepares a source selected while the acquisition was running")
        settingsPanel.source.selectedSegment = 0
        if let selector = settingsPanel.source.action { _ = settingsPanel.source.sendAction(selector, to: settingsPanel.source.target) }

        var photos = plain; photos.detectPhotos = true
        settingsPanel.apply(photos); overview = nil; areaQuads = []
        let beforeDetection = device.requests.count
        scanAction(nil); device.overviewDone?(.success(image))
        check(busyText != nil, "photo scan waits for real photo detection")
        clearAreasAction(nil)
        check(await UITest.wait({ self.busyText == nil }, seconds: 20) && device.requests.count == beforeDetection,
              "invalidated photo detection cannot start a stale automatic scan")
        settingsPanel.apply(photos); settingsChanged(); overview = nil
        scanAction(nil); device.overviewDone?(.success(image))
        settingsPanel.source.selectedSegment = 1
        if let selector = settingsPanel.source.action { _ = settingsPanel.source.sendAction(selector, to: settingsPanel.source.target) }
        check(settings.source == .feeder && overview == nil && areas.isEmpty && areaQuads.isEmpty,
              "changing source clears obsolete overview pixels and scan regions")
        check(await UITest.wait({ self.busyText == nil }, seconds: 20) && device.requests.count == beforeDetection && overview == nil && areas.isEmpty,
              "late glass detection cannot repopulate areas or start scanning after a source change")
        settingsPanel.apply(plain); settingsChanged()

        pageList.select([0]); insertHereAction(nil)
        let anchor = pages[0].id
        let extra = ScanPage(base: image, image: image, dpi: 100, options: plain.processing, report: ProcessReport())
        doc.insert([extra], at: 0)
        check(insertionIndex == pages.firstIndex { $0.id == anchor }, "insert-next marker follows its original page when earlier pages arrive")
        doc.move(IndexSet(integer: 1), to: 0)
        check(insertionIndex == 0 && pageList.insertionIndex == 0, "insert-next marker follows page reordering")
        doc.remove(ids: [anchor]); check(insertionIndex == nil, "deleting the insertion anchor clears its marker")

        // The virtual source performs asynchronous acquisition and pixel transformations, including cancellation.
        do {
            let virtualFolder = folder.appendingPathComponent("virtual")
            try FileManager.default.createDirectory(at: virtualFolder, withIntermediateDirectories: true)
            let gray = Bitmap(width: 400, height: 600, channels: 1, fill: 180).cgImage()!
            for name in ["glass-1", "glass-2", "feed-01a", "feed-01b", "feed-02a", "feed-03a", "feed-03b"] {
                try TestImages.png(gray, dpi: 200).write(to: virtualFolder.appendingPathComponent(name + ".png"))
            }
            setenv("K42_FAKE_SPEED", "0.03", 1)
            let virtual = FakeScanner(folder: virtualFolder)
            virtual.open(); check(await UITest.wait { virtual.status == .ready }, "virtual scanner connects asynchronously")
            var delivered: [RawScan] = [], error: Error?, finished = false
            var request = ScanRequest(source: .feeder, resolution: 100, colorMode: .gray, bitDepth: 8,
                                      area: CGRect(x: 0, y: 0, width: 1, height: 1), paper: nil, duplex: false)
            virtual.scan(request, page: { delivered.append($0) }, done: { error = $0; finished = true })
            check(await UITest.wait({ finished }) && error == nil && delivered.count == 3,
                  "simplex skips named backs while retaining every unpaired front")
            check(delivered.allSatisfy { $0.image.width == 200 && $0.image.height == 300 && $0.dpi == 100 && $0.image.colorSpace?.model == .monochrome },
                  "virtual acquisition applies resolution, dimensions and greyscale")
            delivered = []; error = nil; finished = false; request.duplex = true
            virtual.scan(request, page: { delivered.append($0) }, done: { error = $0; finished = true })
            check(await UITest.wait({ finished }) && error == nil && delivered.count == 5, "duplex delivers all front and back pages")
            delivered = []; error = nil; finished = false; request.source = .flatbed
            request.area = CGRect(x: 0.25, y: 0.2, width: 0.5, height: 0.4)
            virtual.scan(request, page: { delivered.append($0) }, done: { error = $0; finished = true })
            check(await UITest.wait({ finished }) && error == nil && delivered.first?.image.width == 100 && delivered.first?.image.height == 120,
                  "flatbed acquisition crops the selected region before resampling")
            delivered = []; error = nil; finished = false
            var completions = 0
            virtual.scan(request, page: { delivered.append($0) }, done: { error = $0; finished = true; completions += 1 })
            virtual.cancel()
            try? await Task.sleep(nanoseconds: 150_000_000)
            check(finished && error != nil && completions == 1 && delivered.isEmpty && virtual.status == .ready,
                  "Stop during acquisition completes once and suppresses delayed pages")
            var overviewCompletions = 0
            virtual.overview { _ in overviewCompletions += 1 }; virtual.cancel()
            try? await Task.sleep(nanoseconds: 150_000_000)
            check(overviewCompletions == 1 && virtual.status == .ready, "Stop during Overview also completes once")
            virtual.close(); virtual.open(); virtual.close()
            try? await Task.sleep(nanoseconds: 250_000_000)
            check(virtual.status == .offline, "late connection callback cannot reopen a closed scanner")

            setenv("K42_FAKE_WARMUP", "1", 1)
            let warming = FakeScanner(folder: virtualFolder); warming.open()
            _ = await UITest.wait { warming.status == .ready }
            delivered = []; completions = 0
            warming.scan(request, page: { delivered.append($0) }, done: { _ in completions += 1 })
            check(warming.status == .warmingUp, "warm-up has a visible device state")
            warming.cancel()
            try? await Task.sleep(nanoseconds: 1_700_000_000)
            check(completions == 1 && delivered.isEmpty && warming.status == .ready, "cancelling warm-up prevents the later scan from starting")
            warming.close(); unsetenv("K42_FAKE_WARMUP")
            let brokenFolder = folder.appendingPathComponent("broken-scanner")
            try FileManager.default.createDirectory(at: brokenFolder, withIntermediateDirectories: true)
            try Data("broken".utf8).write(to: brokenFolder.appendingPathComponent("feed-1.png"))
            let broken = FakeScanner(folder: brokenFolder); broken.open(); _ = await UITest.wait { broken.status == .ready }
            request.source = .feeder; finished = false; error = nil; delivered = []
            broken.scan(request, page: { delivered.append($0) }, done: { error = $0; finished = true })
            check(await UITest.wait({ finished }) && error != nil && delivered.isEmpty, "unreadable scanner data reports failure rather than a successful empty scan")
            broken.close()
        } catch { check(false, "virtual acquisition fixture: \(error)") }
        check(ScannerStatus.scanning(.nan).text == L("Scanning…") && ScannerStatus.scanning(.infinity).text == L("Scanning…"), "invalid driver progress cannot overflow the status label")
        print("SCN42 scanner regressions: \(count - failures)/\(count) passed")
        return failures == 0
    }
}
