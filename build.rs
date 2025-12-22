use std::env;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

fn main() {
    // Only run on Windows
    if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "windows" {
        return;
    }

    let out_dir = env::var("OUT_DIR").unwrap();
    let ico_path = Path::new(&out_dir).join("icon.ico");

    // Create ICO from PNG
    if let Err(e) = create_ico(&ico_path) {
        println!("cargo:warning=Failed to create icon: {}", e);
        return;
    }

    // Embed icon and version info using winres
    let mut res = winres::WindowsResource::new();
    res.set_icon(ico_path.to_str().unwrap());

    // Set version info
    res.set("ProductName", "Phoenix");
    res.set("FileDescription", "Phoenix - CDDA Game Launcher");
    res.set("LegalCopyright", "MIT License");

    if let Err(e) = res.compile() {
        println!("cargo:warning=Failed to compile Windows resources: {}", e);
    }
}

/// Create an ICO file from the PNG icon
fn create_ico(output_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use image::imageops::FilterType;

    let png_data = include_bytes!("assets/icon.png");
    let img = image::load_from_memory(png_data)?;

    // ICO sizes: 16, 32, 48, 256
    let sizes: &[u32] = &[16, 32, 48, 256];

    let file = fs::File::create(output_path)?;
    let mut writer = BufWriter::new(file);

    // ICO header
    writer.write_all(&[0, 0])?; // Reserved
    writer.write_all(&[1, 0])?; // Type: 1 = ICO
    writer.write_all(&(sizes.len() as u16).to_le_bytes())?; // Number of images

    // Calculate offsets
    let header_size = 6 + sizes.len() * 16; // 6 byte header + 16 bytes per image entry
    let mut image_data: Vec<Vec<u8>> = Vec::new();
    let mut current_offset = header_size;

    // Generate PNG data for each size
    for &size in sizes {
        let resized = img.resize_exact(size, size, FilterType::Lanczos3);
        let mut png_bytes = Vec::new();
        let mut cursor = std::io::Cursor::new(&mut png_bytes);
        resized.write_to(&mut cursor, image::ImageFormat::Png)?;
        image_data.push(png_bytes);
    }

    // Write directory entries
    for (i, &size) in sizes.iter().enumerate() {
        let width = if size >= 256 { 0 } else { size as u8 };
        let height = if size >= 256 { 0 } else { size as u8 };

        writer.write_all(&[width])?; // Width (0 = 256)
        writer.write_all(&[height])?; // Height (0 = 256)
        writer.write_all(&[0])?; // Color palette (0 = no palette)
        writer.write_all(&[0])?; // Reserved
        writer.write_all(&[1, 0])?; // Color planes
        writer.write_all(&[32, 0])?; // Bits per pixel
        writer.write_all(&(image_data[i].len() as u32).to_le_bytes())?; // Size of image data
        writer.write_all(&(current_offset as u32).to_le_bytes())?; // Offset to image data

        current_offset += image_data[i].len();
    }

    // Write image data
    for data in &image_data {
        writer.write_all(data)?;
    }

    writer.flush()?;
    Ok(())
}
