import Foundation

public enum BackendError: Error, Equatable {
    case missingRequiredSymbol(String)
    case missingAccessibilityPermission
    case missingTopologyPrecondition(String)
    case missingTopology(String)
}

enum Environment {
    static func validate(system: any BackendSystem) throws {
        for symbol in PrivateSymbols.requiredSymbols where !system.hasSymbol(symbol){
            throw BackendError.missingRequiredSymbol(symbol)
        }

        guard system.isAccessibilityTrusted() else {
            throw BackendError.missingAccessibilityPermission
        }

        guard system.mainConnectionID() != nil else {
            throw BackendError.missingTopologyPrecondition("main SkyLight connection")
        }
    }
}
