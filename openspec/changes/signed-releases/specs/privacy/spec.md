## ADDED Requirements

### Requirement: No network client dependencies
The supply-chain check SHALL fail when the application's dependency graph gains an
HTTP or network client crate.

#### Scenario: Pull request adds an HTTP client
- **WHEN** a pull request adds a dependency on `reqwest`
- **THEN** the "Licences & advisories" check fails naming the banned crate
