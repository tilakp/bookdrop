// swift-tools-version:5.9
import Foundation
import PackageDescription

// The prebuilt Rust engine (see rust/README.md) — built by
// rust/scripts/build-ffi.sh into rust/target/universal/libanyform_ffi.a,
// linked directly (not via .binaryTarget/.xcframework — a plain C target +
// linker flags proved simpler and more robust for a static lib with a
// hand-written C ABI, see plan Phase 4). Resolved from this manifest's own
// path so it works regardless of the working directory `swift build` is
// invoked from.
let packageDir = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
let rustLibDir = packageDir.appendingPathComponent("rust/target/universal").path

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
        .target(
            name: "CAnyform",
            path: "Sources/CAnyform"
        ),
        .executableTarget(
            name: "Bookdrop",
            dependencies: [
                .product(name: "ZIPFoundation", package: "ZIPFoundation"),
                "CAnyform",
            ],
            path: "Sources/Bookdrop",
            linkerSettings: [
                .unsafeFlags(["-L", rustLibDir, "-lanyform_ffi"]),
                .linkedFramework("Security"),
                .linkedFramework("CoreFoundation"),
                .linkedLibrary("iconv"),
                .linkedLibrary("resolv"),
            ]
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
