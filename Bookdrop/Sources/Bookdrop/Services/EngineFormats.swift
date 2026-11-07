import CAnyform
import Foundation

/// The formats the Rust engine reads and writes, and the options each one
/// reads, from `anyform_formats()`. The app builds its list of openable
/// files and its option controls from this, so a new engine format needs
/// no list to be kept in step here.
struct EngineFormats: Decodable {
    struct Input: Decodable {
        let name: String
        let extensions: [String]
        let options: [String]
    }

    struct Output: Decodable {
        let name: String
        let `extension`: String
        let options: [String]
        let companionSuffix: String?

        enum CodingKeys: String, CodingKey {
            case name, `extension`, options
            case companionSuffix = "companion_suffix"
        }
    }

    let inputs: [Input]
    let outputs: [Output]

    static let shared: EngineFormats = {
        guard let raw = anyform_formats() else { fatalError("anyform_formats returned null") }
        defer { anyform_free_string(raw) }
        do {
            return try JSONDecoder().decode(EngineFormats.self, from: Data(String(cString: raw).utf8))
        } catch {
            fatalError("the engine's format list did not decode: \(error)")
        }
    }()

    /// Every file extension an input plugin reads, in engine order.
    var inputExtensions: [String] { inputs.flatMap(\.extensions) }

    /// The options the output plugin for `fileExtension` reads.
    func options(forOutputExtension fileExtension: String) -> [String] {
        outputs.first { $0.extension == fileExtension }?.options ?? []
    }
}
