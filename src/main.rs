use std::time::Instant;

use filters::{grayscale, Image};
use image::{GenericImageView, ImageBuffer, Rgba};

fn main() {
    let image = image::load_from_memory(include_bytes!("landscape.jpg")).unwrap();
    let (width, height) = image.dimensions();
    let image_bytes = image.to_rgba8();

    let now = Instant::now();
    let grayscaled = image.grayscale();
    println!(
        "Using image crate, {} ms to grayscale",
        now.elapsed().as_millis()
    );
    println!("Grayscale: {:?}", grayscaled.dimensions());

    let image = Image {
        width,
        height,
        pixels: image_bytes.into_raw(),
    };

    let now = Instant::now();
    let result = pollster::block_on(grayscale(&image));
    println!(
        "Took {} ms to grayscale the image",
        now.elapsed().as_millis()
    );

    let buffer =
        ImageBuffer::<Rgba<u8>, _>::from_raw(result.width, result.height, result.pixels).unwrap();
    println!("Got the image");
    buffer.save("output.png").unwrap();
    println!("Saved");
}
