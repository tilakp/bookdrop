// swift-tools-version:5.9
import Foundation
import PackageDescription

// The prebuilt Rust engine: rust/scripts/build-ffi.sh writes
// rust/target/universal/libanyform_ffi.a, linked here with plain linker
// flags (a C target plus -l is simpler than a .binaryTarget for one static
// library). The path is relative to this manifest, so `swift build` works
// from any directory.
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
    targets: [
        .target(
            name: "CAnyform",
            path: "Sources/CAnyform"
        ),
        .executableTarget(
            name: "Bookdrop",
            dependencies: ["CAnyform"],
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
