mod app;
mod bootstrap;
mod http;
mod infrastructure;
mod modules;
mod observability;
mod workers;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    bootstrap::app::run().await
}
