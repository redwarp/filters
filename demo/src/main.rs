use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::Result;
use filters::{Image, Resize};
use image::{codecs::png::PngEncoder, GenericImageView, ImageEncoder};
use oxipng::Options;

const GRAYSCALE: &str = "grayscale";
const INVERSE: &str = "inverse";
const HORIZONTAL_FLIP: &str = "hflip";
const VERTICAL_FLIP: &str = "vflip";
const HALF: &str = "half";
const BOX_BLUR: &str = "boxblur";
const GAUSSIAN_BLUR: &str = "gaussianblur";

fn main() -> Result<()> {
    let input = "sample/sushi.png";
    let image = image::open(input)?;

    let (width, height) = image.dimensions();

    let image = Image {
        width,
        height,
        pixels: bytemuck::cast_slice(&image.to_rgba8().into_raw()).to_vec(),
    };
    let filters = [
        GRAYSCALE,
        INVERSE,
        HORIZONTAL_FLIP,
        VERTICAL_FLIP,
        HALF,
        BOX_BLUR,
        GAUSSIAN_BLUR,
    ];

    for filter in filters {
        let output = output_file(input, filter);

        let now = Instant::now();
        let mut operation = pollster::block_on(image.operation());

        operation = match filter {
            GRAYSCALE => (operation.grayscale()),
            INVERSE => (operation.inverse()),
            HORIZONTAL_FLIP => (operation.hflip()),
            VERTICAL_FLIP => (operation.vflip()),
            HALF => {
                let (width, height) = operation.dimensions();
                operation.resize((width / 2, height / 2), Resize::Linear)
            }
            BOX_BLUR => operation.box_blur(9),
            GAUSSIAN_BLUR => operation.gaussian_blur(3.0),
            _ => operation,
        };
        let image = pollster::block_on(operation.execute());

        println!(
            "Took {} ms to apply the filter to the image",
            now.elapsed().as_millis()
        );

        let mut encoded = vec![];
        let png_encoder = PngEncoder::new(encoded.by_ref());
        png_encoder.write_image(
            image.as_raw(),
            image.width,
            image.height,
            image::ColorType::Rgba8,
        )?;
        let optimized = oxipng::optimize_from_memory(&encoded, &Options::from_preset(5))?;
        fs::write(output, optimized)?;
    }
    Ok(())
}

fn output_file(input: &str, filter: &str) -> PathBuf {
    let path = Path::new(input);
    let parent = path.parent();
    let stem = path
        .file_stem()
        .expect("Expecting .jpg or .png files")
        .to_string_lossy();
    let extension = path
        .extension()
        .expect("Expecting .jpg or .png files")
        .to_string_lossy();

    let filename = format!("{}_{}.{}", stem, filter, extension);
    let output_path = if let Some(parent) = parent {
        let parent = parent.join("output");
        fs::create_dir_all(&parent).unwrap();
        parent.join(filename)
    } else {
        Path::new(&filename).to_path_buf()
    };

    output_path
}

#[cfg(test)]
mod tests {
    use crate::output_file;

    #[test]
    fn output_file_name_no_specified() {
        let file_path = output_file("sunflower.png", "grayscale");

        assert_eq!("sunflower_grayscale.png", file_path.to_string_lossy());
    }
}
