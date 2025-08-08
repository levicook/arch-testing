mod containers;
mod test_config;
mod test_context;
mod test_runner;

pub use containers::*;
pub use test_config::*;
pub use test_context::*;
pub use test_runner::*;

/// Initialize tracing for integration tests.
fn init_tracing() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

        let env_filter = EnvFilter::try_from_default_env() //
            .unwrap_or_else(|_| EnvFilter::new("info"));

        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
    });
}
