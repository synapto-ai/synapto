fn main() -> Result<(), Box<dyn std::error::Error>> {
    battery_pack::build::generate_docs()?;
    Ok(())
}
