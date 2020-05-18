use hound;
use std::i32;
use std::sync::Arc;
use rustfft::FFTplanner;
use rustfft::num_complex::Complex;
use rustfft::num_traits::Zero;
use sdl2::rect::Rect;
use sdl2::pixels::Color;
use sdl2::rect::Point;
use std::sync::atomic::{AtomicBool, Ordering};
use std::path::Path;
use std::fs::File;
use std::io::BufWriter;
use clap::{Arg, App};

fn load_wav_to_stereo(filename: &str) -> (Vec<f32>, Vec<f32>) {
    let mut reader = hound::WavReader::open(filename).unwrap();
    let spec = reader.spec();
    if spec.channels != 2 || spec.sample_rate != 44100 {
        panic!("Sorry, this only supports 2-channel 44.1kHz WAV.");
    }

    let samples = reader.samples::<i32>();

    let mut left = Vec::<f32>::new();
    let mut right = Vec::<f32>::new();

    let mut channel = 0;

    for sample in samples {
        if channel % 2 == 0 {
            left.push((sample.unwrap() as f32) /  2.0_f32.powf((spec.bits_per_sample - 1) as f32));
        } else {
            right.push((sample.unwrap() as f32) / 2.0_f32.powf((spec.bits_per_sample - 1) as f32));
        }
        channel = (channel + 1) % 2;
    }

    assert!(left.len() == right.len());

    (left, right)
}

fn load_wav_to_mono(filename: &str) -> Vec<f32> {
    let (l, r) = load_wav_to_stereo(filename);
    let mut sum = Vec::<f32>::new();

    for it in l.iter().zip(r.iter()) {
        let (left, right) = it;
        sum.push((left + right) / 2.0)
    }

    sum
}

trait ToImagePoint {
    fn to_image_point(&self, width: u32, height: u32) -> Point;
}

impl ToImagePoint for Complex<f32> {
    fn to_image_point(&self, width: u32, height: u32) -> Point {
        // FIXME: all this casting is probably broken in several ways
        Point::new(
            (self.re / 2.0 * (width as f32) + (width as f32)/2.0) as i32, 
            // HACK: using width to scale so it takes up the full screen horizontally, but doesn't stretch
            (self.im / 2.0 * (width as f32) + (height as f32)/2.0) as i32
        )
    }
}

fn find_sdl_gl_driver() -> Option<u32> {
    for (index, item) in sdl2::render::drivers().enumerate() {
        if item.name == "opengl" {
            return Some(index as u32);
        }
    }
    None
}

