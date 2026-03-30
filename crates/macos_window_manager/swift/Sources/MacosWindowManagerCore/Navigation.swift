import Foundation

private let spaceSwitchSettleTimeout: TimeInterval = 0.3
private let spaceSwitchPollInterval: TimeInterval = 0.01
private let spaceSwitchStableTargetPolls = 3

public extension Backend {
    func switchSpace(_ spaceID: UInt64) throws {
        try actionSystem().switchSpace(spaceID)
    }

    func switchAdjacentSpace(_ direction: NativeDirection, targetSpaceID: UInt64) throws {
        try actionSystem().switchAdjacentSpace(direction, targetSpaceID: targetSpaceID)
    }

    func switchSpaceInSnapshot(
        snapshot: DesktopSnapshot,
        targetSpaceID: UInt64,
        adjacentDirection: NativeDirection?
    ) throws {
        guard let targetSpace = snapshot.space(id: targetSpaceID) else {
            throw BackendOperationError.missingSpace(targetSpaceID)
        }
        if targetSpace.kind == SpaceKind.stageManagerOpaque {
            throw BackendOperationError.unsupportedStageManagerSpace(targetSpaceID)
        }
        if snapshot.activeSpaceIDs.contains(targetSpaceID) {
            return
        }

        let transitionWindowIDs = outerSpaceTransitionWindowIDs(snapshot: snapshot, targetSpaceID: targetSpaceID)
        if let adjacentDirection {
            if transitionWindowIDs.targetWindowIDs.isEmpty {
                try switchSpace(targetSpaceID)
                try waitForSpacePresentation(
                    targetSpaceID: targetSpaceID,
                    sourceFocusWindowID: transitionWindowIDs.sourceFocusWindowID,
                    targetWindowIDs: transitionWindowIDs.targetWindowIDs
                )
                return
            }

            try switchAdjacentSpace(adjacentDirection, targetSpaceID: targetSpaceID)
            do {
                try waitForSpacePresentation(
                    targetSpaceID: targetSpaceID,
                    sourceFocusWindowID: transitionWindowIDs.sourceFocusWindowID,
                    targetWindowIDs: transitionWindowIDs.targetWindowIDs
                )
                return
            } catch {
                let waitError = operationError(for: error)
                let targetStillInactive: Bool
                do {
                    targetStillInactive = try !currentActiveSpaceIDs().contains(targetSpaceID)
                } catch {
                    targetStillInactive = true
                }

                if !targetStillInactive {
                    throw waitError
                }

                let retryTargetWindowIDs: Set<UInt64>
                do {
                    let onscreenWindowIDs = try currentOnscreenWindowIDs()
                    if !transitionWindowIDs.targetWindowIDs.isEmpty
                        && !transitionWindowIDs.targetWindowIDs.isDisjoint(with: onscreenWindowIDs)
                    {
                        retryTargetWindowIDs = []
                    } else {
                        retryTargetWindowIDs = transitionWindowIDs.targetWindowIDs
                    }
                } catch {
                    retryTargetWindowIDs = transitionWindowIDs.targetWindowIDs
                }

                try switchSpace(targetSpaceID)
                try waitForSpacePresentation(
                    targetSpaceID: targetSpaceID,
                    sourceFocusWindowID: transitionWindowIDs.sourceFocusWindowID,
                    targetWindowIDs: retryTargetWindowIDs
                )
                return
            }
        }

        try switchSpace(targetSpaceID)
        try waitForSpacePresentation(
            targetSpaceID: targetSpaceID,
            sourceFocusWindowID: transitionWindowIDs.sourceFocusWindowID,
            targetWindowIDs: transitionWindowIDs.targetWindowIDs
        )
    }
}

