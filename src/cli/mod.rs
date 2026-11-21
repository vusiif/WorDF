//! WorDF CLI module (arguments, font discovery, and reporting orchestration).
//! License: GPL-3.0-or-later

pub mod args;
pub mod batch;
pub mod discovery;

pub use args::{map_conversion_error_to_exit_code, parse_cli_args, TOOL_NAME, TOOL_VERSION};
pub use batch::{run_batch_conversion, BatchConfig};
pub use discovery::inspect_fonts;
