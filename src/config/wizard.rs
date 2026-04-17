//! Interactive configuration bootstrap.
//!
//! This module will provide a wizard that writes a template config file to
//! the user's config directory when no config is found. It will be
//! implemented in a future task (see the multi-task refactor plan — Task 8
//! calls `wizard::write_template` from `main.rs`).
