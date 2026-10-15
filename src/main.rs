use clap::Parser;
use fiverr_agent_mcp::{
    cli::{Arguments, Command},
    storage::{Root, Store, logging::Logger},
};
fn main() {
    let native_caller = std::env::args()
        .nth(1)
        .filter(|s| s.starts_with("chrome-extension://"));
    let arguments = if let Some(caller) = native_caller {
        // Native messaging has its own binary framing; never send clap/menu text to stdout.
        Arguments {
            root: None,
            command: Some(Command::NativeSession { caller }),
        }
    } else {
        Arguments::parse()
    };
    if matches!(arguments.command, Some(Command::Version)) {
        println!("fiverr-agent-mcp {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let command = arguments.command.unwrap_or(Command::Menu);
    let bridge = matches!(&command, Command::Bridge | Command::NativeSession { .. });
    let long = matches!(
        &command,
        Command::Bridge | Command::NativeSession { .. } | Command::Http { .. }
    );
    let result = (|| {
        let root = Root::discover(arguments.root.as_deref())?;
        if let Some(code) = fiverr_agent_mcp::bootstrap::rooted_process(&root)? {
            std::process::exit(code);
        }
        let logger = match Logger::open(&root) {
            Ok(log) => log,
            Err(e) => {
                eprintln!(
                    "File logging unavailable; startup stopped safely without claiming a log was created."
                );
                return Err(e);
            }
        };
        logger.event(
            "INFO",
            "STORAGE",
            "Application started",
            serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"bridge":bridge}),
        );
        let store = Store::open(root.clone())?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|_| fiverr_agent_mcp::core::storage_error())?;
        let native = matches!(&command, Command::NativeSession { .. });
        let result = runtime.block_on(fiverr_agent_mcp::cli::execute(root, store, command));
        if native {
            // Tokio stdin uses a blocking reader; EOF may never arrive from Chrome.
            // The process exits after one frame, without waiting on that reader thread.
            runtime.shutdown_timeout(std::time::Duration::ZERO);
        }
        match &result {
            Ok(_) => logger.event(
                "INFO",
                "MCP",
                "Application stopped",
                serde_json::json!({"exit_code":0}),
            ),
            Err(e) => logger.event(
                "ERROR",
                "MCP",
                "Application operation failed",
                serde_json::json!({"code":e.code.as_str(),"exit_code":1}),
            ),
        };
        result
    })();
    match result {
        Ok(value) => {
            if !bridge && !long && !value.is_null() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into())
                );
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1)
        }
    }
}
