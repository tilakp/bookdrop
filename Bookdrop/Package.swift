// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Bookdrop",
    platforms: [
        .macOS(.v13)
    ],
    products: [
        .executable(name: "Bookdrop", targets: ["Bookdrop"])
    ],
    dependencies: [
        .package(url: "https://github.com/weichsel/ZIPFoundation.git", from: "0.9.0")
    ],
    targets: [
        .executableTarget(
            name: "Bookdrop",
            dependencies: [
                .product(name: "ZIPFoundation", package: "ZIPFoundation")
            ],
            path: "Sources/Bookdrop"
        ),
        .testTarget(
            name: "BookdropTests",
            dependencies: ["Bookdrop"],
            path: "Tests/BookdropTests",
            resources: [
                .copy("Fixtures")
            ]
        )
    ]
)
