## ADDED Requirements

### Requirement: Pane transfer payloads SHALL be open and extensible
Transferable pane state SHALL be represented via an open trait-based payload model, allowing new domain integrations without modifying a closed core enum.

#### Scenario: Add new payload type without core enum edits
- **WHEN** a contributor adds a new domain-specific pane state type
- **THEN** the type can be registered and used for transfer without editing a central payload enum

### Requirement: Transfer conversion registry SHALL support source-target negotiation
The system SHALL support registration and lookup of payload converters keyed by source and target payload types.

#### Scenario: Conversion path exists
- **WHEN** a source payload type differs from the target domain’s accepted type
- **THEN** the orchestrator retrieves and applies a registered converter before merge-in

### Requirement: Tear-off and merge SHALL preserve transferable state
Cross-domain movement SHALL capture source pane state at tear-off, negotiate conversion if required, and merge the converted state into the selected target domain.

#### Scenario: Tear-off from editor into WM-managed target
- **WHEN** the target domain differs from the source domain
- **THEN** the orchestrator tears off the source payload, converts if needed, and merges into the target domain with preserved state

### Requirement: Unsupported transfer SHALL trigger explicit fallback
If no compatible transfer path exists, the system SHALL execute an explicit fallback strategy and report the transfer incompatibility.

#### Scenario: Missing converter fallback
- **WHEN** no direct or registered conversion path is available for a transfer
- **THEN** the orchestrator runs the configured fallback spawn strategy and emits a structured incompatibility reason
