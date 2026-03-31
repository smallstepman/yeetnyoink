import Foundation

public enum BackendError: Error, Equatable {
    case missingRequiredSymbol(String)
    case missingAccessibilityPermission
    case missingTopologyPrecondition(String)
    case missingTopology(String)
}

enum Environment {
    static func validate(system: any BackendSystem) throws {
        try validate(
            system: system,
            requiredSymbols: PrivateSymbols.requiredSymbols,
            requiresMainConnection: true
        )
    }

    static func validateFastFocus(system: any BackendSystem) throws {
        try validate(
            system: system,
            requiredSymbols: PrivateSymbols.fastFocusRequiredSymbols,
            requiresMainConnection: false
        )
    }

    private static func validate(
        system: any BackendSystem,
        requiredSymbols: [String],
        requiresMainConnection: Bool
    ) throws {
        for symbol in requiredSymbols where !system.hasSymbol(symbol) {
            throw BackendError.missingRequiredSymbol(symbol)
        }

        guard system.isAccessibilityTrusted() else {
            throw BackendError.missingAccessibilityPermission
        }

        if requiresMainConnection {
            guard system.mainConnectionID() != nil else {
                throw BackendError.missingTopologyPrecondition("main SkyLight connection")
            }
        }
    }
}
