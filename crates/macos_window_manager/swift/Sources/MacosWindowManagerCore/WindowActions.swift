import Foundation

public enum BackendOperationError: Error, Equatable {
    case missingSpace(UInt64)
    case missingWindow(UInt64)
    case missingWindowFrame(UInt64)
    case missingWindowPID(UInt64)
    case unsupportedStageManagerSpace(UInt64)
    case noDirectionalFocusTarget(NativeDirection)
    case noDirectionalMoveTarget(NativeDirection)
    case callFailed(String)
}

public extension Backend {
    func focusWindow(_ windowID: UInt64) throws {
        try actionSystem().focusWindow(windowID)
    }

    func focusWindowWithKnownPID(_ windowID: UInt64, pid: UInt32) throws {
        try actionSystem().focusWindowWithKnownPID(windowID, pid: pid)
    }

    func focusWindowInActiveSpaceWithKnownPID(
        _ windowID: UInt64,
        pid: UInt32,
        targetHint: ActiveSpaceFocusTargetHint?
    ) throws {
        let actionSystem = try actionSystem()

        do {
            try actionSystem.focusWindowWithKnownPID(windowID, pid: pid)
        } catch let error as BackendOperationError {
            guard case .missingWindow(let missingWindowID) = error, missingWindowID == windowID else {
                throw error
            }

            if let remappedTargetID = try activeSpaceRemappedSamePIDTarget(
                actionSystem: actionSystem,
                targetWindowID: windowID,
                pid: pid,
                targetHint: targetHint
            ) {
                try actionSystem.focusWindowWithKnownPID(remappedTargetID, pid: pid)
                return
            }

            throw BackendOperationError.missingWindow(windowID)
        }
    }

    func focusSameSpaceTargetInSnapshot(
        snapshot: DesktopSnapshot,
        direction: NativeDirection,
        targetWindowID: UInt64
    ) throws {
        let actionSystem = try actionSystem()
        let focusTargetID = splitViewSameSpaceFocusTarget(snapshot: snapshot, direction: direction) ?? targetWindowID

        guard let pid = snapshot.window(id: focusTargetID)?.pid else {
            try actionSystem.focusWindow(focusTargetID)
            return
        }

        try focusSameSpaceTargetWithKnownPID(
            actionSystem: actionSystem,
            snapshot: snapshot,
            direction: direction,
            targetWindowID: focusTargetID,
            pid: pid
        )
    }

    func moveWindowToSpace(_ windowID: UInt64, spaceID: UInt64) throws {
        try actionSystem().moveWindowToSpace(windowID, spaceID: spaceID)
    }

    func swapWindowFrames(
        sourceWindowID: UInt64,
        sourceFrame: NativeBounds,
        targetWindowID: UInt64,
        targetFrame: NativeBounds
    ) throws {
        try actionSystem().swapWindowFrames(
            sourceWindowID: sourceWindowID,
            sourceFrame: sourceFrame,
            targetWindowID: targetWindowID,
            targetFrame: targetFrame
        )
    }
}

extension Backend {
    func actionSystem() throws -> any BackendActionSystem {
        guard let actionSystem = system as? any BackendActionSystem else {
            throw BackendOperationError.callFailed("backend_action_system")
        }
        return actionSystem
    }

    func activeSpaceRemappedSamePIDTarget(
        actionSystem: any BackendActionSystem,
        targetWindowID: UInt64,
        pid: UInt32,
        targetHint: ActiveSpaceFocusTargetHint?
    ) throws -> UInt64? {
        let snapshot = try topologySnapshot()
        let targetBounds = snapshot.window(id: targetWindowID)?.bounds ?? targetHint?.bounds
        let targetSpaceID = snapshot.window(id: targetWindowID)?.spaceID ?? targetHint?.spaceID

        guard let targetBounds, let targetSpaceID else {
            return nil
        }
        guard snapshot.space(id: targetSpaceID)?.kind == .splitView else {
            return nil
        }
        if let targetWindow = snapshot.window(id: targetWindowID), targetWindow.pid != pid {
            return nil
        }

        let axWindowIDs = Set(try actionSystem.axWindowIDs(for: pid))
        let candidates = snapshot.windows.filter { window in
            window.id != targetWindowID
                && window.spaceID == targetSpaceID
                && window.pid == pid
                && isDirectionalFocusWindow(window)
                && window.bounds != nil
                && axWindowIDs.contains(window.id)
        }
        return bestTargetMatch(targetBounds: targetBounds, candidates: candidates)?.id
    }

