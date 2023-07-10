mod device;
mod bus;
mod gpio;
mod tests;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    Ok(())
}