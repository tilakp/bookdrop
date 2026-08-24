import Foundation

@MainActor
final class ConversionProgress: ObservableObject {
    @Published var fraction: Double = 0
    @Published var stageText: String = "Reading book…"
    @Published private(set) var isCancelled = false

    func cancel() {
        isCancelled = true
    }
}
