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
            // HACK: using width to scale so it takes up the full screen horizontally, but doesn't distort
            (self.im / 2.0 * (width as f32) + (height as f32)/2.0) as i32
        )
    }
}

/*
struct RGB {
    r: u8,
    g: u8,
    b: u8
}

struct Image {
    pixels: Vec<Vec<RGB>>
}

impl Image {
    pub fn new(width: usize, height: usize) {
        let mut columns: Vec<Vec<RGB>> = Vec::new();
        for x in 0..width {
            let mut row: Vec<RGB> = Vec::new();
            for y in 0..height {
                row.push(RGB {r: 0, g: 0, b: 0})
            }
            columns.push(row)
        }
    }

    pub fn set(&mut self, x: usize, y: usize, color: RGB) {
        self.pixels[x][y] = color;
    }

    pub fn get(&self, x: usize, y: usize) -> RGB {
        self.pixels[x][y]
    }


}

fn draw_frame(image: TODO, path: &str) {
    let path = Path::new(path);
    let file = File::create(path).unwrap();
    let ref mut w = BufWriter::new(file);

    let mut encoder = png::Encoder::new(w, image.width(), image.height());
    encoder.set_color(png::ColorType::RGBA);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();

    let data = image.to_rgba();
    writer.write_image_data(&data).unwrap();
}

*/

fn find_sdl_gl_driver() -> Option<u32> {
    for (index, item) in sdl2::render::drivers().enumerate() {
        if item.name == "opengl" {
            return Some(index as u32);
        }
    }
    None
}

fn main() {

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");

    let sdl_context = sdl2::init().unwrap();
    //let image_width = 1920;
    //let image_height = 1080;
    let image_width =  1280;
    let image_height = 720;
    
    let video_subsystem = sdl_context.video().unwrap();
    let window = video_subsystem.window("Animation", image_width, image_height)
        .opengl() // this line DOES NOT enable opengl, but allows you to create/get an OpenGL context from your window.
        .build()
        .unwrap();
    let mut canvas = window.into_canvas()
        .index(find_sdl_gl_driver().unwrap())
        .build()
        .unwrap();

    canvas.set_draw_color(Color::RGB(0, 0, 0));
    // fills the canvas with the color we set in `set_draw_color`.
    canvas.clear();

    // change the color of our drawing with a gold-color ...
    canvas.set_draw_color(Color::RGB(255, 210, 0));
    // A draw a rectangle which almost fills our window with it !
    canvas.fill_rect(Rect::new(10, 10, 780, 580)).unwrap();

    // However the canvas has not been updated to the window yet,
    // everything has been processed to an internal buffer,
    // but if we want our buffer to be displayed on the window,
    // we need to call `present`. We need to call this everytime
    // we want to render a new frame on the window.
    canvas.present();


    let input_mono = load_wav_to_mono("4s.wav");
    let max = input_mono.iter().cloned().fold(0./0., f32::max);
    print!("max: {}", max);
    let mut input: Vec<Complex<f32>> = input_mono.iter().cloned().map(|x| Complex::new(x, 0.0)).collect();
    let mut output: Vec<Complex<f32>> = vec![Complex::zero(); input.len()];

    let mut planner = FFTplanner::new(false);
    let fft = planner.plan_fft(input.len());
    fft.process(&mut input, &mut output);

    let seconds = input.len() as f32 / 44100.0;
    let fundamental = 1.0 / seconds;

    /*
    print!("Seconds: {}\n", seconds);
    for i in 0..output.len() {
        print!("{} Hz: {}, {}\n", i as f32 * fundamental, output[i].norm(), output[i].arg() / std::f32::consts::PI * 180.0);
    }
    */
    // let num_frames = (seconds * target_fps / speed_factor) as u64;

    let speed_factor = 0.02;
    let target_fps = 60.0;
    let frame_duration_ms = (1000.0 / target_fps) as u64;
    let scale : f32 = 2.0;

    assert!(output.len() % 2 == 0);

    // https://github.com/Rust-SDL2/rust-sdl2/blob/master/examples/ttf-demo.rs
    let texture_creator = canvas.texture_creator();
    let ttf_context = sdl2::ttf::init().unwrap();
    let mut font = ttf_context.load_font("./DejaVuSans-Bold.ttf", 128).unwrap();
    font.set_style(sdl2::ttf::FontStyle::BOLD);

    let render = true;

    let mut frame: u64 = 0;
    while running.load(Ordering::SeqCst) {
        let simulated_time_s = (frame * frame_duration_ms) as f32 / 1000.0 * speed_factor;

        if simulated_time_s > seconds {
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
        if true {
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

        // Draw the harmonic vectors themselves in the background

        for i in 0..1000 {
            let frequency = fundamental * (i as f32);
            let harmonic = scale * output[i] / (output.len() as f32) * (Complex::i() * frequency * simulated_time_s * std::f32::consts::PI * 2.0).exp();

            // Aside: draw the harmonic vector itself
            canvas.set_draw_color(Color::RGB(100, 100, 100));
            canvas.draw_line(Complex::zero().to_image_point(image_width, image_height), (1.0*harmonic).to_image_point(image_width, image_height)).unwrap();
        }

        // Draw only the lowest 1000 harmonics.
        for i in 0..1000 {
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
        for i in 1..1000 {
            let frequency = -fundamental * (i as f32);
            let harmonic = scale * output[output.len() - i] / (output.len() as f32) * (Complex::i() * frequency * simulated_time_s  * std::f32::consts::PI * 2.0).exp();
            let cur_pos : Complex<f32> = last_pos + harmonic;

            // Draw a line between last_pos and cur_pos
            canvas.draw_line(last_pos.to_image_point(image_width, image_height), cur_pos.to_image_point(image_width, image_height)).unwrap();

            last_pos = cur_pos;
        }

        // highlight the final point
        let last_pos_point = last_pos.to_image_point(image_width, image_height);
        canvas.set_draw_color(Color::RGB(255, 255, 0));
        canvas.fill_rect(
            Rect::new(
                last_pos_point.x() - 4, last_pos_point.y() - 4,
                8, 8
            )
        ).unwrap();

        canvas.present();
        if render {
            let pixels = canvas.read_pixels(Rect::new(0, 0, image_width, image_height), sdl2::pixels::PixelFormatEnum::RGB24).unwrap();

            let filename = format!("frames/{:06}.png", frame);
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
