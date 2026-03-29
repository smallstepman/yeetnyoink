import Foundation

private let desktopSpaceType: Int32 = 0
private let fullscreenSpaceType: Int32 = 4

public enum SpaceKind: Int32, Equatable {
    case desktop = 0
    case fullscreen = 1
    case splitView = 2
    case system = 3
    case stageManagerOpaque = 4
}

public struct NativeBounds: Equatable {
    public let x: Int32
    public let y: Int32
    public let width: Int32
    public let height: Int32

    public init(x: Int32, y: Int32, width: Int32, height: Int32) {
        self.x = x
        self.y = y
        self.width = width
        self.height = height
    }
}

public struct DesktopSpaceSnapshot: Equatable {
    public let id: UInt64
    public let displayIndex: Int
    public let active: Bool
    public let kind: SpaceKind

    public init(id: UInt64, displayIndex: Int, active: Bool, kind: SpaceKind) {
        self.id = id
        self.displayIndex = displayIndex
        self.active = active
        self.kind = kind
    }
}

public struct DesktopWindowSnapshot: Equatable {
    public let id: UInt64
    public let pid: UInt32?
    public let appID: String?
    public let title: String?
    public let bounds: NativeBounds?
    public let level: Int32
    public let spaceID: UInt64
    public let orderIndex: Int?

    public init(
        id: UInt64,
        pid: UInt32?,
        appID: String?,
        title: String?,
        bounds: NativeBounds?,
        level: Int32,
        spaceID: UInt64,
        orderIndex: Int?
    ) {
        self.id = id
        self.pid = pid
        self.appID = appID
        self.title = title
        self.bounds = bounds
        self.level = level
        self.spaceID = spaceID
        self.orderIndex = orderIndex
    }
}

public struct DesktopSnapshot: Equatable {
    public let spaces: [DesktopSpaceSnapshot]
    public let windows: [DesktopWindowSnapshot]
    public let focusedWindowID: UInt64?

    public init(spaces: [DesktopSpaceSnapshot], windows: [DesktopWindowSnapshot], focusedWindowID: UInt64?) {
        self.spaces = spaces
        self.windows = windows
        self.focusedWindowID = focusedWindowID
    }
}

public struct SystemWindowDescription: Equatable {
    public let id: UInt64
    public let pid: UInt32?
    public var appID: String?
    public let title: String?
    public let level: Int32
    public var visibleIndex: Int?
    public let frame: NativeBounds?

    public init(
        id: UInt64,
        pid: UInt32?,
        appID: String? = nil,
        title: String?,
        level: Int32,
        visibleIndex: Int? = nil,
        frame: NativeBounds?
    ) {
        self.id = id
        self.pid = pid
        self.appID = appID
        self.title = title
        self.level = level
        self.visibleIndex = visibleIndex
        self.frame = frame
    }
}

public protocol BackendSystem {
    func hasSymbol(_ symbol: String) -> Bool
    func isAccessibilityTrusted() -> Bool
    func mainConnectionID() -> UInt32?
    func managedDisplaySpaces() throws -> [[String: Any]]
    func windowsForSpace(_ spaceID: UInt64) throws -> [UInt64]
    func windowDescriptions(for windowIDs: [UInt64]) throws -> [SystemWindowDescription]
    func onscreenWindowOrder() throws -> [UInt64]
    func focusedWindowID() throws -> UInt64?
    func stableAppID(for pid: UInt32) -> String?
}

public struct FakeSystem: BackendSystem {
    private let availableSymbols: Set<String>
    private let accessibilityTrusted: Bool
    private let connectionID: UInt32?
    private let managedDisplaySpacesPayload: [[String: Any]]
    private let windowsBySpaceID: [UInt64: [UInt64]]
    private let windowDescriptionsByID: [UInt64: SystemWindowDescription]
    private let onscreenWindowIDs: [UInt64]
    private let focusedWindow: UInt64?
    private let stableAppIDsByPID: [UInt32: String]

    public init(
        requiredSymbols: [String],
        accessibilityTrusted: Bool,
        mainConnectionID: UInt32?,
        managedDisplaySpaces: [[String: Any]],
        windowsBySpaceID: [UInt64: [UInt64]],
        windowDescriptionsByID: [UInt64: SystemWindowDescription],
        onscreenWindowOrder: [UInt64],
        focusedWindowID: UInt64?,
        stableAppIDsByPID: [UInt32: String]
    ) {
        self.availableSymbols = Set(requiredSymbols)
        self.accessibilityTrusted = accessibilityTrusted
        self.connectionID = mainConnectionID
        self.managedDisplaySpacesPayload = managedDisplaySpaces
        self.windowsBySpaceID = windowsBySpaceID
        self.windowDescriptionsByID = windowDescriptionsByID
        self.onscreenWindowIDs = onscreenWindowOrder
        self.focusedWindow = focusedWindowID
        self.stableAppIDsByPID = stableAppIDsByPID
    }

    public func hasSymbol(_ symbol: String) -> Bool {
        availableSymbols.contains(symbol)
    }

