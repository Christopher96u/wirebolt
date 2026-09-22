import Foundation
import Testing
@testable import WireboltKit

struct WebSocketBufferTests {
    @Test func burstPreservesOrderWithBackpressure() async throws {
        let buffer = WebSocketEventBuffer(byteLimit: 256, eventLimit: 2)
        let producer = Task.detached {
            for i in 0..<1000 {
                guard buffer.push(.message(Data(String(i).utf8), binary: false, outgoing: false)) else { return false }
            }
            buffer.finish()
            return true
        }
        var received: [String] = []
        while let event = try await buffer.next() {
            if case let .message(data, _, _, _) = event { received.append(String(decoding: data, as: UTF8.self)) }
        }
        #expect(await producer.value)
        #expect(received == (0..<1000).map(String.init))
    }
    @Test func cancellationWakesBlockedProducerAndConsumer() async throws {
        let buffer = WebSocketEventBuffer(byteLimit: 128, eventLimit: 1)
        #expect(buffer.push(.message(Data([1]), binary: true, outgoing: false)))
        let producer = Task.detached { buffer.push(.message(Data([2]), binary: true, outgoing: false)) }
        try await Task.sleep(for: .milliseconds(20))
        buffer.finish(CancellationError(), discardingPending: true)
        #expect(await producer.value == false)
        await #expect(throws: CancellationError.self) { try await buffer.next() }
        let empty = WebSocketEventBuffer()
        let consumer = Task { try await empty.next() }
        try await Task.sleep(for: .milliseconds(20))
        empty.finish(CancellationError(), discardingPending: true)
        await #expect(throws: CancellationError.self) { try await consumer.value }
    }
    @Test func oversizedEventFailsWithoutWaitingForever() async {
        let buffer = WebSocketEventBuffer(byteLimit: 64)
        #expect(!buffer.push(.message(Data([1]), binary: true, outgoing: false)))
        await #expect(throws: WebSocketUIError.self) { try await buffer.next() }
    }
}
