## ADDED Requirements

### Requirement: No network client dependencies
The supply-chain check SHALL fail when the application's dependency graph gains one of
the HTTP, WebSocket or TLS client crates named in `deny.toml`. (It cannot see a connection
opened without such a crate; that is for review.)

#### Scenario: Pull request adds an HTTP client
- **WHEN** a pull request adds a dependency on `reqwest`
- **THEN** the "Licences & advisories" check fails naming the banned crate
