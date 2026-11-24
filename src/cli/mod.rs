//! WorDF CLI module (arguments, font discovery, and reporting orchestration).
//! License: GPL-3.0-or-later

pub mod analyze_route;
pub mod args;
pub mod batch;
pub mod discovery;
pub mod single;

#[allow(unused_imports)]
pub use args::{map_conversion_error_to_exit_code, parse_cli_args, TOOL_NAME, TOOL_VERSION};
pub use batch::{run_batch_conversion, BatchConfig};
#[allow(unused_imports)]
pub use discovery::{inspect_font_chain, inspect_fonts};
pub use single::{run_single_conversion, SingleConfig};