private extension Backend {
    func waitForSpacePresentation(
        targetSpaceID: UInt64,
        sourceFocusWindowID: UInt64?,
        targetWindowIDs: Set<UInt64>
    ) throws {
        let deadline = Date().addingTimeInterval(spaceSwitchSettleTimeout)
        var stableTargetPolls = 0

        while true {
            let activeSpaceIDs = try currentActiveSpaceIDs()
            let onscreenWindowIDs = try currentOnscreenWindowIDs()
            let targetActive = activeSpaceIDs.contains(targetSpaceID)
            let sourceFocusHidden = sourceFocusWindowID.map { !onscreenWindowIDs.contains($0) } ?? true
            let targetVisible = targetWindowIDs.isEmpty || !targetWindowIDs.isDisjoint(with: onscreenWindowIDs)

            if targetActive && targetVisible {
                stableTargetPolls += 1
            } else {
                stableTargetPolls = 0
            }

            if targetActive
                && targetVisible
                && (sourceFocusHidden || stableTargetPolls >= spaceSwitchStableTargetPolls)
            {
                return
            }

            if Date() >= deadline {
                throw BackendOperationError.callFailed("wait_for_active_space")
            }

            Thread.sleep(forTimeInterval: spaceSwitchPollInterval)
        }
    }

    func currentActiveSpaceIDs() throws -> Set<UInt64> {
        do {
            return try activeSpaceIDs(from: system.managedDisplaySpaces())
        } catch {
            throw operationError(for: error)
        }
    }

    func currentOnscreenWindowIDs() throws -> Set<UInt64> {
        do {
            return Set(try system.onscreenWindowOrder())
        } catch {
            throw operationError(for: error)
        }
    }
}

private func outerSpaceTransitionWindowIDs(
    snapshot: DesktopSnapshot,
    targetSpaceID: UInt64
) -> (sourceFocusWindowID: UInt64?, targetWindowIDs: Set<UInt64>) {
    let targetDisplayIndex = snapshot.space(id: targetSpaceID)?.displayIndex
    let sourceSpaceID = targetDisplayIndex.flatMap { displayIndex in
        snapshot.spaces.first {
            $0.active && $0.displayIndex == displayIndex && $0.id != targetSpaceID
        }?.id
    }
    let sourceFocusWindowID: UInt64?
    if let focusedWindowID = snapshot.focusedWindowID,
       snapshot.window(id: focusedWindowID)?.spaceID == sourceSpaceID
    {
        sourceFocusWindowID = focusedWindowID
    } else {
        sourceFocusWindowID = nil
    }
    let targetWindowIDs = Set(snapshot.windows.lazy.filter { $0.spaceID == targetSpaceID }.map(\.id))
    return (sourceFocusWindowID, targetWindowIDs)
}

private func activeSpaceIDs(from payload: [[String: Any]]) throws -> Set<UInt64> {
    let activeSpaceIDs = try Set(payload.map { display in
        if let active = u64(display["Current Space ID"]) {
            return active
        }
        if let active = u64(display["CurrentManagedSpaceID"]) {
            return active
        }
        if let currentSpace = dictionary(display["Current Space"]),
           let active = u64(currentSpace["ManagedSpaceID"]) ?? u64(currentSpace["id64"])
        {
            return active
        }

        throw BackendError.missingTopology("SLSCopyManagedDisplaySpaces")
    })

    guard !activeSpaceIDs.isEmpty else {
        throw BackendError.missingTopology("SLSCopyManagedDisplaySpaces")
    }

    return activeSpaceIDs
}

private func operationError(for error: Error) -> BackendOperationError {
    if let error = error as? BackendOperationError {
        return error
    }
    if let error = error as? BackendError {
        switch error {
        case let .missingRequiredSymbol(symbol):
            return .callFailed(symbol)
        case .missingAccessibilityPermission:
            return .callFailed("AXIsProcessTrusted")
        case let .missingTopologyPrecondition(precondition):
            return .callFailed(precondition)
        case let .missingTopology(probe):
            return .callFailed(probe)
        }
    }

    return .callFailed(String(describing: error))
}
