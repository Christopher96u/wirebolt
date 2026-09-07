import Foundation
import CoreFoundation

struct JSONNode: Identifiable, Sendable {
    let id: String
    let key: String
    let type: String
    let value: String
    let children: [JSONNode]?

    static func makeRoot(from text: String) -> [JSONNode] {
        makeRoot(from: Data(text.utf8))
    }

    static func makeRoot(from data: Data) -> [JSONNode] {
        guard let object = try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
        else {
            return []
        }
        return [make(key: "Root", value: object, path: "root")]
    }

    private static func make(key: String, value: Any, path: String) -> JSONNode {
        if let dictionary = value as? NSDictionary {
            let children = dictionary.allKeys.compactMap { $0 as? String }.map {
                make(key: $0, value: dictionary[$0]!, path: "\(path)/\($0.replacingOccurrences(of: "~", with: "~0").replacingOccurrences(of: "/", with: "~1"))")
            }
            return JSONNode(
                id: path,
                key: key,
                type: "Object",
                value: "Object(\(children.count) items)",
                children: children
            )
        }
        if let array = value as? [Any] {
            let children = array.enumerated().map {
                make(key: "\($0.offset)", value: $0.element, path: "\(path)/\($0.offset)")
            }
            return JSONNode(
                id: path,
                key: key,
                type: "Array",
                value: "Array(\(children.count) items)",
                children: children
            )
        }
        if value is NSNull {
            return JSONNode(id: path, key: key, type: "Null", value: "null", children: nil)
        }
        if let number = value as? NSNumber {
            if CFGetTypeID(number) == CFBooleanGetTypeID() {
                return JSONNode(id: path, key: key, type: "Boolean", value: number.boolValue ? "true" : "false", children: nil)
            }
            return JSONNode(id: path, key: key, type: "Number", value: number.stringValue, children: nil)
        }
        return JSONNode(id: path, key: key, type: "String", value: String(describing: value), children: nil)
    }

}