fn main() {
    let matches = App::new("fft-anything")
        .version("0.1.0")
        .author("Taylor Hornby <taylor@defuse.ca>")
        .about("Generate fourier transform visualizations from any 44.1kHz stereo WAV file")
        .arg(Arg::with_name("waveform")
            .short("-w")
            .long("waveform")
            .takes_value(false)
            .help("Draw the waveform at the bottom (slow)"))
        .arg(Arg::with_name("savepngs")
            .short("-p")
            .long("savepngs")
            .takes_value(true)
            .help("Path to directory (will be created) to save animation frame .png images in"))
        .arg(Arg::with_name("num-harmonics")
            .short("-n")
            .long("num-harmonics")
            .takes_value(true)
            .help("The number of harmonics to draw (default 1000)"))
        .arg(Arg::with_name("zoom")
            .short("-z")
            .long("zoom")
            .takes_value(true)
            .help("Scale the the animation by this factor (default 2.0)"))
        .arg(Arg::with_name("raw-vectors")
            .short("-r")
            .long("raw-vectors")
            .takes_value(false)
            .help("Draw the positive frequency vectors from the center, too"))
        .arg(Arg::with_name("speed")
            .short("-s")
            .long("speed")
            .takes_value(true)
            .help("Animation speed factor (default 0.02)"))
        // TODO: implement start time and end time by slicing that section out of the file
        //.arg(Arg::with_name("start-time")
        //    .short("-s")
        //    .long("start-time")
        //    .takes_value(true)
        //    .help("Start time (in seconds)"))
        //.arg(Arg::with_name("end-time")
        //    .short("-e")
        //    .long("end-time")
        //    .takes_value(true)
        //    .help("End time (in seconds)"))
        .arg(Arg::with_name("input")
            .multiple(false))
        .get_matches();
    
    let slow_draw_waveform = matches.is_present("waveform");
    let save_dir : Option<&str> = matches.value_of("savepngs");
    let num_harmonics : usize = matches.value_of("num-harmonics").unwrap_or("1000").parse().unwrap();
    let scale : f32 = matches.value_of("zoom").unwrap_or("2.0").parse().unwrap();
    let speed_factor : f32 = matches.value_of("speed").unwrap_or("0.02").parse().unwrap();
    let raw_vectors = matches.is_present("raw-vectors");
    let input_path = matches.value_of("input").unwrap();
    //let start_time : f32 = matches.value_of("start-time").unwrap_or("0.0").parse().unwrap();
    //let end_time : Option<f32> = if matches.is_present("end-time") {
    //    Some(matches.value_of("end-time").unwrap().parse().unwrap())
    //} else {
    //    None
    //};

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");

    let sdl_context = sdl2::init().unwrap();
    let image_width =  1920;
    let image_height = 1080;
    
    let video_subsystem = sdl_context.video().unwrap();
    let window = video_subsystem.window("Animation", image_width, image_height)
        .opengl() 
        .build()
        .unwrap();
    let mut canvas = window.into_canvas()
        .index(find_sdl_gl_driver().unwrap())
        .build()
        .unwrap();

    let input_mono = load_wav_to_mono(input_path);
    let mut input: Vec<Complex<f32>> = input_mono.iter().cloned().map(|x| Complex::new(x, 0.0)).collect();
    let mut output: Vec<Complex<f32>> = vec![Complex::zero(); input.len()];

    let mut planner = FFTplanner::new(false);
    let fft = planner.plan_fft(input.len());
    fft.process(&mut input, &mut output);

    let seconds = input.len() as f32 / 44100.0;
    let fundamental = 1.0 / seconds;

    let target_fps = 60.0;
    let frame_duration_ms = (1000.0 / target_fps) as u64;

    // https://github.com/Rust-SDL2/rust-sdl2/blob/master/examples/ttf-demo.rs
    let texture_creator = canvas.texture_creator();
    let ttf_context = sdl2::ttf::init().unwrap();
    let mut font = ttf_context.load_font("./DejaVuSans-Bold.ttf", 128).unwrap();
    font.set_style(sdl2::ttf::FontStyle::BOLD);

    // TODO: print the fundmaental frequency so the user knows what they're looking at

    if save_dir.is_some() {
            std::fs::create_dir(save_dir.unwrap()).unwrap();
    }

    let mut frame: u64 = 0;
    while running.load(Ordering::SeqCst) {
        let simulated_time_s = (frame * frame_duration_ms) as f32 / 1000.0 * speed_factor;

        if simulated_time_s > seconds /* || (end_time.is_some() && simulated_time_s > end_time.unwrap() )*/ {
            break;
        }

        canvas.set_draw_color(Color::RGB(0, 0, 0));
        canvas.clear();

        // Draw the time
        let surface = font.render(&format!("time = {:.3}s", simulated_time_s))
            .blended(Color::RGBA(255, 255, 255, 255)).unwrap();
        let texture = texture_creator.create_texture_from_surface(&surface).unwrap();
        canvas.copy(&texture, None, Some(Rect::new(100, 100, 150, 50))).unwrap();

        // Draw the status bar
        if slow_draw_waveform {
            let status_bar_height = 100;
            //      draw the waveform
            for (t, sample) in input_mono.iter().enumerate() {
                let x = (t as f32) / (input_mono.len() as f32) * (image_width as f32);
                let y = image_height as f32 - (status_bar_height as f32)/2.0 + sample * (status_bar_height as f32)/2.0;
                canvas.set_draw_color(Color::RGB(255, 255, 255));
                canvas.fill_rect(Rect::new(x as i32, y as i32, 1, 1)).unwrap();
            }
            //      draw the cursor
            canvas.set_draw_color(Color::RGB(255, 255, 0));
            canvas.fill_rect(Rect::new((simulated_time_s / seconds * (image_width as f32)) as i32, (image_height - 100) as i32, 3, 100)).unwrap();
        }

        // Start at the origin.
        let mut last_pos : Complex<f32> = Complex::zero();

        if raw_vectors {
            // Draw the positive frequency harmonic vectors themselves in the background
            for i in 0..num_harmonics {
                let frequency = fundamental * (i as f32);
                let harmonic = scale * output[i] / (output.len() as f32) * (Complex::i() * frequency * simulated_time_s * std::f32::consts::PI * 2.0).exp();

                canvas.set_draw_color(Color::RGB(100, 100, 100));
                canvas.draw_line(Complex::zero().to_image_point(image_width, image_height), (1.0*harmonic).to_image_point(image_width, image_height)).unwrap();
            }
        }

        // Positive frequencies
        for i in 0..num_harmonics {
            let frequency = fundamental * (i as f32);
            let harmonic = scale * output[i] / (output.len() as f32) * (Complex::i() * frequency * simulated_time_s * std::f32::consts::PI * 2.0).exp();
            let cur_pos : Complex<f32> = last_pos + harmonic;

            // Draw a line between last_pos and cur_pos
            canvas.set_draw_color(Color::RGB(0, 200, 200));
            canvas.draw_line(last_pos.to_image_point(image_width, image_height), cur_pos.to_image_point(image_width, image_height)).unwrap();

            last_pos = cur_pos;
        }

        // Negative frequencies
        canvas.set_draw_color(Color::RGB(200, 0, 200));
        for i in 1..num_harmonics {
            let frequency = -fundamental * (i as f32);
            let harmonic = scale * output[output.len() - i] / (output.len() as f32) * (Complex::i() * frequency * simulated_time_s  * std::f32::consts::PI * 2.0).exp();
            let cur_pos : Complex<f32> = last_pos + harmonic;

            // Draw a line between last_pos and cur_pos
            canvas.draw_line(last_pos.to_image_point(image_width, image_height), cur_pos.to_image_point(image_width, image_height)).unwrap();

            last_pos = cur_pos;
        }

        // Highlight the final point
        let last_pos_point = last_pos.to_image_point(image_width, image_height);
        canvas.set_draw_color(Color::RGB(255, 255, 0));
        canvas.fill_rect(
            Rect::new(
                last_pos_point.x() - 4, last_pos_point.y() - 4,
                8, 8
            )
        ).unwrap();

        canvas.present();

        if save_dir.is_some() {
            // Save to frames/<num>.png
            let pixels = canvas.read_pixels(Rect::new(0, 0, image_width, image_height), sdl2::pixels::PixelFormatEnum::RGB24).unwrap();

            let filename = format!("{}/{:06}.png", save_dir.unwrap(), frame);
            let path = Path::new(&filename);
            let file = File::create(path).unwrap();
            let ref mut w = BufWriter::new(file);
            let mut encoder = png::Encoder::new(w, image_width, image_height);
            encoder.set_color(png::ColorType::RGB);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&pixels).unwrap();
        } else {
            std::thread::sleep(std::time::Duration::from_millis(frame_duration_ms));
        }
        frame += 1;
    }
}
