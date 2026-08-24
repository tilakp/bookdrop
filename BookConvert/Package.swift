// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "BookConvert",
    platforms: [
        .macOS(.v13)
    ],
    products: [
        .executable(name: "BookConvert", targets: ["BookConvert"])
    ],
    dependencies: [
        .package(url: "https://github.com/weichsel/ZIPFoundation.git", from: "0.9.0")
    ],
    targets: [
        .executableTarget(
            name: "BookConvert",
            dependencies: [
                .product(name: "ZIPFoundation", package: "ZIPFoundation")
            ],
            path: "Sources/BookConvert"
        ),
        .testTarget(
            name: "BookConvertTests",
            dependencies: ["BookConvert"],
            path: "Tests/BookConvertTests",
            resources: [
                .copy("Fixtures")
            ]
        )
    ]
)
