// swift-tools-version: 6.2

import PackageDescription
import Foundation

let repository = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().path

let package = Package(
    name: "WireboltApple",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "WireboltKit", targets: ["WireboltKit"]),
    ],
    targets: [
        .systemLibrary(name: "wirebolt_ffiFFI", path: "BridgeFFI"),
        .target(name: "WireboltBridge", dependencies: ["wirebolt_ffiFFI"], path: "Generated", sources: ["wirebolt_ffi.swift"],
            linkerSettings: [.unsafeFlags(["\(repository)/target/release/libwirebolt_ffi.a"]), .linkedLibrary("icucore"), .linkedFramework("Security"), .linkedFramework("SystemConfiguration")]),
        .target(name: "WireboltKit", dependencies: ["WireboltBridge"]),
        .testTarget(name: "WireboltKitTests", dependencies: ["WireboltKit"]),
    ]
)
