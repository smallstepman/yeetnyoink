## ADDED Requirements

### Requirement: Plugin mutation interface SHALL use framework-wrapped implementations
Contributors SHALL implement mutation behavior through `*Impl` traits, and framework wrapper traits SHALL issue mutation-completion tokens.

#### Scenario: Contributor plugin implementation
- **WHEN** a contributor implements a new domain plugin
- **THEN** the plugin compiles only when required `*Impl` methods are implemented and wrapper trait behavior remains framework-controlled

### Requirement: Topology mutation tokens SHALL be sealed
The topology-changed token type SHALL not be publicly constructible outside the core module that defines it.

#### Scenario: External token construction attempt
- **WHEN** plugin code attempts to construct a topology-changed token directly
- **THEN** compilation fails because the token constructor is not visible outside core

### Requirement: Topology-changing methods SHALL return mutation tokens
Any method that mutates domain topology SHALL return a topology-changed token through the framework wrapper API.

#### Scenario: Tear-off mutation return contract
- **WHEN** a plugin completes a tear-off mutation
- **THEN** the framework wrapper returns both payload state and a topology-changed token

### Requirement: Orchestrator SHALL enforce resync after mutation token receipt
The orchestrator SHALL consume mutation tokens and trigger topology resync before processing further routing decisions.

#### Scenario: Mutation token consumed before next command
- **WHEN** a move operation produces a topology-changed token
- **THEN** the orchestrator refreshes affected topology snapshots before the next focus/move action is evaluated