    func focusSameSpaceTargetWithKnownPID(
        actionSystem: any BackendActionSystem,
        snapshot: DesktopSnapshot,
        direction: NativeDirection,
        targetWindowID: UInt64,
        pid: UInt32
    ) throws {
        let focused = resolvedFocusedWindow(in: snapshot)
        let samePIDSplitView = focused?.pid == pid
            && focused.flatMap { snapshot.space(id: $0.spaceID) }?.kind == .splitView

        var focusTargetID = targetWindowID
        var axWindowIDs = Set<UInt64>()

        if samePIDSplitView {
            axWindowIDs = Set(try actionSystem.axWindowIDs(for: pid))
            if !axWindowIDs.contains(targetWindowID),
               let remappedTargetID = nativeAXBackedSamePIDTarget(
                   snapshot: snapshot,
                   direction: direction,
                   pid: pid,
                   axWindowIDs: axWindowIDs
               ),
               remappedTargetID != targetWindowID
            {
                focusTargetID = remappedTargetID
            }
        }

        do {
            try actionSystem.focusWindowWithKnownPID(focusTargetID, pid: pid)
        } catch let error as BackendOperationError {
            guard case .missingWindow(let missingWindowID) = error, missingWindowID == focusTargetID else {
                throw error
            }

            if samePIDSplitView,
               let remappedTargetID = nativeAXBackedSamePIDTarget(
                   snapshot: snapshot,
                   direction: direction,
                   pid: pid,
                   axWindowIDs: axWindowIDs.isEmpty ? Set(try actionSystem.axWindowIDs(for: pid)) : axWindowIDs
               ),
               remappedTargetID != focusTargetID
            {
                try actionSystem.focusWindowWithKnownPID(remappedTargetID, pid: pid)
                return
            }

            throw BackendOperationError.missingWindow(focusTargetID)
        }
    }
}

extension DesktopSnapshot {
    var activeSpaceIDs: Set<UInt64> {
        Set(spaces.lazy.filter(\.active).map(\.id))
    }

    func space(id: UInt64) -> DesktopSpaceSnapshot? {
        spaces.first { $0.id == id }
    }

    func window(id: UInt64) -> DesktopWindowSnapshot? {
        windows.first { $0.id == id }
    }
}

private func resolvedFocusedWindow(in snapshot: DesktopSnapshot) -> DesktopWindowSnapshot? {
    if let focusedWindowID = snapshot.focusedWindowID, let focusedWindow = snapshot.window(id: focusedWindowID) {
        return focusedWindow
    }

    return snapshot.windows
        .filter { snapshot.activeSpaceIDs.contains($0.spaceID) }
        .min(by: isHigherPriorityActiveWindow)
}

private func splitViewSameSpaceFocusTarget(
    snapshot: DesktopSnapshot,
    direction: NativeDirection
) -> UInt64? {
    guard let focusedWindow = resolvedFocusedWindow(in: snapshot) else {
        return nil
    }
    return splitViewSameSpaceFocusTarget(
        snapshot: snapshot,
        sourceWindowID: focusedWindow.id,
        direction: direction
    )
}

private func splitViewSameSpaceFocusTarget(
    snapshot: DesktopSnapshot,
    sourceWindowID: UInt64,
    direction: NativeDirection
) -> UInt64? {
    guard let focusedWindow = snapshot.window(id: sourceWindowID),
          snapshot.space(id: focusedWindow.spaceID)?.kind == .splitView,
          let sourceBounds = focusedWindow.bounds
    else {
        return nil
    }

    let candidates = snapshot.windows.filter { window in
        window.id != focusedWindow.id
            && window.spaceID == focusedWindow.spaceID
            && isDirectionalFocusWindow(window)
            && window.bounds.map { candidateExtendsInDirection(source: sourceBounds, candidate: $0, direction: direction) } == true
    }
    return bestEdgeWindow(candidates: candidates, direction: direction)?.id
}

private func nativeAXBackedSamePIDTarget(
    snapshot: DesktopSnapshot,
    direction: NativeDirection,
    pid: UInt32,
    axWindowIDs: Set<UInt64>
) -> UInt64? {
    guard let focusedWindow = resolvedFocusedWindow(in: snapshot),
          focusedWindow.pid == pid,
          snapshot.space(id: focusedWindow.spaceID)?.kind == .splitView,
          let sourceBounds = focusedWindow.bounds
    else {
        return nil
    }

    let candidates = snapshot.windows.filter { window in
        window.id != focusedWindow.id
            && window.spaceID == focusedWindow.spaceID
            && window.pid == pid
            && isDirectionalFocusWindow(window)
            && axWindowIDs.contains(window.id)
            && window.bounds.map { candidateExtendsInDirection(source: sourceBounds, candidate: $0, direction: direction) } == true
    }
    return bestEdgeWindow(candidates: candidates, direction: direction)?.id
}

