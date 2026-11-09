use super::args::{McpArgs, McpTarget};
use super::error::{CliError, Result};

pub(crate) fn mcp(a: &McpArgs) -> Result {
    let backend: Box<dyn iridium_cad_mcp::Backend> = match a.target() {
        McpTarget::Connect { addr, lazy: true, launch } => Box::new(iridium_cad_mcp::Remote::lazy(&addr, launch)),
        McpTarget::Connect { addr, .. } => match iridium_cad_mcp::Remote::connect(&addr) {
            Ok(r) => Box::new(r),
            Err(source) => return Err(CliError::McpConnect { addr, source }),
        },
        McpTarget::Headless => Box::new(iridium_cad_mcp::Headless::default()),
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    iridium_cad_mcp::Server::new(backend).serve(stdin.lock(), stdout.lock())?;
    Ok(())
}
