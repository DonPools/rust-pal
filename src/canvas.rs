use crate::pal;

#[derive(Clone, Copy)]
pub struct Color(pub u32);
impl Color {
    /// Create a new Color from RGB values.
    #[inline]
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Color {
        let (r, g, b) = (r as u32, g as u32, b as u32);
        Color((r << 16) | (g << 8) | b)
    }

    #[inline]
    pub fn to_rgb(self) -> (u8, u8, u8) {
        let r = ((self.0 >> 16) & 0xFF) as u8;
        let g = ((self.0 >> 8) & 0xFF) as u8;
        let b = (self.0 & 0xFF) as u8;

        (r, g, b)
    }
}

#[derive(Clone)]
pub struct Palette {
    pub colors: Vec<Color>,
}

impl Palette {
    pub fn new() -> Self {
        Self {
            colors: vec![Color(0); 256],
        }
    }

    pub fn with_colors(colors: Vec<Color>) -> Self {
        Self { colors }
    }
}

pub struct Canvas {
    width: usize,
    height: usize,

    palette: Palette,

    pixels: Vec<u8>,
    buffer: Vec<u32>,
    //window: Window,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            palette: Palette::new(),
            pixels: vec![0; width * height],
            buffer: vec![0; width * height],
        }
    }

    pub fn set_palette(&mut self, palette: &Palette) {
        self.palette = palette.clone();
    }

    pub fn set_pixels<F: FnOnce(&mut [u8])>(&mut self, f: F) {
        f(&mut self.pixels);
    }

    pub fn refresh_buffer(&mut self) -> &[u32] {
        for i in 0..self.pixels.len() {
            self.buffer[i] = self.palette.colors[self.pixels[i] as usize].0;
        }

        &self.buffer
    }
}
