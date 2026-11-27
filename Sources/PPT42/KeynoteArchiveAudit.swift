import Foundation
import Kit42

/// Synthetic wire fixtures exercise boundaries without invoking Keynote or relying on user files.
enum KeynoteArchiveAudit {
    static func run() -> Bool {
        func archive(_ block: [UInt8], extra: String? = nil) -> Data {
            var zip = ZipArchive()
            zip["Index/Document.iwa"] = Data([0, UInt8(block.count), 0, 0] + block)
            if let extra { zip[extra] = Data() }
            return zip.data()
        }
        // Object 1, type 3, payload protobuf field 1 = 42.
        let wire: [UInt8] = [8, 8, 1, 18, 4, 8, 3, 24, 2, 8, 42]
        let literal = [UInt8(wire.count), UInt8((wire.count - 1) << 2)] + wire
        do {
            let repeated = try KeynoteArchive.decompress(Data([0, 5, 0, 0, 5, 0, 65, 1, 1]))
            guard repeated == Data("AAAAA".utf8) else { return false }
            let parsed = try KeynoteArchive(data: archive(literal))
            guard parsed.objects[1]?.type == 3, try parsed.objects[1]?.message().integer(1) == 42 else { return false }
            for bad in [Data(), archive([4, 1, 0]), archive(literal, extra: "../escape"), archive([255, 255, 255, 255, 255, 255, 255, 255, 255, 2])] {
                do { _ = try KeynoteArchive(data: bad); return false } catch { }
            }
            // Both streams individually fit, but their combined decoded size does not.
            var secondWire = wire; secondWire[2] = 2
            let secondLiteral = [UInt8(secondWire.count), UInt8((secondWire.count - 1) << 2)] + secondWire
            let streams = ["Index/Document.iwa": Data([0, UInt8(literal.count), 0, 0] + literal),
                           "Index/Slide.iwa": Data([0, UInt8(secondLiteral.count), 0, 0] + secondLiteral)]
            guard try KeynoteArchive(files: streams, maxDecodedBytes: wire.count * 2).objects.count == 2 else { return false }
            do { _ = try KeynoteArchive(files: streams, maxDecodedBytes: wire.count * 2 - 1); return false } catch KeynoteArchiveError.limit { }
            do { _ = try KeynoteMessage(Data([0x80])); return false } catch { }
            do { _ = try KeynoteMessage(Data([0x0b])); return false } catch { }
            print("Keynote archive wire checks passed")
            return true
        } catch { print("Keynote archive wire checks failed: \(error)"); return false }
    }
}
