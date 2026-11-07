import Foundation

/// An error the Rust engine reported. `code` is `ConvError::code()` in
/// `anyform-core` (or "invalid_argument" / "internal" from the FFI layer).
/// The engine says what went wrong; this type adds what the user can do.
struct EngineError: LocalizedError, Equatable, Decodable {
    let code: String
    let message: String

    static let cancelled = EngineError(code: "cancelled", message: "Conversion was cancelled.")

    var isCancellation: Bool { code == "cancelled" }

    var errorDescription: String? { message }

    /// Advice for the codes where the fix is known. `nil` means the caller
    /// should use its own, more general hint.
    var recoverySuggestion: String? {
        switch code {
        case "drm_protected":
            return "Kindle books bought from Amazon are encrypted. Use a DRM-free copy of the book."
        case "password_protected":
            return "Open the PDF with its password, save an unprotected copy, and convert that."
        case "scanned_pdf":
            return "Bookdrop can't read text from images yet. Use a PDF with selectable text."
        case "multi_column_pdf":
            return "Converting it would put the text in the wrong order."
        case "tool_missing":
            return "Part of Bookdrop is missing. Install Bookdrop again."
        case "output_exists":
            return "Rename or move that folder, or choose another folder to save to."
        default:
            return nil
        }
    }

    /// Decodes the engine's `{"status":"error","code":…,"message":…}`
    /// payload. Unexpected text still reaches the user as the message.
    static func decode(_ json: String?) -> EngineError {
        guard let json else { return EngineError(code: "internal", message: "The conversion failed.") }
        return (try? JSONDecoder().decode(EngineError.self, from: Data(json.utf8)))
            ?? EngineError(code: "internal", message: json)
    }
}
