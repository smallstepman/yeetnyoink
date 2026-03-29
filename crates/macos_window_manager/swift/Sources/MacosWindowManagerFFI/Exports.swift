import MacosWindowManagerCore

@_cdecl("mwm_backend_smoke_test")
public func mwm_backend_smoke_test() -> Int32 {
    Backend.smokeTest()
}
