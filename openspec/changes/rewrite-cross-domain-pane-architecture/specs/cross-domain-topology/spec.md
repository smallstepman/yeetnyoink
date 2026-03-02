## ADDED Requirements

### Requirement: Domain plugins SHALL provide normalized topology snapshots
Each registered domain plugin SHALL expose a topology snapshot containing stable native pane/window identifiers, domain ownership, and absolute rectangle geometry for all exported leaves.

#### Scenario: Snapshot export from nested domains
- **WHEN** the orchestrator requests snapshots for a focused stack (WM -> terminal -> editor)
- **THEN** each domain returns a normalized snapshot with native IDs and absolute rectangles for its exported leaves

### Requirement: Orchestrator SHALL maintain a global domain tree and flattened leaves
The system SHALL maintain a global domain containment tree and a flattened `GlobalLeaf` set derived from plugin snapshots for directional solving.

#### Scenario: Building global topology
- **WHEN** snapshots are collected for all active domains
- **THEN** the orchestrator materializes a domain tree and flattened global leaves without relying on runtime string tags for domain semantics

### Requirement: Topology-changing operations SHALL trigger topology rebuild
Any operation that mutates layout topology SHALL cause the orchestrator to rebuild affected domain snapshots before handling the next routing decision.

#### Scenario: Rebuild after mutation
- **WHEN** a plugin reports a successful tear-off or merge mutation
- **THEN** the orchestrator refreshes topology for affected domains before executing subsequent focus/move logic