    public func isAccessibilityTrusted() -> Bool {
        accessibilityTrusted
    }

    public func mainConnectionID() -> UInt32? {
        connectionID
    }

    public func managedDisplaySpaces() throws -> [[String: Any]] {
        managedDisplaySpacesPayload
    }

    public func windowsForSpace(_ spaceID: UInt64) throws -> [UInt64] {
        windowsBySpaceID[spaceID] ?? []
    }

    public func windowDescriptions(for windowIDs: [UInt64]) throws -> [SystemWindowDescription] {
        windowIDs.compactMap { windowDescriptionsByID[$0] }
    }

    public func onscreenWindowOrder() throws -> [UInt64] {
        onscreenWindowIDs
    }

    public func focusedWindowID() throws -> UInt64? {
        focusedWindow
    }

    public func stableAppID(for pid: UInt32) -> String? {
        stableAppIDsByPID[pid]
    }
}

private struct RawSpaceRecord {
    let managedSpaceID: UInt64
    let displayIndex: Int
    let spaceType: Int32
    let tileSpaces: [UInt64]
    let hasTileLayoutManager: Bool
    let stageManagerManaged: Bool
}

enum DesktopSnapshotBuilder {
    static func build(system: any BackendSystem) throws -> DesktopSnapshot {
        let payload = try system.managedDisplaySpaces()
        let rawSpaces = try parseManagedSpaces(payload)
        let activeSpaceIDs = try parseActiveSpaceIDs(payload)
        let visibleOrder = try Dictionary(
            uniqueKeysWithValues: system.onscreenWindowOrder().enumerated().map { ($0.element, $0.offset) }
        )

        let spaces = rawSpaces.map {
            DesktopSpaceSnapshot(
                id: $0.managedSpaceID,
                displayIndex: $0.displayIndex,
                active: activeSpaceIDs.contains($0.managedSpaceID),
                kind: classifySpace($0)
            )
        }

        var windows = [DesktopWindowSnapshot]()
        for space in rawSpaces {
            let windowIDs = try system.windowsForSpace(space.managedSpaceID)
            if activeSpaceIDs.contains(space.managedSpaceID) {
                let descriptions = try system.windowDescriptions(for: windowIDs)
                let enriched = descriptions.map { description -> SystemWindowDescription in
                    var copy = description
                    if copy.appID == nil, let pid = copy.pid {
                        copy.appID = system.stableAppID(for: pid)
                    }
                    if copy.visibleIndex == nil {
                        copy.visibleIndex = visibleOrder[copy.id]
                    }
                    return copy
                }
                let ordered = orderActiveSpaceWindows(enriched)
                windows.append(contentsOf: ordered.enumerated().map { index, window in
                    DesktopWindowSnapshot(
                        id: window.id,
                        pid: window.pid,
                        appID: window.appID,
                        title: window.title,
                        bounds: window.frame,
                        level: window.level,
                        spaceID: space.managedSpaceID,
                        orderIndex: index
                    )
                })
            } else {
                windows.append(contentsOf: windowIDs.map { windowID in
                    DesktopWindowSnapshot(
                        id: windowID,
                        pid: nil,
                        appID: nil,
                        title: nil,
                        bounds: nil,
                        level: 0,
                        spaceID: space.managedSpaceID,
                        orderIndex: nil
                    )
                })
            }
        }

        return DesktopSnapshot(
            spaces: spaces,
            windows: windows,
            focusedWindowID: try system.focusedWindowID()
        )
    }

    private static func parseManagedSpaces(_ payload: [[String: Any]]) throws -> [RawSpaceRecord] {
        var spaces = [RawSpaceRecord]()

        for (displayIndex, display) in payload.enumerated() {
            guard let displaySpaces = array(display["Spaces"]) else {
                throw BackendError.missingTopology("SLSCopyManagedDisplaySpaces")
            }

            for rawSpace in displaySpaces {
                guard let space = dictionary(rawSpace) else {
                    throw BackendError.missingTopology("SLSCopyManagedDisplaySpaces")
                }
                spaces.append(try parseRawSpaceRecord(space, displayIndex: displayIndex))
            }
        }

        return spaces
    }

    private static func parseActiveSpaceIDs(_ payload: [[String: Any]]) throws -> Set<UInt64> {
        let activeSpaceIDs = try Set(payload.map { display in
            if let active = u64(display["Current Space ID"]) {
                return active
            }
            if let active = u64(display["CurrentManagedSpaceID"]) {
                return active
            }
            if let currentSpace = dictionary(display["Current Space"]),
               let active = u64(currentSpace["ManagedSpaceID"]) ?? u64(currentSpace["id64"]) {
                return active
            }

            throw BackendError.missingTopology("SLSCopyManagedDisplaySpaces")
        })

        guard !activeSpaceIDs.isEmpty else {
            throw BackendError.missingTopology("SLSCopyManagedDisplaySpaces")
        }

        return activeSpaceIDs
    }

