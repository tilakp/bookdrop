import Foundation

enum AppScreen {
    case empty
    case loaded(Book)
    case duplicateConfirm(book: Book, filename: String)
    case converting(book: Book, progress: ConversionProgress)
    case complete(CompletionInfo)
    case error(message: String, hint: String?, technicalDetails: String?)
    case multipleFiles(MultiConversionModel)
}
