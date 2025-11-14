# Change Log

All notable changes to the "rulekit" extension will be documented in this file.

## [0.1.1] - 2025-11-14

### Fixed
- CIDR notation (e.g., `192.168.0.0/16`) no longer incorrectly parsed as regex pattern
- Regex pattern matcher now uses negative lookbehind/lookahead to avoid conflicting with CIDR slash

## [0.1.0] - 2025-11-14

### Added
- Initial release
- Syntax highlighting for `.rk` files
- Support for all Rulekit operators and literals
- Code snippets for common patterns
- Auto-closing pairs for brackets, quotes, and regex delimiters
- Comment support (line and block comments)
- Bracket matching

