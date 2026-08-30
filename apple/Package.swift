// swift-tools-version: 6.2

import PackageDescription

let package = Package(
    name: "WireboltApple",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "WireboltKit", targets: ["WireboltKit"]),
    ],
    targets: [
        .target(name: "WireboltKit"),
        .testTarget(name: "WireboltKitTests", dependencies: ["WireboltKit"]),
    ]
)
