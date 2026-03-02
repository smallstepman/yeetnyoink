## ADDED Requirements

### Requirement: Directional navigation SHALL be geometry-driven
The system SHALL resolve directional focus and move targets using pane/window rectangles in global coordinate space rather than tree-walk sibling traversal.

#### Scenario: Horizontal move target resolution
- **WHEN** a user issues `move west` from a focused leaf
- **THEN** the solver selects candidates that are physically west of the focused leaf and chooses the nearest valid candidate by geometry

### Requirement: Neighbor selection SHALL enforce directional and overlap constraints
The solver SHALL only consider candidates whose receiving edge lies in the requested direction and that overlap on the perpendicular axis.

#### Scenario: Reject diagonal candidate
- **WHEN** a candidate lies west but has no vertical overlap with the focused leaf
- **THEN** the solver excludes that candidate from neighbor selection

### Requirement: Tie-breaker behavior SHALL be deterministic
If multiple candidates have equal directional distance, the solver SHALL apply a deterministic tie-breaker based on perpendicular proximity.

#### Scenario: Deterministic tie in stacked candidates
- **WHEN** two westward candidates have the same receiving-edge distance
- **THEN** the solver chooses the candidate with minimum perpendicular offset to the focused leaf

### Requirement: Domain routing SHALL be evaluated after neighbor selection
The system SHALL perform geometry neighbor selection first, then determine same-domain or cross-domain action routing using source/target domain ownership.

#### Scenario: Cross-domain boundary resolution
- **WHEN** the selected neighbor belongs to a different domain
- **THEN** the orchestrator executes cross-domain routing logic rather than same-domain internal focus/move commands
