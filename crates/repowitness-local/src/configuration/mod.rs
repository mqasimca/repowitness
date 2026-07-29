//! Strict bounded TOML configuration admission for local files.

mod dto;
mod parser;

pub use parser::{
    ConfigurationFileError, ConfigurationFileLayer, MAX_CONFIGURATION_FILE_BYTES,
    MAX_CONFIGURATION_TEXT_BYTES, parse_configuration_file,
};

#[cfg(test)]
mod tests;
