import Foundation

/// UTF-16 ranges match TextKit's selection and edit coordinates.
public struct CodeFold: Equatable, Sendable {
    public let line: Int
    public let range: NSRange
}

public struct CodeProjection: Sendable {
    public enum Syntax: Sendable { case json, xml, html, none }
    public let source: String
    public let text: String
    public let folds: [CodeFold]
    public let collapsed: Set<Int>
    private let replacements: [(source: NSRange, display: NSRange)]

    public init(source: String, collapsed: Set<Int> = [], syntax: Syntax = .json) {
        self.source = source
        switch syntax {
        case .json: folds = Self.folds(in: source)
        case .xml, .html: folds = Self.markupFolds(in: source, html: syntax == .html)
        case .none: folds = []
        }
        self.collapsed = collapsed
        if collapsed.isEmpty {
            text = source
            replacements = []
            return
        }
        let string = source as NSString
        var output = ""
        var cursor = 0
        var replacements: [(source: NSRange, display: NSRange)] = []
        for fold in folds where collapsed.contains(fold.line) && fold.range.location >= cursor {
            output += string.substring(with: NSRange(location: cursor, length: fold.range.location - cursor))
            replacements.append((fold.range, NSRange(location: (output as NSString).length, length: 1)))
            output += "…"
            cursor = NSMaxRange(fold.range)
        }
        output += string.substring(from: cursor)
        text = output
        self.replacements = replacements
    }

    public func sourceOffset(_ display: Int, afterPlaceholder: Bool = false) -> Int {
        var delta = 0
        for replacement in replacements {
            if display < replacement.display.location { break }
            if display < NSMaxRange(replacement.display) {
                return afterPlaceholder ? NSMaxRange(replacement.source) : replacement.source.location
            }
            delta += replacement.source.length - replacement.display.length
        }
        return min((source as NSString).length, max(0, display + delta))
    }

    public func displayOffset(_ source: Int) -> Int {
        var delta = 0
        for replacement in replacements {
            if source < replacement.source.location { break }
            if source < NSMaxRange(replacement.source) { return replacement.display.location }
            delta += replacement.source.length - replacement.display.length
        }
        return max(0, source - delta)
    }

    /// Edits outside a folded range preserve its entire hidden contents.
    public func replacing(displayRange: NSRange, with replacement: String) -> String {
        let start = sourceOffset(displayRange.location)
        let end = sourceOffset(NSMaxRange(displayRange), afterPlaceholder: displayRange.length > 0)
        return (source as NSString).replacingCharacters(in: NSRange(location: start, length: max(0, end - start)), with: replacement)
    }

    private static func folds(in text: String) -> [CodeFold] {
        let units = Array(text.utf16)
        var stack: [(character: UInt16, offset: Int, line: Int)] = []
        var quoted = false
        var escaped = false
        var line = 0
        var result: [CodeFold] = []
        for (offset, unit) in units.enumerated() {
            if unit == 10 { line += 1 }
            if quoted {
                if escaped { escaped = false }
                else if unit == 92 { escaped = true }
                else if unit == 34 { quoted = false }
                continue
            }
            if unit == 34 { quoted = true; continue }
            if unit == 123 || unit == 91 { stack.append((unit, offset, line)) }
            else if unit == 125 || unit == 93, let start = stack.last,
                    (start.character == 123 && unit == 125) || (start.character == 91 && unit == 93) {
                stack.removeLast()
                if line > start.line {
                    result.append(CodeFold(line: start.line, range: NSRange(location: start.offset + 1, length: offset - start.offset - 1)))
                }
            }
        }
        return result.sorted { $0.range.location < $1.range.location }
    }

    private static func markupFolds(in text: String, html: Bool) -> [CodeFold] {
        let source = text as NSString
        let units = Array(text.utf16)
        let voidTags: Set<String> = ["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"]
        var stack: [(name: String, end: Int, line: Int)] = []
        var result: [CodeFold] = []
        var offset = 0, line = 0
        func has(_ token: String, at index: Int) -> Bool {
            let token = Array(token.utf16)
            return index + token.count <= units.count && units[index..<(index + token.count)].elementsEqual(token)
        }
        func append(start: Int, end: Int, startLine: Int) {
            guard end > start, units[start..<end].contains(10) else { return }
            result.append(CodeFold(line: startLine, range: NSRange(location: start, length: end - start)))
        }
        while offset < units.count {
            guard units[offset] == 60 else {
                if units[offset] == 10 { line += 1 }
                offset += 1
                continue
            }
            let start = offset, startLine = line
            if has("<!--", at: start) || has("<![CDATA[", at: start) {
                let comment = has("<!--", at: start)
                let opener = comment ? 4 : 9, closer = comment ? "-->" : "]]>"
                offset += opener
                while offset < units.count, !has(closer, at: offset) {
                    if units[offset] == 10 { line += 1 }
                    offset += 1
                }
                if offset < units.count {
                    append(start: start + opener, end: offset, startLine: startLine)
                    offset += 3
                }
                continue
            }
            var end = start + 1
            var quote: UInt16?
            while end < units.count {
                let unit = units[end]
                if let current = quote { if unit == current { quote = nil } }
                else if unit == 34 || unit == 39 { quote = unit }
                else if unit == 62 { break }
                end += 1
            }
            guard end < units.count else { break }
            line += units[start...end].filter { $0 == 10 }.count
            offset = end + 1
            let closing = units[start + 1] == 47
            let nameStart = start + (closing ? 2 : 1)
            var nameEnd = nameStart
            while nameEnd < end, ![UInt16(9), 10, 13, 32, 47].contains(units[nameEnd]) { nameEnd += 1 }
            guard nameEnd > nameStart, units[nameStart] != 33, units[nameStart] != 63 else { continue }
            let rawName = source.substring(with: NSRange(location: nameStart, length: nameEnd - nameStart))
            let name = html ? rawName.lowercased() : rawName
            if closing {
                guard let match = stack.lastIndex(where: { $0.name == name }) else { continue }
                let opened = stack[match]
                append(start: opened.end, end: start, startLine: opened.line)
                stack.removeSubrange(match...)
            } else if units[end - 1] != 47 && !(html && voidTags.contains(name)) {
                stack.append((name, end + 1, startLine))
                // Raw-text elements may contain angle brackets that are not tags.
                if html && ["script", "style", "textarea", "title"].contains(name) {
                    let close = source.range(of: "</" + name, options: .caseInsensitive,
                        range: NSRange(location: offset, length: units.count - offset))
                    if close.location != NSNotFound {
                        line += units[offset..<close.location].filter { $0 == 10 }.count
                        offset = close.location
                    }
                }
            }
        }
        return result.sorted { $0.range.location < $1.range.location }
    }
}
