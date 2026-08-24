import Foundation

struct HistoryEntry: Identifiable, Codable, Equatable {
    let id: UUID
    let title: String
    let conversionLabel: String
    let date: Date
    let outputPath: String
    let sourcePath: String

    init(
        id: UUID = UUID(), title: String, conversionLabel: String, date: Date = Date(),
        outputPath: String, sourcePath: String
    ) {
        self.id = id
        self.title = title
        self.conversionLabel = conversionLabel
        self.date = date
        self.outputPath = outputPath
        self.sourcePath = sourcePath
    }
}
