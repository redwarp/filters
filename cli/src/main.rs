use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::Result;
use clap::Arg;
use filters::{Filters, Image, Resize};
use image::{GenericImageView, ImageBuffer, Rgba};
use pollster::FutureExt;

const GRAYSCALE: &str = "grayscale";
const INVERSE: &str = "inverse";
const HORIZONTAL_FLIP: &str = "hflip";
const VERTICAL_FLIP: &str = "vflip";
const HALF: &str = "half";
const BOX_BLUR: &str = "boxblur";
const GAUSSIAN_BLUR: &str = "gaussianblur";

fn main() -> Result<()> {
    let matches = clap::command!()
        .arg(
            Arg::new("input")
                .long("input")
                .short('i')
                .required(true)
                .num_args(1)
                .value_parser(|input: &str| {
                    if (input.ends_with(".png") || input.ends_with(".jpg"))
                        && PathBuf::from(&input).exists()
                    {
                        Ok(input.to_owned())
                    } else {
                        Err(String::from("Filters only support png or jpg files"))
                    }
                }),
        )
        .arg(
            Arg::new("output")
                .long("output")
                .short('o')
                .required(false)
                .num_args(1),
        )
        .arg(
            Arg::new("filter")
                .long("filter")
                .value_parser([
                    GRAYSCALE,
                    INVERSE,
                    HORIZONTAL_FLIP,
                    VERTICAL_FLIP,
                    HALF,
                    BOX_BLUR,
                    GAUSSIAN_BLUR,
                ])
                .required(true)
                .num_args(1..),
        )
        .get_matches();

    let input = matches
        .get_one::<String>("input")
        .expect("Input is required");
    let image = image::open(input)?;
    let filter_list: Vec<String> = matches
        .get_many::<String>("filter")
        .expect("Filter is required")
        .cloned()
        .collect();

    let filter_concat = filter_list.join("_");
    let output = output_file(
        matches.get_one::<String>("output").map(|x| &**x),
        input,
        &filter_concat,
    );

    let (width, height) = image.dimensions();

    let image = Image {
        width,
        height,
        pixels: bytemuck::cast_slice(&image.to_rgba8().into_raw()).to_vec(),
    };

    let filters = Filters::new().block_on();
    let now = Instant::now();
    let mut operation = image.operation(&filters);

    for filter in filter_list {
        operation = match filter.as_str() {
            GRAYSCALE => operation.grayscale(),
            INVERSE => operation.inverse(),
            HORIZONTAL_FLIP => operation.hflip(),
            VERTICAL_FLIP => operation.vflip(),
            HALF => {
                let (width, height) = operation.dimensions();
                operation.resize((width / 2, height / 2), Resize::Linear)
            }
            BOX_BLUR => operation.box_blur(15),
            GAUSSIAN_BLUR => operation.gaussian_blur(3.0),
            _ => operation,
        };
    }
    let image = operation.execute().block_on();

    println!(
        "Took {} ms to apply the filter to the image",
        now.elapsed().as_millis()
    );

    let buffer =
        ImageBuffer::<Rgba<u8>, _>::from_raw(image.width, image.height, image.as_raw()).unwrap();
    buffer.save(output).unwrap();

    Ok(())
}

fn output_file(output: Option<&str>, input: &str, filter: &str) -> PathBuf {
    if let Some(output) = output {
        Path::new(output).to_owned()
    } else {
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
            parent.join(filename)
        } else {
            Path::new(&filename).to_path_buf()
        };

        output_path
    }
}

#[cfg(test)]
mod tests {
    use crate::output_file;

    #[test]
    fn output_file_name_no_specified() {
        let file_path = output_file(None, "sunflower.png", "grayscale");

        assert_eq!("sunflower_grayscale.png", file_path.to_string_lossy());
    }

    #[test]
    fn output_file_name_output_specified() {
        let file_path = output_file(Some("output.png"), "sunflower.png", "grayscale");

        assert_eq!("output.png", file_path.to_string_lossy());
    }
}
