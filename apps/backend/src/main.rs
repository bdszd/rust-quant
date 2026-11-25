#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    quant_backend::bootstrap::app::run().await
}
