use std::{path::Path, time::Instant};

use anyhow::Result;
use clap::{app_from_crate, Arg};
use filters::Image;
use image::{GenericImageView, ImageBuffer, Rgba};

const GRAYSCALE: &str = "grayscale";
const INVERSE: &str = "inverse";
const HFLIP: &str = "hflip";
const VFLIP: &str = "vflip";

fn main() -> Result<()> {
    let matches = app_from_crate!()
        .arg(
            Arg::new("input")
                .long("input")
                .short('i')
                .required(true)
                .takes_value(true)
                .validator(|input| {
                    if input.ends_with(".png") || input.ends_with(".jpg") {
                        Ok(())
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
                .takes_value(true),
        )
        .arg(
            Arg::new("filter")
                .long("filter")
                .possible_values(["grayscale", "inverse", "hflip", "vflip"])
                .required(true)
                .multiple_values(true),
        )
        .get_matches();

    let input = matches.value_of("input").expect("Input is required");
    let image = image::open(input)?;
    let filters = matches.values_of("filter").expect("Filter is required");
    let filter_contat = filters.clone().collect::<Vec<_>>().join("_");
    let output = output_file_name(matches.value_of("output"), input, &filter_contat);

    let (width, height) = image.dimensions();

    let image = Image {
        width,
        height,
        pixels: image.to_rgba8().into_raw(),
    };

    let now = Instant::now();
    let mut operation = pollster::block_on(image.operation());

    for filter in filters {
        operation = match filter {
            GRAYSCALE => (operation.grayscale()),
            INVERSE => (operation.inverse()),
            HFLIP => (operation.hflip()),
            VFLIP => (operation.vflip()),
            _ => operation,
        };
    }
    let image = pollster::block_on(operation.execute());

    println!(
        "Took {} ms to apply the filter to the image",
        now.elapsed().as_millis()
    );

    let buffer =
        ImageBuffer::<Rgba<u8>, _>::from_raw(image.width, image.height, image.pixels).unwrap();
    buffer.save(output).unwrap();

    Ok(())
}

fn output_file_name(output: Option<&str>, input: &str, filter: &str) -> String {
    if let Some(output) = output {
        output.to_owned()
    } else {
        let path = Path::new(input);
        let parent = path.parent();
        let stem = path.file_stem().unwrap().to_string_lossy();
        let extension = path.extension().unwrap().to_string_lossy();

        let filename = format!("{}_{}.{}", stem, filter, extension);
        let output_path = if let Some(parent) = parent {
            parent.join(filename)
        } else {
            Path::new(&filename).to_path_buf()
        };

        output_path.to_string_lossy().to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::output_file_name;

    #[test]
    fn output_file_name_no_specified() {
        let file_path = output_file_name(None, "sunflower.png", "grayscale");

        assert_eq!("sunflower_grayscale.png", file_path);
    }

    #[test]
    fn output_file_name_output_specified() {
        let file_path = output_file_name(Some("output.png"), "sunflower.png", "grayscale");

        assert_eq!("output.png", file_path);
    }
}
