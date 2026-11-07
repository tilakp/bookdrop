import Foundation

enum AppScreen {
    case empty
    case loading(filename: String)
    case loaded(Book)
    case duplicateConfirm(book: Book, filename: String)
    case converting(book: Book, progress: ConversionProgress)
    case complete(CompletionInfo)
    case error(message: String, hint: String?, technicalDetails: String?)
    case multipleFiles(MultiConversionModel)
}

/// The answer to "a file with this name already exists".
enum DuplicateResolution {
    case replace
    case keepBoth
    case cancel
}
