// swift-tools-version: 6.2

import Foundation
import PackageDescription

let root = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().path
let package = Package(
    name: "WireboltApple",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "WireboltKit", targets: ["WireboltKit"]),
    ],
    targets: [
        .target(
            name: "WireboltKit", path: ".",
            exclude: ["Tests", "Benchmarks", "WireboltApp", "Generated/wirebolt_ffiFFI.h", "Generated/wirebolt_ffiFFI.modulemap"],
            sources: ["Sources/WireboltKit", "Generated/wirebolt_ffi.swift"],
            swiftSettings: [.unsafeFlags(["-Xcc", "-fmodule-map-file=\(root)/apple/Generated/wirebolt_ffiFFI.modulemap"])],
            linkerSettings: [
                .linkedLibrary("icucore"), .linkedFramework("Security"), .linkedFramework("SystemConfiguration"),
                .unsafeFlags(["\(root)/target/release/libwirebolt_ffi.a"]),
            ]
        ),
        .testTarget(name: "WireboltKitTests", dependencies: ["WireboltKit"],
                    swiftSettings: [.unsafeFlags(["-Xcc", "-fmodule-map-file=\(root)/apple/Generated/wirebolt_ffiFFI.modulemap"])]),
    ]
)