private func bestTargetMatch(
    targetBounds: NativeBounds,
    candidates: [DesktopWindowSnapshot]
) -> DesktopWindowSnapshot? {
    candidates.reduce(nil) { best, candidate in
        guard let best else {
            return candidate
        }
        return isBetterTargetMatch(candidate, than: best, targetBounds: targetBounds) ? candidate : best
    }
}

private func bestEdgeWindow(
    candidates: [DesktopWindowSnapshot],
    direction: NativeDirection
) -> DesktopWindowSnapshot? {
    candidates.reduce(nil) { best, candidate in
        guard let best else {
            return candidate
        }
        return isBetterEdgeWindow(candidate, than: best, direction: direction) ? candidate : best
    }
}

private func isDirectionalFocusWindow(_ window: DesktopWindowSnapshot) -> Bool {
    window.level == 0
}

private func candidateExtendsInDirection(
    source: NativeBounds,
    candidate: NativeBounds,
    direction: NativeDirection
) -> Bool {
    switch direction {
    case .west:
        candidate.x < source.x
    case .east:
        candidate.x + candidate.width > source.x + source.width
    case .north:
        candidate.y < source.y
    case .south:
        candidate.y + candidate.height > source.y + source.height
    }
}

private func isBetterTargetMatch(
    _ left: DesktopWindowSnapshot,
    than right: DesktopWindowSnapshot,
    targetBounds: NativeBounds
) -> Bool {
    guard let leftBounds = left.bounds, let rightBounds = right.bounds else {
        return right.bounds != nil
    }

    let leftOverlap = overlapArea(leftBounds, targetBounds)
    let rightOverlap = overlapArea(rightBounds, targetBounds)
    if leftOverlap != rightOverlap {
        return leftOverlap > rightOverlap
    }

    let leftDistance = centerDistanceSquared(leftBounds, targetBounds)
    let rightDistance = centerDistanceSquared(rightBounds, targetBounds)
    if leftDistance != rightDistance {
        return leftDistance < rightDistance
    }

    return isHigherPriorityActiveWindow(left, right)
}

private func isBetterEdgeWindow(
    _ left: DesktopWindowSnapshot,
    than right: DesktopWindowSnapshot,
    direction: NativeDirection
) -> Bool {
    guard let leftBounds = left.bounds, let rightBounds = right.bounds else {
        return right.bounds != nil
    }

    let leftMetric: Int32
    let rightMetric: Int32
    switch direction {
    case .east:
        leftMetric = leftBounds.x + leftBounds.width
        rightMetric = rightBounds.x + rightBounds.width
        if leftMetric != rightMetric {
            return leftMetric > rightMetric
        }
    case .west:
        leftMetric = leftBounds.x
        rightMetric = rightBounds.x
        if leftMetric != rightMetric {
            return leftMetric < rightMetric
        }
    case .north:
        leftMetric = leftBounds.y
        rightMetric = rightBounds.y
        if leftMetric != rightMetric {
            return leftMetric < rightMetric
        }
    case .south:
        leftMetric = leftBounds.y + leftBounds.height
        rightMetric = rightBounds.y + rightBounds.height
        if leftMetric != rightMetric {
            return leftMetric > rightMetric
        }
    }

    return isHigherPriorityActiveWindow(left, right)
}

private func isHigherPriorityActiveWindow(
    _ left: DesktopWindowSnapshot,
    _ right: DesktopWindowSnapshot
) -> Bool {
    switch (left.orderIndex, right.orderIndex) {
    case let (.some(leftIndex), .some(rightIndex)) where leftIndex != rightIndex:
        return leftIndex < rightIndex
    case (.some, nil):
        return true
    case (nil, .some):
        return false
    default:
        return left.id < right.id
    }
}

private func overlapArea(_ left: NativeBounds, _ right: NativeBounds) -> Int64 {
    overlapLength(left.x, left.width, right.x, right.width)
        * overlapLength(left.y, left.height, right.y, right.height)
}

private func overlapLength(_ startA: Int32, _ lengthA: Int32, _ startB: Int32, _ lengthB: Int32) -> Int64 {
    let endA = startA + lengthA
    let endB = startB + lengthB
    return Int64(max(0, min(endA, endB) - max(startA, startB)))
}

private func centerDistanceSquared(_ left: NativeBounds, _ right: NativeBounds) -> Int64 {
    let leftCenterX = Int64(left.x) + Int64(left.width) / 2
    let leftCenterY = Int64(left.y) + Int64(left.height) / 2
    let rightCenterX = Int64(right.x) + Int64(right.width) / 2
    let rightCenterY = Int64(right.y) + Int64(right.height) / 2
    let deltaX = leftCenterX - rightCenterX
    let deltaY = leftCenterY - rightCenterY
    return deltaX * deltaX + deltaY * deltaY
}