    private static func parseRawSpaceRecord(
        _ space: [String: Any],
        displayIndex: Int
    ) throws -> RawSpaceRecord {
        guard let managedSpaceID = u64(space["ManagedSpaceID"]),
              let spaceType = i32(space["type"])
        else {
            throw BackendError.missingTopology("SLSCopyManagedDisplaySpaces")
        }

        let tileLayoutManager = dictionary(space["TileLayoutManager"])
        let tileSpaces = array(tileLayoutManager?["TileSpaces"])?
            .compactMap { rawSpace in
                dictionary(rawSpace).flatMap { dictionary in
                    u64(dictionary["ManagedSpaceID"]) ?? u64(dictionary["id64"])
                }
            } ?? []

        return RawSpaceRecord(
            managedSpaceID: managedSpaceID,
            displayIndex: displayIndex,
            spaceType: spaceType,
            tileSpaces: tileSpaces,
            hasTileLayoutManager: tileLayoutManager != nil,
            stageManagerManaged: stageManagerManaged(space)
        )
    }
}

private func classifySpace(_ rawSpace: RawSpaceRecord) -> SpaceKind {
    if rawSpace.stageManagerManaged {
        return .stageManagerOpaque
    }
    if rawSpace.hasTileLayoutManager || !rawSpace.tileSpaces.isEmpty {
        return .splitView
    }
    if rawSpace.spaceType == fullscreenSpaceType {
        return .fullscreen
    }
    if rawSpace.spaceType == desktopSpaceType {
        return .desktop
    }
    return .system
}

private func orderActiveSpaceWindows(_ windows: [SystemWindowDescription]) -> [SystemWindowDescription] {
    windows.sorted { left, right in
        if let prefersLeft = compareVisibleIndex(left.visibleIndex, right.visibleIndex) {
            return prefersLeft
        }
        if let prefersLeft = compareLevel(left.level, right.level) {
            return prefersLeft
        }
        return left.id < right.id
    }
}

private func compareVisibleIndex(_ left: Int?, _ right: Int?) -> Bool? {
    switch (left, right) {
    case let (left?, right?) where left != right:
        return left < right
    case (_?, nil):
        return true
    case (nil, _?):
        return false
    default:
        return nil
    }
}

private func compareLevel(_ left: Int32, _ right: Int32) -> Bool? {
    guard left != right else {
        return nil
    }

    return left > right
}

func dictionary(_ value: Any?) -> [String: Any]? {
    if let dictionary = value as? [String: Any] {
        return dictionary
    }
    if let dictionary = value as? NSDictionary {
        var result = [String: Any]()
        for (key, value) in dictionary {
            guard let key = key as? String else {
                continue
            }
            result[key] = value
        }
        return result
    }
    return nil
}

func array(_ value: Any?) -> [Any]? {
    if let array = value as? [Any] {
        return array
    }
    if let array = value as? NSArray {
        return array.map { $0 }
    }
    return nil
}

func u64(_ value: Any?) -> UInt64? {
    switch value {
    case let value as UInt64:
        return value
    case let value as UInt32:
        return UInt64(value)
    case let value as UInt:
        return UInt64(value)
    case let value as Int:
        return value >= 0 ? UInt64(value) : nil
    case let value as Int32:
        return value >= 0 ? UInt64(value) : nil
    case let value as NSNumber:
        return value.uint64Value
    default:
        return nil
    }
}

func u32(_ value: Any?) -> UInt32? {
    switch value {
    case let value as UInt32:
        return value
    case let value as UInt64:
        return UInt32(exactly: value)
    case let value as Int:
        return UInt32(exactly: value)
    case let value as NSNumber:
        return value.uint32Value
    default:
        return nil
    }
}

func i32(_ value: Any?) -> Int32? {
    switch value {
    case let value as Int32:
        return value
    case let value as Int:
        return Int32(exactly: value)
    case let value as UInt64:
        return Int32(exactly: value)
    case let value as NSNumber:
        return value.int32Value
    default:
        return nil
    }
}

func cgBounds(from value: Any?) -> NativeBounds? {
    guard let rawValue = value else {
        return nil
    }

    if let bounds = dictionary(rawValue) {
        guard let x = i32(bounds["X"]),
              let y = i32(bounds["Y"]),
              let width = i32(bounds["Width"]),
              let height = i32(bounds["Height"])
        else {
            return nil
        }

        return NativeBounds(x: x, y: y, width: width, height: height)
    }

    if let bounds = rawValue as? NSDictionary,
       let rect = CGRect(dictionaryRepresentation: bounds)
    {
        return NativeBounds(
            x: Int32(rect.origin.x),
            y: Int32(rect.origin.y),
            width: Int32(rect.size.width),
            height: Int32(rect.size.height)
        )
    }

    return nil
}

private func stageManagerManaged(_ dictionary: [String: Any]) -> Bool {
    ["StageManagerManaged", "StageManagerSpace", "isStageManager", "StageManager"]
        .contains { key in
            switch dictionary[key] {
            case nil:
                return false
            case let value as Bool:
                return value
            case let value as NSNumber:
                return value.uint64Value != 0
            default:
                return true
            }
        }
}
